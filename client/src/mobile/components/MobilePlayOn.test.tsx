/**
 * Play On on mobile: Now Playing tool + sheet, mini player indicator, inline notice and the takeover sheet.
 */

import React from 'react';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { PlaybackSnapshot, PlayerState, PlayerTransportControls } from '../../components/Player';
import { PlayOnContext, type PlayOnApi, type PlayerRemoteView } from '../../remote/usePlayOn';
import type { RendererInfo } from '../../remote/remoteTypes';
import type { Track } from '../../types';
import MobileNowPlayingView from '../views/MobileNowPlayingView';
import MobileMiniPlayer from './MobileMiniPlayer';
import MobileConfirmationSheet from './MobileConfirmationSheet';

vi.mock('../../api', () => ({
  api: {
    albumArtUrl: vi.fn(() => '/cover.jpg'),
    trackLyrics: vi.fn().mockResolvedValue({ lyrics: '', synced: [] }),
    trackSonicFingerprint: vi.fn().mockResolvedValue(null),
  },
}));

const song: Track = {
  id: '1', file_name: 'song.mp3', file_size: 1, format: 'MP3', duration: 120, bitrate: 320, sample_rate: 44100,
  channels: 2, title: 'Out Of Format', artist: 'Agents Of Time', album: 'Album', library_name: 'Main', track_number: 1,
  disc_number: 1, year: 2025, genre: null, composer: null, comment: null, bpm: null, scanned_at: '', album_id: '9',
};
const state: PlayerState = { queue: [song], currentIndex: 0, isPlaying: true, playToken: 1 };
const wiim: RendererInfo = {
  udn: 'uuid:wiim', name: 'WiiM Ultra', friendlyName: 'WiiM Ultra', alias: null, manufacturer: 'Linkplay',
  model: 'WiiM Ultra Receiver', vendor: 'wiim', online: true, hidden: false, formatPolicy: 'auto', hasIcon: false,
  capabilities: { setNext: true, seek: true, volume: true }, lastSeenAt: '', session: null,
};

function playOnApi(over: Partial<PlayOnApi> = {}): PlayOnApi {
  return {
    available: true,
    target: { kind: 'local' },
    renderers: [wiim, { ...wiim, udn: 'uuid:off', name: 'Kitchen', online: false }, { ...wiim, udn: 'uuid:hidden', name: 'Hidden', hidden: true }],
    renderersLoaded: true,
    discovering: false,
    notify: vi.fn(),
    watchRenderers: vi.fn(() => () => {}),
    refreshRenderers: vi.fn(),
    selectTarget: vi.fn(),
    pendingUdn: null,
    deviceName: null,
    remoteView: null,
    remoteActions: { next: vi.fn(), prev: vi.fn(), seek: vi.fn(), setVolume: vi.fn(), setMuted: vi.fn(), setShuffle: vi.fn(), setRepeat: vi.fn() },
    playerState: state,
    dispatch: vi.fn(),
    notice: null,
    takeover: null,
    openSettings: null,
    ...over,
  };
}

function nowPlaying(playOn: PlayOnApi) {
  return render(
    <PlayOnContext.Provider value={playOn}>
      <MobileNowPlayingView snapshot={null} playerState={state} onStateChange={() => {}} />
    </PlayOnContext.Provider>,
  );
}

describe('mobile Play On', () => {
  it('opens the picker sheet from the Now Playing tool and picks a speaker', () => {
    const playOn = playOnApi();
    nowPlaying(playOn);
    fireEvent.click(screen.getByRole('button', { name: 'Play on…' }));
    const sheet = screen.getByRole('dialog', { name: 'Play on' });
    const options = within(sheet).getAllByRole('option');
    expect(options.map((o) => o.textContent)).toEqual([
      expect.stringContaining('This phone'),
      expect.stringContaining('WiiM Ultra'),
      expect.stringContaining('Kitchen'),
    ]);
    expect(options[2]).toBeDisabled();
    expect(within(options[2]).getByText('Offline')).toBeInTheDocument();
    expect(playOn.watchRenderers).toHaveBeenCalled();
    fireEvent.click(within(sheet).getByRole('button', { name: 'Search for speakers' }));
    expect(playOn.refreshRenderers).toHaveBeenCalled();
    fireEvent.click(options[1]);
    expect(playOn.selectTarget).toHaveBeenCalledWith({ kind: 'renderer', udn: 'uuid:wiim' });
    expect(screen.queryByRole('dialog', { name: 'Play on' })).not.toBeInTheDocument();
  });

  it('marks the tool active on a renderer and shows the inline notice with its action', () => {
    const run = vi.fn();
    nowPlaying(playOnApi({
      target: { kind: 'renderer', udn: 'uuid:wiim' },
      deviceName: 'WiiM Ultra',
      notice: { key: 1, text: 'WiiM Ultra is unreachable', tone: 'danger', action: { label: 'Use this browser', run } },
    }));
    expect(screen.getByRole('button', { name: 'Playing on WiiM Ultra' })).toHaveStyle({ color: 'var(--accent)' });
    expect(screen.getByRole('status')).toHaveTextContent('WiiM Ultra is unreachable');
    fireEvent.click(screen.getByRole('button', { name: 'Use this browser' }));
    expect(run).toHaveBeenCalled();
  });

  it('offers the speaker settings link to admins only when nothing is found', () => {
    const empty = playOnApi({ renderers: [] });
    const { unmount } = nowPlaying(empty);
    fireEvent.click(screen.getByRole('button', { name: 'Play on…' }));
    expect(screen.getByText('No speakers found')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Speaker settings' })).not.toBeInTheDocument();
    unmount();
    render(
      <PlayOnContext.Provider value={empty}>
        <MobileNowPlayingView
          currentUser={{ id: 'u1', username: 'admin', role: 'admin', canManageLibraries: true, canEditMetadata: true }}
          snapshot={null}
          playerState={state}
          onStateChange={() => {}}
        />
      </PlayOnContext.Provider>,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Play on…' }));
    expect(screen.getByRole('button', { name: 'Speaker settings' })).toBeInTheDocument();
  });

  it('seeks from the progress bar when the drag ends', () => {
    const controls: PlayerTransportControls = { seek: vi.fn(), setVolume: vi.fn(), toggleMute: vi.fn() };
    const snapshot: PlaybackSnapshot = { currentTrack: song, currentTime: 10, duration: 120, isPlaying: true, volume: 1, muted: false, loading: false, audioError: null };
    render(
      <PlayOnContext.Provider value={playOnApi()}>
        <MobileNowPlayingView snapshot={snapshot} playerState={state} onStateChange={() => {}} transportControls={controls} />
      </PlayOnContext.Provider>,
    );
    const seek = screen.getByRole('slider', { name: 'Seek' });
    fireEvent.change(seek, { target: { value: '60' } });
    expect(controls.seek).not.toHaveBeenCalled();
    expect(screen.getByText('1:00')).toBeInTheDocument();
    fireEvent.pointerUp(seek);
    expect(controls.seek).toHaveBeenCalledWith(60);
    // Local playback: the phone's own buttons set the volume, so no volume row.
    expect(screen.queryByRole('slider', { name: /Volume/ })).not.toBeInTheDocument();
  });

  it('shows the speaker volume on a renderer and explains a fixed one', () => {
    const controls: PlayerTransportControls = { seek: vi.fn(), setVolume: vi.fn(), toggleMute: vi.fn() };
    const snapshot: PlaybackSnapshot = { currentTrack: song, currentTime: 10, duration: 120, isPlaying: true, volume: 0.4, muted: false, loading: false, audioError: null };
    const view: PlayerRemoteView = {
      udn: 'uuid:wiim', deviceName: 'WiiM Ultra', status: 'playing', positionSec: 10, positionAt: 0, durationSec: 120,
      volume: 0.4, muted: false, canSeek: true, canSetVolume: true, shuffle: false, repeat: 'off',
    };
    const remote = playOnApi({ target: { kind: 'renderer', udn: 'uuid:wiim' }, deviceName: 'WiiM Ultra', remoteView: view });
    const { rerender } = render(
      <PlayOnContext.Provider value={remote}>
        <MobileNowPlayingView snapshot={snapshot} playerState={state} onStateChange={() => {}} transportControls={controls} />
      </PlayOnContext.Provider>,
    );
    const volume = screen.getByRole('slider', { name: 'Volume on WiiM Ultra' });
    expect(volume).toHaveValue('40');
    fireEvent.change(volume, { target: { value: '75' } });
    expect(controls.setVolume).toHaveBeenCalledWith(0.75);
    fireEvent.click(screen.getByRole('button', { name: 'Mute' }));
    expect(controls.toggleMute).toHaveBeenCalled();

    rerender(
      <PlayOnContext.Provider value={{ ...remote, remoteView: { ...view, canSetVolume: false } }}>
        <MobileNowPlayingView snapshot={snapshot} playerState={state} onStateChange={() => {}} transportControls={controls} />
      </PlayOnContext.Provider>,
    );
    expect(screen.getByText("WiiM Ultra's volume is fixed")).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Mute' })).toBeDisabled();
    expect(screen.getByRole('slider', { name: 'Seek' })).not.toBeDisabled();
    rerender(
      <PlayOnContext.Provider value={{ ...remote, remoteView: { ...view, canSeek: false } }}>
        <MobileNowPlayingView snapshot={snapshot} playerState={state} onStateChange={() => {}} transportControls={controls} />
      </PlayOnContext.Provider>,
    );
    expect(screen.getByRole('slider', { name: 'Seek' })).toBeDisabled();
  });

  it('hides the tool when Play On is off', () => {
    nowPlaying(playOnApi({ available: false }));
    expect(screen.queryByRole('button', { name: 'Play on…' })).not.toBeInTheDocument();
  });

  it('shows the renderer in the mini player subtitle', () => {
    render(
      <PlayOnContext.Provider value={playOnApi({ target: { kind: 'renderer', udn: 'uuid:wiim' }, deviceName: 'WiiM Ultra' })}>
        <MobileMiniPlayer snapshot={null} playerState={state} onStateChange={() => {}} onOpenNowPlaying={() => {}} />
      </PlayOnContext.Provider>,
    );
    expect(screen.getByText('WiiM Ultra')).toBeInTheDocument();
  });

  it('uses the accent tone and custom cancel label for a takeover confirmation', () => {
    const onConfirm = vi.fn();
    render(
      <MobileConfirmationSheet
        open
        tone="default"
        title="Take over WiiM Ultra?"
        description="Dana is listening on WiiM Ultra."
        itemLabel="Fragments"
        confirmLabel="Take over"
        cancelLabel="Cancel"
        onClose={() => {}}
        onConfirm={onConfirm}
      />,
    );
    const confirm = screen.getByRole('button', { name: 'Take over' });
    expect(confirm).toHaveStyle({ color: 'var(--accent)' });
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Keep it' })).not.toBeInTheDocument();
    fireEvent.click(confirm);
    expect(onConfirm).toHaveBeenCalled();
  });
});
