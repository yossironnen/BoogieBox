/**
 * Tests the Equalizer's saved EQ block: the switch, the status line, save and remove, and failures.
 */

import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import SavedEqPanel from './SavedEqPanel';
import type { PlayerEqControls, SavedEqStatus } from './Player';

function controls(over: Partial<PlayerEqControls> = {}, status: Partial<SavedEqStatus> = {}) {
  return {
    savedEqEnabled: false,
    savedEqScope: null,
    savedEq: { hasTrack: false, hasAlbum: false, edited: false, canSave: true, canSaveAlbum: true, browserOnly: false, ...status },
    onSavedEqEnabledChange: vi.fn(),
    onSaveSavedEq: vi.fn(async () => true),
    onRemoveSavedEq: vi.fn(async () => true),
    watchSavedEq: vi.fn(() => () => {}),
    ...over,
  } as Pick<PlayerEqControls,
    'savedEqEnabled' | 'savedEqScope' | 'savedEq' | 'onSavedEqEnabledChange' | 'onSaveSavedEq' | 'onRemoveSavedEq' | 'watchSavedEq'>;
}

describe('SavedEqPanel', () => {
  it('watches the song while shown and turns the switch on and off', () => {
    const stop = vi.fn();
    const c = controls({ watchSavedEq: vi.fn(() => stop) });
    const { unmount } = render(<SavedEqPanel controls={c} />);
    expect(c.watchSavedEq).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole('checkbox', { name: 'Use my saved EQ' }));
    expect(c.onSavedEqEnabledChange).toHaveBeenCalledWith(true);
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Remove from this song' })).not.toBeInTheDocument();
    unmount();
    expect(stop).toHaveBeenCalled();
  });

  it('says whose curve plays, or that an edit isn\'t saved yet', () => {
    const { rerender } = render(<SavedEqPanel controls={controls({ savedEqEnabled: true, savedEqScope: 'track' }, { hasTrack: true })} />);
    expect(screen.getByRole('status')).toHaveTextContent('Using the EQ saved for this song');
    expect(screen.getByRole('button', { name: 'Remove from this song' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Remove from this album' })).not.toBeInTheDocument();

    rerender(<SavedEqPanel controls={controls({ savedEqEnabled: true, savedEqScope: 'album' }, { hasAlbum: true })} />);
    expect(screen.getByRole('status')).toHaveTextContent('Using the EQ saved for this album');
    expect(screen.getByRole('button', { name: 'Remove from this album' })).toBeInTheDocument();

    rerender(<SavedEqPanel controls={controls({ savedEqEnabled: true, savedEqScope: 'album' }, { hasAlbum: true, edited: true })} />);
    expect(screen.getByRole('status')).toHaveTextContent('Not saved yet');
  });

  it('saves and removes, and says when it couldn\'t', async () => {
    const c = controls({}, { hasTrack: true, hasAlbum: true });
    vi.mocked(c.onSaveSavedEq).mockResolvedValueOnce(false);
    render(<SavedEqPanel controls={c} />);

    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Save for this song' })); });
    expect(c.onSaveSavedEq).toHaveBeenCalledWith('track');
    expect(screen.getByRole('alert')).toHaveTextContent("Couldn't save this EQ. Try again.");

    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Save for this album' })); });
    expect(c.onSaveSavedEq).toHaveBeenCalledWith('album');
    await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument());

    vi.mocked(c.onRemoveSavedEq).mockRejectedValueOnce(new Error('offline'));
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Remove from this song' })); });
    expect(c.onRemoveSavedEq).toHaveBeenCalledWith('track');
    expect(screen.getByRole('alert')).toHaveTextContent("Couldn't remove this EQ. Try again.");
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Remove from this album' })); });
    expect(c.onRemoveSavedEq).toHaveBeenCalledWith('album');
  });

  it('disables saving where it can\'t apply, and explains Play On', () => {
    const { rerender } = render(<SavedEqPanel controls={controls({}, { canSave: false, canSaveAlbum: false })} />);
    expect(screen.getByRole('button', { name: 'Save for this song' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Save for this album' })).toBeDisabled();

    rerender(<SavedEqPanel controls={controls({ savedEqEnabled: true, savedEqScope: 'track' }, { browserOnly: true })} />);
    expect(screen.getByText('EQ applies to this browser only')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Save for this song' })).not.toBeInTheDocument();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  it('uses 44 px targets on a phone', () => {
    render(<SavedEqPanel controls={controls({}, { hasTrack: true })} mobile />);
    expect(screen.getByRole('button', { name: 'Save for this song' })).toHaveStyle({ minHeight: '44px' });
    expect(screen.getByRole('button', { name: 'Save for this album' })).toHaveStyle({ minHeight: '44px' });
    expect(screen.getByRole('button', { name: 'Remove from this song' })).toHaveStyle({ minHeight: '44px' });
  });
});
