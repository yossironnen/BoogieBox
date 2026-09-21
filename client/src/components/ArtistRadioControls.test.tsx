/**
 * Tests the Artist Radio split button, options popover/sheet, and queue reason chip.
 */

import React from 'react';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  ArtistRadioSplitButton,
  DEFAULT_RADIO_OPTIONS,
  MoodIcon,
  RADIO_MOODS,
  RadioReasonChip,
  loadRadioPrefs,
  radioReasonText,
  radioTargetShares,
  saveRadioPrefs,
} from './ArtistRadioControls';
import type { ArtistRadioOptionsSnapshot, RadioMoodBucket } from '../types';

const { apiMock } = vi.hoisted(() => ({ apiMock: { artistRadioOptions: vi.fn() } }));
vi.mock('../api', () => ({ api: apiMock }));

const snapshot = (overrides: Partial<ArtistRadioOptionsSnapshot> = {}): ArtistRadioOptionsSnapshot => ({
  tags: ['trip-hop'],
  autoMoods: ['melancholic', 'dreamy'],
  moods: RADIO_MOODS.map(({ bucket }) => ({
    bucket,
    available: bucket !== 'aggressive',
    auto: bucket === 'melancholic' || bucket === 'dreamy',
  })),
  libraryTagProgress: { tagged: 82, candidates: 100 },
  ...overrides,
});

beforeEach(() => {
  vi.clearAllMocks();
  window.localStorage.clear();
  apiMock.artistRadioOptions.mockResolvedValue(snapshot());
});

const renderButton = (props: Partial<React.ComponentProps<typeof ArtistRadioSplitButton>> = {}) => {
  const onStart = vi.fn();
  const utils = render(
    <ArtistRadioSplitButton artistId="7" artistName="Massive Attack" loading={false} onStart={onStart} {...props} />,
  );
  return { onStart, ...utils };
};

const openOptions = async () => {
  fireEvent.click(screen.getByRole('button', { name: 'Radio options' }));
  await screen.findByRole('dialog', { name: /Artist Radio options for Massive Attack/ });
  await waitFor(() => expect(screen.getByText(/82% of your library tagged/)).toBeInTheDocument());
};

describe('radioTargetShares', () => {
  it('yields the documented mixes and always sums to 1', () => {
    const [seed, similar, mood] = radioTargetShares('similar', 0.45);
    expect([seed, similar, mood].map((n) => Math.round(n * 100))).toEqual([30, 45, 25]);
    expect(radioTargetShares('mood', 0.45).map((n) => Math.round(n * 100))).toEqual([10, 25, 65]);
    for (const focus of ['similar', 'mood'] as const) {
      for (const v of [-1, 0, 0.45, 1, 5]) {
        const shares = radioTargetShares(focus, v);
        expect(shares.reduce((a, b) => a + b, 0)).toBeCloseTo(1, 9);
        expect(shares[0]).toBeGreaterThanOrEqual(0.05 - 1e-9);
      }
    }
    expect(radioTargetShares('similar', 1)[0]).toBeLessThan(radioTargetShares('similar', 0)[0]);
  });
});

describe('radio preferences', () => {
  it('round-trips focus and variety but never moods', () => {
    expect(loadRadioPrefs()).toEqual(DEFAULT_RADIO_OPTIONS);
    saveRadioPrefs({ focus: 'mood', moods: ['dark'], variety: 0.8 });
    expect(loadRadioPrefs()).toEqual({ focus: 'mood', moods: [], variety: 0.8 });
  });

  it('falls back to defaults for corrupt, out-of-range, or unreadable storage', () => {
    window.localStorage.setItem('bb.artistRadio.prefs', '{not json');
    expect(loadRadioPrefs()).toEqual(DEFAULT_RADIO_OPTIONS);
    window.localStorage.setItem('bb.artistRadio.prefs', JSON.stringify({ focus: 'weird', variety: 7 }));
    expect(loadRadioPrefs()).toEqual(DEFAULT_RADIO_OPTIONS);
    const getItem = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('blocked'); });
    expect(loadRadioPrefs()).toEqual(DEFAULT_RADIO_OPTIONS);
    getItem.mockRestore();
    const setItem = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('blocked'); });
    expect(() => saveRadioPrefs(DEFAULT_RADIO_OPTIONS)).not.toThrow();
    setItem.mockRestore();
  });
});

describe('RadioReasonChip', () => {
  it('describes each reason kind with an icon and tooltip', () => {
    const { rerender } = render(<RadioReasonChip reason={{ kind: 'seed', label: 'Massive Attack' }} />);
    let chip = screen.getByTestId('radio-reason-chip');
    expect(chip).toHaveTextContent('Seed');
    expect(chip).toHaveAttribute('title', 'Seed artist');
    expect(chip.querySelector('svg')).not.toBeNull();

    rerender(<RadioReasonChip reason={{ kind: 'similar', label: 'Massive Attack' }} />);
    chip = screen.getByTestId('radio-reason-chip');
    expect(chip).toHaveTextContent('Similar');
    expect(chip).toHaveAttribute('title', 'Similar to Massive Attack');

    rerender(<RadioReasonChip reason={{ kind: 'mood', label: 'melancholic' }} />);
    expect(screen.getByTestId('radio-reason-chip')).toHaveTextContent('Melancholic');

    rerender(<RadioReasonChip reason={{ kind: 'style', label: 'trip-hop' }} />);
    expect(screen.getByTestId('radio-reason-chip')).toHaveTextContent('Trip-hop');
    expect(radioReasonText({ kind: 'mood', label: 'chill' })).toBe('Chill');
    // A mood label the client doesn't know still renders (generic tag icon).
    rerender(<RadioReasonChip reason={{ kind: 'mood', label: 'mystery' }} />);
    expect(screen.getByTestId('radio-reason-chip')).toHaveTextContent('Mystery');
  });

  it('has a distinct glyph for every mood bucket', () => {
    const markup = RADIO_MOODS.map(({ bucket }) => {
      const { container, unmount } = render(<MoodIcon bucket={bucket} />);
      const html = container.innerHTML;
      unmount();
      return html;
    });
    expect(new Set(markup).size).toBe(RADIO_MOODS.length);
    const { container } = render(<MoodIcon bucket={'nope' as RadioMoodBucket} />);
    expect(container.querySelector('svg')).not.toBeNull();
  });
});

describe('ArtistRadioSplitButton', () => {
  it('starts immediately with saved focus/variety and auto moods when the main half is clicked', () => {
    saveRadioPrefs({ focus: 'mood', moods: [], variety: 0.7 });
    const { onStart } = renderButton();
    fireEvent.click(screen.getByRole('button', { name: /Play Artist Radio/ }));
    expect(onStart).toHaveBeenCalledWith({ focus: 'mood', moods: [], variety: 0.7 });
    expect(apiMock.artistRadioOptions).not.toHaveBeenCalled();
  });

  it('shows a spinner and blocks starting while loading', () => {
    const { onStart, container } = renderButton({ loading: true });
    const main = screen.getByRole('button', { name: /Play Artist Radio/ });
    expect(main).toBeDisabled();
    expect(container.querySelector('.icon-action-spinner')).not.toBeNull();
    fireEvent.click(main);
    expect(onStart).not.toHaveBeenCalled();
  });

  it('opens the options popover, loads the artist snapshot, and marks auto moods', async () => {
    renderButton();
    const chevron = screen.getByRole('button', { name: 'Radio options' });
    expect(chevron).toHaveAttribute('aria-expanded', 'false');
    await openOptions();
    expect(apiMock.artistRadioOptions).toHaveBeenCalledWith('7');
    expect(chevron).toHaveAttribute('aria-expanded', 'true');

    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getByText(/auto-picked from this artist/)).toBeInTheDocument();
    const melancholic = within(dialog).getByRole('button', { name: /Melancholic/ });
    expect(melancholic).toHaveAttribute('aria-pressed', 'true');
    expect(within(dialog).getByRole('button', { name: /Dreamy/ })).toHaveAttribute('aria-pressed', 'true');
    expect(within(dialog).getByRole('button', { name: /^Chill/ })).toHaveAttribute('aria-pressed', 'false');
    // A mood with few tagged tracks is dimmed with an explanation, but still selectable.
    expect(within(dialog).getByRole('button', { name: /Aggressive/ })).toHaveAttribute('title', expect.stringMatching(/Few tagged tracks/));
    expect(within(dialog).getAllByText('auto')).toHaveLength(2);
  });

  it('lets the user override moods, focus and variety, previews the mix, and starts with those options', async () => {
    const { onStart } = renderButton();
    await openOptions();
    const dialog = screen.getByRole('dialog');

    // Toggling an auto chip off starts from the auto selection, then removes it.
    fireEvent.click(within(dialog).getByRole('button', { name: /Dreamy/ }));
    expect(within(dialog).getByRole('button', { name: /Dreamy/ })).toHaveAttribute('aria-pressed', 'false');
    expect(within(dialog).getByRole('button', { name: /Melancholic/ })).toHaveAttribute('aria-pressed', 'true');
    expect(within(dialog).queryByText(/auto-picked/)).toBeNull();
    fireEvent.click(within(dialog).getByRole('button', { name: /Chill/ }));

    fireEvent.click(within(dialog).getByRole('button', { name: /^Mood$/ }));
    expect(within(dialog).getByRole('button', { name: /^Mood$/ })).toHaveAttribute('aria-pressed', 'true');
    expect(within(dialog).getByRole('img', { name: /mood and style 65%/ })).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole('button', { name: /Artist \+ similar/ }));
    expect(within(dialog).getByRole('img', { name: /Massive Attack 30%, similar artists 45%/ })).toBeInTheDocument();

    fireEvent.change(within(dialog).getByRole('slider'), { target: { value: '1' } });
    expect(within(dialog).getByRole('slider')).toHaveAttribute('aria-valuetext', 'Adventurous');
    fireEvent.change(within(dialog).getByRole('slider'), { target: { value: '0' } });
    expect(within(dialog).getByRole('slider')).toHaveAttribute('aria-valuetext', 'Familiar');
    fireEvent.change(within(dialog).getByRole('slider'), { target: { value: '0.5' } });
    expect(within(dialog).getByRole('slider')).toHaveAttribute('aria-valuetext', 'Balanced');
    // Toggling a selected chip back off.
    fireEvent.click(within(dialog).getByRole('button', { name: /Chill/ }));

    fireEvent.click(within(dialog).getByRole('button', { name: 'Start radio' }));
    expect(onStart).toHaveBeenCalledWith({ focus: 'similar', moods: ['melancholic'], variety: 0.5 });
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(loadRadioPrefs()).toEqual({ focus: 'similar', moods: [], variety: 0.5 });
  });

  it('remembers focus and variety the next time the popover opens', async () => {
    saveRadioPrefs({ focus: 'mood', moods: [], variety: 0.9 });
    renderButton();
    await openOptions();
    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getByRole('button', { name: /^Mood$/ })).toHaveAttribute('aria-pressed', 'true');
    expect(within(dialog).getByRole('slider')).toHaveValue('0.9');
  });

  it('closes on the chevron, Escape, and outside clicks', async () => {
    renderButton();
    const chevron = screen.getByRole('button', { name: 'Radio options' });
    await openOptions();
    fireEvent.click(chevron);
    expect(screen.queryByRole('dialog')).toBeNull();

    await openOptions();
    fireEvent.keyDown(document, { key: 'Enter' });
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();

    await openOptions();
    fireEvent.mouseDown(screen.getByRole('dialog'));
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    fireEvent.mouseDown(document.body);
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('degrades gracefully when the options cannot be loaded', async () => {
    apiMock.artistRadioOptions.mockRejectedValueOnce(new Error('offline'));
    const { onStart } = renderButton();
    fireEvent.click(screen.getByRole('button', { name: 'Radio options' }));
    const dialog = await screen.findByRole('dialog');
    await act(async () => { await Promise.resolve(); });
    expect(within(dialog).queryByText(/library tagged/)).toBeNull();
    expect(within(dialog).queryByText(/auto-picked/)).toBeNull();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Start radio' }));
    expect(onStart).toHaveBeenCalledWith({ focus: 'similar', moods: [], variety: 0.45 });
  });

  it('hides tag coverage when the library has nothing to measure', async () => {
    apiMock.artistRadioOptions.mockResolvedValue(snapshot({ libraryTagProgress: { tagged: 0, candidates: 0 }, autoMoods: [] }));
    renderButton();
    fireEvent.click(screen.getByRole('button', { name: 'Radio options' }));
    const dialog = await screen.findByRole('dialog');
    await waitFor(() => expect(apiMock.artistRadioOptions).toHaveBeenCalled());
    expect(within(dialog).queryByText(/library tagged/)).toBeNull();
    expect(within(dialog).queryByText(/auto-picked/)).toBeNull();
  });

  it('renders as a bottom sheet with a scrim on mobile and closes from the scrim', async () => {
    renderButton({ presentation: 'sheet' });
    await openOptions();
    const scrim = screen.getByTestId('radio-sheet-scrim');
    expect(screen.getByRole('dialog')).toHaveStyle({ position: 'fixed', bottom: '0px' });
    fireEvent.click(scrim);
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('disables the popover start button while a radio is loading and ignores stale option responses', async () => {
    let resolve: (value: ArtistRadioOptionsSnapshot) => void = () => undefined;
    apiMock.artistRadioOptions.mockReturnValueOnce(new Promise<ArtistRadioOptionsSnapshot>((r) => { resolve = r; }));
    const { rerender, onStart } = renderButton();
    fireEvent.click(screen.getByRole('button', { name: 'Radio options' }));
    const dialog = await screen.findByRole('dialog');
    rerender(<ArtistRadioSplitButton artistId="7" artistName="Massive Attack" loading onStart={onStart} />);
    expect(within(dialog).getByRole('button', { name: 'Start radio' })).toBeDisabled();
    // Close before the response arrives: it must not update state or throw.
    fireEvent.keyDown(document, { key: 'Escape' });
    await act(async () => { resolve(snapshot()); await Promise.resolve(); });
    expect(screen.queryByRole('dialog')).toBeNull();
  });
});
