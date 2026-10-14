/**
 * Player in Play On remote mode: silent audio, controls drive the renderer, picker and notices in the dock.
 */

import React from 'react';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Player, { type PlaybackSnapshot, type PlayerState, type PlayerTransportControls } from '../components/Player';
import { api } from '../api';
import { PlayOnContext, type PlayOnApi, type PlayerRemoteView } from '../remote/usePlayOn';
import type { RendererInfo } from '../remote/remoteTypes';
import type { Track } from '../types';

function track(id: string): Track {
  return {
    id, file_name: `${id}.mp3`, file_size: 1, format: 'mp3', duration: 200, bitrate: null, sample_rate: null,
    channels: null, title: `Song ${id}`, artist: 'Tycho', album: 'Dive', library_name: null, track_number: 1,
    disc_number: 1, year: null, genre: null, composer: null, comment: null, bpm: null, album_id: null, scanned_at: '',
  };
}

const wiim: RendererInfo = {
  udn: 'uuid:wiim', name: 'WiiM Ultra', friendlyName: 'WiiM Ultra', alias: null, manufacturer: 'Linkplay',
  model: 'WiiM Ultra Receiver', vendor: 'wiim', online: true, hidden: false, formatPolicy: 'auto', hasIcon: false,
  capabilities: { setNext: true, seek: true, volume: true }, lastSeenAt: '', session: null,
};
const tv: RendererInfo = {
  ...wiim, udn: 'uuid:tv', name: 'Living room TV', model: 'Bravia',
  session: { status: 'playing', ownerUserId: 'u2', ownerName: 'Dana', nowPlaying: 'Fragments', heldByOther: true },
};

function remoteView(over: Partial<PlayerRemoteView> = {}): PlayerRemoteView {
  return {
    udn: 'uuid:wiim', deviceName: 'WiiM Ultra', status: 'playing', positionSec: 61, positionAt: Date.now(),
    durationSec: 200, volume: 0.4, muted: false, canSeek: true, canSetVolume: true, shuffle: false, repeat: 'off', ...over,
  };
}

function playOnApi(over: Partial<PlayOnApi> = {}): PlayOnApi {
  return {
    available: true,
    target: { kind: 'renderer', udn: 'uuid:wiim' },
    renderers: [wiim, tv],
    renderersLoaded: true,
    discovering: false,
    watchRenderers: vi.fn(() => () => {}),
    refreshRenderers: vi.fn(),
    selectTarget: vi.fn(),
    pendingUdn: null,
    deviceName: 'WiiM Ultra',
    remoteView: remoteView(),
    remoteActions: {
      next: vi.fn(), prev: vi.fn(), seek: vi.fn(), setVolume: vi.fn(), setMuted: vi.fn(), setShuffle: vi.fn(), setRepeat: vi.fn(),
    },
    playerState: { queue: [], currentIndex: 0, isPlaying: false, playToken: 0 },
    dispatch: vi.fn(),
    notice: null,
    takeover: null,
    openSettings: null,
    ...over,
  };
}

const STATE: PlayerState = { queue: [track('1'), track('2')], currentIndex: 0, isPlaying: true, playToken: 0 };

function renderPlayer(playOn: PlayOnApi, extra: Partial<React.ComponentProps<typeof Player>> = {}) {
  const onStateChange = vi.fn();
  const utils = render(
    <PlayOnContext.Provider value={playOn}>
      <Player state={STATE} onStateChange={onStateChange} ffmpegAvailable hybridPreview {...extra} />
    </PlayOnContext.Provider>,
  );
  return { ...utils, onStateChange };
}

describe('Player remote mode', () => {
  let play: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    vi.spyOn(HTMLMediaElement.prototype, 'load').mockImplementation(() => {});
    play = vi.spyOn(HTMLMediaElement.prototype, 'play').mockImplementation(() => Promise.resolve());
    vi.spyOn(HTMLMediaElement.prototype, 'pause').mockImplementation(() => {});
    vi.spyOn(api.userSettings, 'get').mockResolvedValue({});
    vi.spyOn(api.userSettings, 'update').mockResolvedValue({ ok: true });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('keeps the browser silent and synthesizes the snapshot from the renderer', () => {
    const snapshots: PlaybackSnapshot[] = [];
    renderPlayer(playOnApi(), { onPlaybackSnapshotChange: (s) => snapshots.push(s) });
    expect(play).not.toHaveBeenCalled();
    const last = snapshots[snapshots.length - 1];
    expect(last.currentTime).toBeGreaterThanOrEqual(61);
    expect(last).toMatchObject({ isPlaying: true, volume: 0.4, muted: false, duration: 200 });
    expect(document.querySelector('audio')?.getAttribute('src')).toBeNull();
  });

  it('routes transport, shuffle and repeat to the renderer', () => {
    const playOn = playOnApi();
    const { onStateChange } = renderPlayer(playOn);
    fireEvent.click(screen.getByRole('button', { name: 'Previous track' }));
    fireEvent.click(screen.getByRole('button', { name: 'Next track' }));
    expect(playOn.remoteActions.prev).toHaveBeenCalled();
    expect(playOn.remoteActions.next).toHaveBeenCalled();
    fireEvent.click(screen.getByTitle('Shuffle queue'));
    expect(playOn.remoteActions.setShuffle).toHaveBeenCalledWith(true);
    fireEvent.click(screen.getByRole('button', { name: 'Repeat off' }));
    expect(playOn.remoteActions.setRepeat).toHaveBeenCalledWith('one');
    // Play/pause goes through the PlayerState bridge.
    fireEvent.click(screen.getByRole('button', { name: 'Pause' }));
    expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ isPlaying: false }));
  });

  it('exposes seek and volume for other surfaces, routed to the renderer', () => {
    const playOn = playOnApi();
    let controls: PlayerTransportControls | null = null;
    renderPlayer(playOn, { onTransportControlsChange: (c) => { controls = c; } });
    expect(controls).not.toBeNull();
    controls!.seek(42);
    expect(playOn.remoteActions.seek).toHaveBeenCalledWith(42);
    controls!.setVolume(0.6);
    expect(playOn.remoteActions.setVolume).toHaveBeenCalledWith(0.6);
    controls!.toggleMute();
    expect(playOn.remoteActions.setMuted).toHaveBeenCalledWith(true);
  });

  it('disables EQ and a fixed volume with explanations', () => {
    renderPlayer(playOnApi({ remoteView: remoteView({ canSetVolume: false }) }));
    const eq = screen.getByRole('button', { name: 'Equalizer' });
    expect(eq).toBeDisabled();
    expect(eq).toHaveAttribute('title', 'EQ applies to this browser only');
    const mute = screen.getByRole('button', { name: 'Mute' });
    expect(mute).toBeDisabled();
    expect(mute).toHaveAttribute('title', "WiiM Ultra's volume is fixed");
  });

  it('shows where it plays and opens the picker from the indicator and the dock button', () => {
    const playOn = playOnApi();
    renderPlayer(playOn);
    const dockButton = screen.getByRole('button', { name: 'Playing on WiiM Ultra' });
    const queueButton = screen.getByRole('button', { name: 'Playback queue' });
    expect(dockButton.nextElementSibling).toBe(queueButton);

    fireEvent.click(screen.getByRole('button', { name: 'Playing on WiiM Ultra. Change speaker' }));
    const dialog = screen.getByRole('dialog', { name: 'Play on' });
    expect(within(dialog).getByText('· 3')).toBeInTheDocument();
    const options = within(dialog).getAllByRole('option');
    expect(options.map((o) => o.textContent)).toEqual([
      expect.stringContaining('This browser'),
      expect.stringContaining('WiiM Ultra'),
      expect.stringContaining('In use by Dana · Fragments'),
    ]);
    expect(options[1]).toHaveAttribute('aria-selected', 'true');
    expect(within(options[2]).getByText('In use')).toBeInTheDocument();
    expect(playOn.watchRenderers).toHaveBeenCalled();

    fireEvent.click(options[0]);
    expect(playOn.selectTarget).toHaveBeenCalledWith({ kind: 'local' });
    expect(screen.queryByRole('dialog', { name: 'Play on' })).not.toBeInTheDocument();
  });

  it('renders inline notices with their action in the progress slot', () => {
    const run = vi.fn();
    renderPlayer(playOnApi({ notice: { key: 1, text: 'WiiM Ultra is playing another source', tone: 'muted', action: { label: 'Resume here', run } } }));
    expect(screen.getByRole('status')).toHaveTextContent('WiiM Ultra is playing another source');
    fireEvent.click(screen.getByRole('button', { name: 'Resume here' }));
    expect(run).toHaveBeenCalled();
  });

  it('offers Play On while local, with an empty state for no speakers', () => {
    const playOn = playOnApi({ target: { kind: 'local' }, remoteView: null, deviceName: null, renderers: [] });
    renderPlayer(playOn);
    expect(play).toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Play on…' }));
    const dialog = screen.getByRole('dialog', { name: 'Play on' });
    expect(within(dialog).getByText('No speakers found')).toBeInTheDocument();
    expect(within(dialog).getByRole('option', { name: /This browser/ })).toHaveAttribute('aria-selected', 'true');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Search for speakers' }));
    expect(playOn.refreshRenderers).toHaveBeenCalled();
  });

  it('links admins from the empty picker to the Play On settings', () => {
    const openSettings = vi.fn();
    renderPlayer(playOnApi({ target: { kind: 'local' }, remoteView: null, deviceName: null, renderers: [], openSettings }));
    fireEvent.click(screen.getByRole('button', { name: 'Play on…' }));
    fireEvent.click(screen.getByRole('button', { name: 'Speaker settings' }));
    expect(openSettings).toHaveBeenCalled();
    expect(screen.queryByRole('dialog', { name: 'Play on' })).not.toBeInTheDocument();
  });

  it('hides the Play On button when the server has it switched off', () => {
    renderPlayer(playOnApi({ available: false, target: { kind: 'local' }, remoteView: null, deviceName: null }));
    expect(screen.queryByRole('button', { name: 'Play on…' })).not.toBeInTheDocument();
  });
});
