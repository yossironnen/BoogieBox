/**
 * Tests the steering pill row with its hook: loading, tapping, the limit, failures, the queue prompt, and the
 * status chip.
 */

// @vitest-environment jsdom

import React from 'react';
import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { apiMock } = vi.hoisted(() => ({
  apiMock: { playlists: { steer: vi.fn(), setSteer: vi.fn() } },
}));

vi.mock('../api', () => ({ api: apiMock }));

import SteerPills, { SteerAddedDot, SteerChangedChip } from './SteerPills';
import { useSteer, type SteerState } from '../hooks/useSteer';
import type { SteerPill, SteerUpdate, SteerView } from '../smartSteer';
import type { PlaylistSnapshot } from '../types';

const sludge: SteerPill = { kind: 'style', tag: 'sludge', direction: 'more', count: 86 };
const doom: SteerPill = { kind: 'style', tag: 'doom metal', direction: 'less', count: 40 };
const deeper: SteerPill = { kind: 'style', tag: 'rock', direction: 'deeper', count: 132 };
const popular: SteerPill = { kind: 'signal', signal: 'popular', count: 25 };

function view(over: Partial<SteerView> = {}): SteerView {
  return {
    active: [], pills: [deeper, sludge, doom, popular], pool: 247, tagged: 247, fewTags: false,
    popularSource: 'local', addedTrackIds: [], editRevision: 3, ...over,
  };
}

const snapshot = { playlist: { id: 'p1' }, tracks: [], membershipRevision: 2, refreshError: null } as unknown as PlaylistSnapshot;
const answer = (over: Partial<SteerView>): SteerUpdate => ({ steer: view(over), snapshot });

let latest: SteerState;
function Harness({ playlistId = 'p1', enabled = true, queueHere = false, mobile = false, onSnapshot = () => {}, onUpdateQueue = () => {} }: {
  playlistId?: string; enabled?: boolean; queueHere?: boolean; mobile?: boolean;
  onSnapshot?: (s: PlaylistSnapshot) => void; onUpdateQueue?: () => void;
}) {
  const steer = useSteer(playlistId, enabled, onSnapshot);
  latest = steer;
  return (
    <>
      <SteerChangedChip steer={steer} />
      <SteerPills steer={steer} mobile={mobile} queueIsThisPlaylist={queueHere} onUpdateQueue={onUpdateQueue} />
    </>
  );
}

const pill = (name: string) => screen.getByRole('button', { name });

describe('SteerPills', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMock.playlists.steer.mockResolvedValue(view());
  });
  afterEach(() => cleanup());

  it('offers the pills with their wording, tooltips and state', async () => {
    render(<Harness />);
    const group = await screen.findByRole('group', { name: 'Steer this playlist' });
    expect(within(group).getAllByRole('button').map(b => b.textContent)).toEqual([
      'Go deeper into rock', 'More sludge', 'Less doom metal', 'Most played here',
    ]);
    expect(pill('More sludge')).toHaveAttribute('aria-pressed', 'false');
    expect(pill('More sludge')).toHaveAttribute('title', '86 tracks');
    expect(pill('Go deeper into rock')).toHaveAttribute('title', '132 tracks in narrower kinds of rock');
    expect(pill('Most played here')).toHaveAttribute('title', '25 tracks played on this server');
    expect(screen.queryByRole('button', { name: 'Reset' })).toBeNull();
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('shows nothing when there is nothing to offer, and reads nothing when turned off', async () => {
    apiMock.playlists.steer.mockResolvedValue(view({ pills: [] }));
    const { container } = render(<Harness />);
    await waitFor(() => expect(latest.view).not.toBeNull());
    expect(container).toBeEmptyDOMElement();

    cleanup();
    apiMock.playlists.steer.mockClear();
    render(<Harness enabled={false} />);
    expect(apiMock.playlists.steer).not.toHaveBeenCalled();
  });

  it('stays quiet when pills can not be read', async () => {
    apiMock.playlists.steer.mockRejectedValue(new Error('busy'));
    const { container } = render(<Harness />);
    await waitFor(() => expect(apiMock.playlists.steer).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });

  it('turns a pill on at once, then shows what changed and hands over the tracks', async () => {
    let finish: (u: SteerUpdate) => void = () => {};
    apiMock.playlists.setSteer.mockImplementation(() => new Promise(resolve => { finish = resolve; }));
    const onSnapshot = vi.fn();
    render(<Harness onSnapshot={onSnapshot} />);
    fireEvent.click(await screen.findByRole('button', { name: 'More sludge' }));

    const on = pill('More sludge');
    expect(on).toHaveAttribute('aria-pressed', 'true');
    expect(on).toBeDisabled();
    expect(pill('Less doom metal')).toBeDisabled();
    expect(latest.busy).toBe(true);
    expect(apiMock.playlists.setSteer).toHaveBeenCalledWith('p1', [{ kind: 'style', tag: 'sludge', direction: 'more' }]);

    await act(async () => {
      finish(answer({
        active: [{ kind: 'style', tag: 'sludge', direction: 'more' }],
        pills: [deeper, doom, popular], addedTrackIds: ['t1', 't2'],
      }));
    });
    expect(onSnapshot).toHaveBeenCalledWith(snapshot);
    expect(pill('More sludge')).toBeEnabled();
    expect(pill('More sludge')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByText('2 tracks changed')).toBeVisible();
    expect(latest.addedIds.has('t1')).toBe(true);
    expect(pill('Reset')).toBeVisible();
  });

  it('turns a pill off, and resets them all', async () => {
    const on = { kind: 'style' as const, tag: 'sludge', direction: 'more' as const };
    const other = { kind: 'signal' as const, signal: 'new' as const };
    apiMock.playlists.steer.mockResolvedValue(view({ active: [on, other], pills: [doom] }));
    apiMock.playlists.setSteer.mockResolvedValue(answer({ active: [other], pills: [sludge, doom] }));
    render(<Harness />);
    fireEvent.click(await screen.findByRole('button', { name: 'More sludge' }));
    expect(apiMock.playlists.setSteer).toHaveBeenLastCalledWith('p1', [other]);
    await waitFor(() => expect(pill('More sludge')).toHaveAttribute('aria-pressed', 'false'));

    apiMock.playlists.setSteer.mockResolvedValue(answer({ active: [] }));
    fireEvent.click(pill('Reset'));
    expect(apiMock.playlists.setSteer).toHaveBeenLastCalledWith('p1', []);
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Reset' })).toBeNull());
  });

  it('dims the rest at three, with the reason', async () => {
    apiMock.playlists.steer.mockResolvedValue(view({
      active: [
        { kind: 'style', tag: 'a', direction: 'more' },
        { kind: 'style', tag: 'b', direction: 'less' },
        { kind: 'signal', signal: 'new' },
      ],
      pills: [sludge],
    }));
    render(<Harness />);
    const dimmed = await screen.findByRole('button', { name: 'More sludge' });
    expect(dimmed).toBeDisabled();
    expect(dimmed).toHaveAttribute('title', 'Up to 3 at a time');
    expect(pill('More a')).toBeEnabled();
    fireEvent.click(dimmed);
    expect(apiMock.playlists.setSteer).not.toHaveBeenCalled();
  });

  it('turns the pill back off when saving fails, and can try again', async () => {
    apiMock.playlists.setSteer.mockRejectedValueOnce(new Error('nope'));
    render(<Harness />);
    fireEvent.click(await screen.findByRole('button', { name: 'More sludge' }));
    expect(await screen.findByRole('alert')).toHaveTextContent("Couldn't update this playlist. The pill was turned back off.");
    expect(pill('More sludge')).toHaveAttribute('aria-pressed', 'false');

    apiMock.playlists.setSteer.mockResolvedValue(answer({ active: [{ kind: 'style', tag: 'sludge', direction: 'more' }] }));
    fireEvent.click(pill('Try again'));
    await waitFor(() => expect(pill('More sludge')).toHaveAttribute('aria-pressed', 'true'));
    expect(screen.queryByRole('alert')).toBeNull();
    expect(apiMock.playlists.setSteer).toHaveBeenCalledTimes(2);
  });

  it('says when few tracks have style tags', async () => {
    apiMock.playlists.steer.mockResolvedValue(view({ pills: [popular], fewTags: true }));
    render(<Harness />);
    expect(await screen.findByText('More pills appear once BoogieBox finds style tags for these tracks.')).toBeVisible();
    expect(pill('Most played here')).toBeVisible();
  });

  it('shows that message even with no pills', async () => {
    apiMock.playlists.steer.mockResolvedValue(view({ pills: [], fewTags: true }));
    render(<Harness />);
    expect(await screen.findByText(/More pills appear/)).toBeVisible();
    expect(screen.queryByRole('group')).toBeNull();
  });

  it('offers to update the queue only after a tap, only for this playlist, and only until answered', async () => {
    apiMock.playlists.setSteer.mockResolvedValue(answer({ active: [{ kind: 'style', tag: 'sludge', direction: 'more' }] }));
    const onUpdateQueue = vi.fn();
    const { rerender } = render(<Harness queueHere onUpdateQueue={onUpdateQueue} />);
    await screen.findByRole('group');
    expect(screen.queryByText('Your queue still has the old tracks.')).toBeNull();

    fireEvent.click(pill('More sludge'));
    expect(await screen.findByText('Your queue still has the old tracks.')).toBeVisible();

    rerender(<Harness queueHere={false} onUpdateQueue={onUpdateQueue} />);
    expect(screen.queryByText('Your queue still has the old tracks.')).toBeNull();
    rerender(<Harness queueHere onUpdateQueue={onUpdateQueue} />);

    fireEvent.click(pill("Update what's next"));
    expect(onUpdateQueue).toHaveBeenCalledTimes(1);
    expect(screen.queryByText('Your queue still has the old tracks.')).toBeNull();

    fireEvent.click(pill('Reset'));
    await screen.findByText('Your queue still has the old tracks.');
    fireEvent.click(pill('Not now'));
    expect(screen.queryByText('Your queue still has the old tracks.')).toBeNull();
    expect(onUpdateQueue).toHaveBeenCalledTimes(1);
  });

  it('drops an old answer when another playlist opens', async () => {
    let first: (v: SteerView) => void = () => {};
    apiMock.playlists.steer.mockImplementationOnce(() => new Promise(resolve => { first = resolve; }));
    const { rerender } = render(<Harness playlistId="p1" />);
    apiMock.playlists.steer.mockResolvedValue(view({ pills: [popular] }));
    rerender(<Harness playlistId="p2" />);
    await screen.findByRole('button', { name: 'Most played here' });
    await act(async () => { first(view({ pills: [sludge] })); });
    expect(screen.queryByRole('button', { name: 'More sludge' })).toBeNull();
    expect(apiMock.playlists.steer).toHaveBeenLastCalledWith('p2');
  });

  it('reads again when asked, but not during a tap', async () => {
    let finish: (u: SteerUpdate) => void = () => {};
    apiMock.playlists.setSteer.mockImplementation(() => new Promise(resolve => { finish = resolve; }));
    render(<Harness />);
    await screen.findByRole('group');
    expect(apiMock.playlists.steer).toHaveBeenCalledTimes(1);

    act(() => latest.reload());
    await waitFor(() => expect(apiMock.playlists.steer).toHaveBeenCalledTimes(2));

    fireEvent.click(pill('More sludge'));
    act(() => latest.reload());
    expect(apiMock.playlists.steer).toHaveBeenCalledTimes(2);
    await act(async () => { finish(answer({})); });
  });

  it('uses larger touch targets on mobile and scrolls instead of wrapping', async () => {
    render(<Harness mobile />);
    const group = await screen.findByRole('group');
    expect(group).toHaveStyle({ overflowX: 'auto' });
    expect(pill('More sludge')).toHaveStyle({ height: '44px' });
  });

  it('marks added rows', () => {
    render(<SteerAddedDot />);
    expect(screen.getByTestId('steer-added-dot')).toBeInTheDocument();
  });
});
