import { act, renderHook, waitFor } from '@testing-library/react';
import { useCallback, useState, type SetStateAction } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, RemoteApiError } from '../api';
import type { PlayerState } from '../components/Player';
import type { Track } from '../types';
import type { RemoteEvent, RemoteSessionState, RendererInfo } from './remoteTypes';
import { PLAYBACK_TARGET_STORAGE_KEY } from './usePlaybackTarget';
import { usePlayOn } from './usePlayOn';

const events = vi.hoisted(() => ({ listeners: [] as Array<(event: RemoteEvent) => void> }));
vi.mock('./remoteEvents', () => ({
  subscribeRemoteEvents: (listener: (event: RemoteEvent) => void) => {
    events.listeners.push(listener);
    return () => { events.listeners.splice(events.listeners.indexOf(listener), 1); };
  },
}));

function emit(event: RemoteEvent) {
  act(() => { for (const listener of [...events.listeners]) listener(event); });
}

function track(id: string): Track {
  return {
    id, file_name: `${id}.flac`, file_size: 1, format: 'flac', duration: 200, bitrate: null, sample_rate: null,
    channels: null, title: `Title ${id}`, artist: 'Artist', album: 'Album', library_name: null, track_number: 1,
    disc_number: 1, year: null, genre: null, composer: null, comment: null, bpm: null, album_id: 'al1', scanned_at: '',
  };
}

const wiim: RendererInfo = {
  udn: 'uuid:wiim', name: 'WiiM Ultra', friendlyName: 'WiiM Ultra', alias: null, manufacturer: 'Linkplay',
  model: 'WiiM Ultra Receiver', vendor: 'wiim', online: true, hidden: false, formatPolicy: 'auto', hasIcon: false,
  capabilities: { setNext: true, seek: true, volume: true }, lastSeenAt: '', session: null,
};

function session(over: Partial<RemoteSessionState> = {}): RemoteSessionState {
  return {
    udn: 'uuid:wiim', rendererName: 'WiiM Ultra', status: 'playing',
    queue: [
      { qid: 1, kind: 'track', id: 'a', title: 'Title a', artist: 'Artist', album: 'Album', albumId: 'al1', duration: 200 },
      { qid: 2, kind: 'track', id: 'b', title: 'Title b', artist: 'Artist', album: 'Album', albumId: 'al1', duration: 200 },
    ],
    currentQid: 1, positionSec: 30, durationSec: 200, sampledAtMs: 1, volume: 40, muted: false, shuffle: false,
    repeat: 'off', queueSource: null, ownerUserId: 'u1', ownerName: 'Ann', lastControllerUserId: 'u1',
    canSeek: true, canSetVolume: true, revision: 10, error: null, ...over,
  };
}

const EMPTY: PlayerState = { queue: [], currentIndex: 0, isPlaying: false, playToken: 0 };
let localOptions: Array<{ alreadyRecorded?: boolean } | undefined> = [];

function setup(initial: PlayerState = EMPTY) {
  return renderHook(() => {
    const [local, setLocal] = useState(initial);
    const setLocalState = useCallback((action: SetStateAction<PlayerState>, options?: { alreadyRecorded?: boolean }) => {
      localOptions.push(options);
      setLocal(action);
    }, []);
    const playOn = usePlayOn({ enabled: true, currentUserId: 'u1', localState: local, setLocalState, getLocalPosition: () => 42 });
    return { playOn, local };
  });
}

describe('usePlayOn', () => {
  beforeEach(() => {
    events.listeners = [];
    localOptions = [];
    localStorage.clear();
    vi.spyOn(api.remote, 'renderers').mockResolvedValue({ running: true, renderers: [wiim] });
    vi.spyOn(api.remote, 'session').mockResolvedValue(session());
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('keeps local playback local and opens no event stream', async () => {
    const { result } = setup();
    await waitFor(() => expect(result.current.playOn.renderersLoaded).toBe(true));
    act(() => result.current.playOn.dispatch({ ...EMPTY, queue: [track('x')], isPlaying: true, playToken: 1 }));
    expect(result.current.local.queue.map((t) => t.id)).toEqual(['x']);
    expect(result.current.playOn.playerState).toBe(result.current.local);
    expect(events.listeners).toHaveLength(0);
  });

  it('mirrors the stored renderer session and turns a queue click into a jump', async () => {
    localStorage.setItem(PLAYBACK_TARGET_STORAGE_KEY, JSON.stringify({ kind: 'renderer', udn: 'uuid:wiim' }));
    const jump = vi.spyOn(api.remote, 'command').mockResolvedValue(session({ currentQid: 2, revision: 11 }));
    const { result } = setup();
    await waitFor(() => expect(result.current.playOn.playerState.queue).toHaveLength(2));
    expect(result.current.playOn.playerState).toMatchObject({ currentIndex: 0, isPlaying: true });
    expect(result.current.playOn.deviceName).toBe('WiiM Ultra');
    expect(events.listeners).toHaveLength(1);

    act(() => result.current.playOn.dispatch((prev) => ({ ...prev, currentIndex: 1, isPlaying: true, playToken: prev.playToken + 1 })));
    await waitFor(() => expect(result.current.playOn.playerState.currentIndex).toBe(1));
    expect(jump).toHaveBeenCalledWith('uuid:wiim', { cmd: 'jump', value: 2 }, false);
  });

  it('prompts before taking over another user and retries with takeover', async () => {
    localStorage.setItem(PLAYBACK_TARGET_STORAGE_KEY, JSON.stringify({ kind: 'renderer', udn: 'uuid:wiim' }));
    const inUse = { code: 'session_in_use', udn: 'uuid:wiim', deviceName: 'WiiM Ultra', ownerUserId: 'u2', ownerName: 'Dana', status: 'playing', nowPlaying: { title: 'Fragments' } };
    const command = vi.spyOn(api.remote, 'command')
      .mockRejectedValueOnce(new RemoteApiError('in use', 409, 'session_in_use', inUse))
      .mockResolvedValueOnce(session({ status: 'paused', revision: 12 }));
    const { result } = setup();
    await waitFor(() => expect(result.current.playOn.playerState.queue).toHaveLength(2));

    act(() => result.current.playOn.dispatch((prev) => ({ ...prev, isPlaying: false })));
    await waitFor(() => expect(result.current.playOn.takeover).not.toBeNull());
    expect(result.current.playOn.takeover?.info.ownerName).toBe('Dana');
    expect(result.current.playOn.playerState.isPlaying).toBe(true);

    await act(async () => { await result.current.playOn.takeover?.confirm(); });
    expect(result.current.playOn.takeover).toBeNull();
    expect(command).toHaveBeenLastCalledWith('uuid:wiim', { cmd: 'pause' }, true);
    expect(result.current.playOn.playerState.isPlaying).toBe(false);
  });

  it('cancelling the takeover prompt sends nothing', async () => {
    localStorage.setItem(PLAYBACK_TARGET_STORAGE_KEY, JSON.stringify({ kind: 'renderer', udn: 'uuid:wiim' }));
    const inUse = { code: 'session_in_use', udn: 'uuid:wiim', deviceName: 'WiiM Ultra', ownerUserId: 'u2', ownerName: 'Dana', status: 'playing', nowPlaying: null };
    const command = vi.spyOn(api.remote, 'command').mockRejectedValueOnce(new RemoteApiError('in use', 409, 'session_in_use', inUse));
    const { result } = setup();
    await waitFor(() => expect(result.current.playOn.playerState.queue).toHaveLength(2));
    act(() => result.current.playOn.dispatch((prev) => ({ ...prev, isPlaying: false })));
    await waitFor(() => expect(result.current.playOn.takeover).not.toBeNull());
    act(() => result.current.playOn.takeover?.cancel());
    expect(result.current.playOn.takeover).toBeNull();
    expect(command).toHaveBeenCalledTimes(1);
  });

  it('hands the local queue to a renderer at the current position', async () => {
    const load = vi.spyOn(api.remote, 'load').mockResolvedValue(session({ revision: 20 }));
    const { result } = setup({ queue: [track('a'), track('b')], currentIndex: 1, isPlaying: true, playToken: 3, queueSource: { type: 'album', id: 'al1' } });
    await waitFor(() => expect(result.current.playOn.renderersLoaded).toBe(true));

    await act(async () => { result.current.playOn.selectTarget({ kind: 'renderer', udn: 'uuid:wiim' }); });
    await waitFor(() => expect(result.current.playOn.target).toEqual({ kind: 'renderer', udn: 'uuid:wiim' }));
    expect(load).toHaveBeenCalledWith('uuid:wiim', {
      items: [{ kind: 'track', id: 'a' }, { kind: 'track', id: 'b' }],
      startIndex: 1,
      startAtSec: 42,
      queueSource: { type: 'album', id: 'al1' },
      resumed: true,
      takeover: false,
    });
    expect(result.current.local.isPlaying).toBe(false);
    expect(result.current.playOn.playerState.queue[0]).toMatchObject({ id: 'a', remoteQid: 1, file_name: 'a.flac' });
    expect(JSON.parse(localStorage.getItem(PLAYBACK_TARGET_STORAGE_KEY) ?? '{}')).toEqual({ kind: 'renderer', udn: 'uuid:wiim' });
  });

  it('brings the session back to this browser and releases the renderer', async () => {
    localStorage.setItem(PLAYBACK_TARGET_STORAGE_KEY, JSON.stringify({ kind: 'renderer', udn: 'uuid:wiim' }));
    const release = vi.spyOn(api.remote, 'release').mockResolvedValue(session({ status: 'stopped' }));
    const { result } = setup({ ...EMPTY, playToken: 5 });
    await waitFor(() => expect(result.current.playOn.playerState.queue).toHaveLength(2));

    act(() => result.current.playOn.selectTarget({ kind: 'local' }));
    expect(result.current.playOn.target).toEqual({ kind: 'local' });
    expect(release).toHaveBeenCalledWith('uuid:wiim');
    expect(result.current.local).toMatchObject({ currentIndex: 0, isPlaying: true, playToken: 6 });
    expect(result.current.local.resumeAt).toMatchObject({ playToken: 6, trackId: 'a' });
    expect(result.current.local.queue[0].remoteQid).toBeUndefined();
    expect(localOptions[localOptions.length - 1]).toEqual({ alreadyRecorded: true });
  });

  it('falls back to this browser, paused, when another user takes over, and can copy the queue', async () => {
    localStorage.setItem(PLAYBACK_TARGET_STORAGE_KEY, JSON.stringify({ kind: 'renderer', udn: 'uuid:wiim' }));
    const { result } = setup({ queue: [track('z')], currentIndex: 0, isPlaying: true, playToken: 2 });
    await waitFor(() => expect(result.current.playOn.playerState.queue).toHaveLength(2));

    emit({ type: 'takeover', udn: 'uuid:wiim', fromUserId: 'u1', byUserId: 'u2', byUserName: 'Dana' });
    expect(result.current.playOn.target).toEqual({ kind: 'local' });
    expect(result.current.local).toMatchObject({ isPlaying: false, queue: [expect.objectContaining({ id: 'z' })] });
    expect(result.current.playOn.notice?.text).toBe('Dana took over WiiM Ultra');
    expect(result.current.playOn.notice?.action?.label).toBe('Copy queue here');

    act(() => result.current.playOn.notice?.action?.run());
    expect(result.current.local.queue.map((t) => t.id)).toEqual(['a', 'b']);
    expect(result.current.local.isPlaying).toBe(false);
    expect(result.current.playOn.notice).toBeNull();
  });

  it('keeps the queue here, paused, when taken over with nothing queued locally', async () => {
    localStorage.setItem(PLAYBACK_TARGET_STORAGE_KEY, JSON.stringify({ kind: 'renderer', udn: 'uuid:wiim' }));
    const { result } = setup();
    await waitFor(() => expect(result.current.playOn.playerState.queue).toHaveLength(2));

    emit({ type: 'position', udn: 'uuid:wiim', status: 'playing', positionSec: 41, durationSec: 200, sampledAtMs: 9 });
    emit({ type: 'takeover', udn: 'uuid:wiim', fromUserId: 'u1', byUserId: 'u2', byUserName: 'Dana' });
    expect(result.current.playOn.target).toEqual({ kind: 'local' });
    expect(result.current.local.queue.map((t) => t.id)).toEqual(['a', 'b']);
    expect(result.current.local).toMatchObject({ isPlaying: false, resumeAt: { trackId: 'a', sec: 41 } });
    expect(result.current.playOn.notice).toMatchObject({ text: 'Dana took over WiiM Ultra' });
    expect(result.current.playOn.notice?.action).toBeUndefined();
  });

  it('plays here when the stored renderer is offline at startup', async () => {
    localStorage.setItem(PLAYBACK_TARGET_STORAGE_KEY, JSON.stringify({ kind: 'renderer', udn: 'uuid:wiim' }));
    vi.mocked(api.remote.renderers).mockResolvedValue({ running: true, renderers: [{ ...wiim, online: false }] });
    const { result } = setup();
    await waitFor(() => expect(result.current.playOn.target).toEqual({ kind: 'local' }));
    expect(result.current.playOn.notice?.text).toBe('WiiM Ultra is offline, playing here');
  });

  it('shows server notices and a resume action when another source took the speaker', async () => {
    localStorage.setItem(PLAYBACK_TARGET_STORAGE_KEY, JSON.stringify({ kind: 'renderer', udn: 'uuid:wiim' }));
    vi.mocked(api.remote.session).mockResolvedValue(session({ status: 'lostControl' }));
    const command = vi.spyOn(api.remote, 'command').mockResolvedValue(session({ revision: 30 }));
    const { result } = setup();
    await waitFor(() => expect(result.current.playOn.notice?.text).toBe('WiiM Ultra is playing another source'));
    act(() => result.current.playOn.notice?.action?.run());
    await waitFor(() => expect(command).toHaveBeenCalledWith('uuid:wiim', { cmd: 'play' }, false));

    emit({ type: 'notice', udn: 'uuid:wiim', level: 'warning', message: 'Skipped Teardrop: WiiM Ultra can’t play this format' });
    expect(result.current.playOn.notice).toMatchObject({ tone: 'warning', text: 'Skipped Teardrop: WiiM Ultra can’t play this format' });
  });

  it('follows position ticks and hides Play On when the server has it switched off', async () => {
    localStorage.setItem(PLAYBACK_TARGET_STORAGE_KEY, JSON.stringify({ kind: 'renderer', udn: 'uuid:wiim' }));
    const { result } = setup();
    await waitFor(() => expect(result.current.playOn.remoteView).not.toBeNull());
    emit({ type: 'position', udn: 'uuid:wiim', status: 'paused', positionSec: 77, durationSec: 200, sampledAtMs: 5 });
    expect(result.current.playOn.remoteView).toMatchObject({ positionSec: 77, status: 'paused', volume: 0.4 });
    expect(result.current.playOn.playerState.isPlaying).toBe(false);

    vi.mocked(api.remote.renderers).mockRejectedValue(new RemoteApiError('Play On is disabled', 404, null, null));
    emit({ type: 'connected' });
    await waitFor(() => expect(result.current.playOn.available).toBe(false));
    expect(result.current.playOn.target).toEqual({ kind: 'local' });
  });
});
