import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { renderHook } from '@testing-library/react';
import type { Track } from '../types';
import {
  MEDIA_SESSION_SEEK_STEP,
  artworkAlbumId,
  buildMediaMetadata,
  clearMediaSession,
  getMediaSession,
  useMediaSession,
  type MediaSessionActions,
  type MediaSessionState,
} from './mediaSession';

type Handler = ((details: Partial<MediaSessionActionDetails>) => void) | null;

function installMediaSession() {
  const handlers = new Map<string, Handler>();
  const session = {
    metadata: null as unknown,
    playbackState: 'none' as MediaSessionPlaybackState,
    setActionHandler: vi.fn((action: string, handler: Handler) => {
      if (action === 'unsupported') throw new Error('nope');
      handlers.set(action, handler);
    }),
    setPositionState: vi.fn(),
  };
  Object.defineProperty(navigator, 'mediaSession', { value: session, configurable: true });
  class FakeMediaMetadata {
    constructor(init: MediaMetadataInit) { Object.assign(this, init); }
  }
  vi.stubGlobal('MediaMetadata', FakeMediaMetadata);
  return { session, handlers };
}

function track(overrides: Partial<Track> = {}): Track {
  return {
    id: 't1',
    title: 'Song',
    artist: 'Artist',
    album: 'Album',
    album_id: 'a1',
    file_name: 'song.mp3',
    duration: 200,
    ...overrides,
  } as Track;
}

function actions(): MediaSessionActions {
  return { play: vi.fn(), pause: vi.fn(), stop: vi.fn(), previous: vi.fn(), next: vi.fn(), seekTo: vi.fn() };
}

function state(overrides: Partial<MediaSessionState> = {}): MediaSessionState {
  return { track: track(), isPlaying: true, position: 0, duration: 200, canSeek: true, ...overrides };
}

describe('mediaSession', () => {
  let mock: ReturnType<typeof installMediaSession>;

  beforeEach(() => {
    mock = installMediaSession();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    delete (navigator as any).mediaSession;
  });

  it('returns null when MediaSession is unsupported', () => {
    delete (navigator as any).mediaSession;
    expect(getMediaSession()).toBeNull();
    expect(() => clearMediaSession()).not.toThrow();
  });

  it('builds metadata with absolute artwork URLs and the mix album override', () => {
    const meta = buildMediaMetadata(track(), 'BoogieMix');
    expect(meta.title).toBe('Song');
    expect(meta.album).toBe('BoogieMix');
    expect(meta.artwork).toHaveLength(2);
    expect(meta.artwork?.[0].src).toMatch(/^http.*\/api\/albums\/a1\/art\?size=300$/);
    expect(meta.artwork?.[1].sizes).toBe('800x800');
  });

  it('uses the first collage album for mixes and no artwork without an album', () => {
    expect(artworkAlbumId(track({ album_id: null, cover_album_ids: ['c1', 'c2'] }))).toBe('c1');
    const meta = buildMediaMetadata(track({ album_id: null, title: '', file_name: 'x.mp3', artist: null as any, album: null }));
    expect(meta).toMatchObject({ title: 'x.mp3', artist: '', album: '', artwork: [] });
  });

  it('sets metadata, playback state and position for the current track', () => {
    const { rerender } = renderHook((s: MediaSessionState) => useMediaSession(s, actions()), { initialProps: state() });
    expect((mock.session.metadata as any).title).toBe('Song');
    expect(mock.session.playbackState).toBe('playing');
    expect(mock.session.setPositionState).toHaveBeenLastCalledWith({ duration: 200, position: 0, playbackRate: 1 });

    rerender(state({ isPlaying: false, position: 0.2 }));
    expect(mock.session.playbackState).toBe('paused');

    rerender(state({ track: track({ id: 't2', title: 'Next' }) }));
    expect((mock.session.metadata as any).title).toBe('Next');
  });

  it('only forwards position updates that drift from the extrapolated position', () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(0);
      const { rerender } = renderHook((s: MediaSessionState) => useMediaSession(s, actions()), { initialProps: state() });
      const calls = () => mock.session.setPositionState.mock.calls.length;
      const base = calls();
      vi.setSystemTime(1000);
      rerender(state({ position: 1 }));
      expect(calls()).toBe(base);
      rerender(state({ position: 90 }));
      expect(calls()).toBe(base + 1);
    } finally {
      vi.useRealTimers();
    }
  });

  it('routes OS actions to the player and clamps seeks', () => {
    const a = actions();
    renderHook(() => useMediaSession(state({ position: 5 }), a));
    mock.handlers.get('play')?.({});
    mock.handlers.get('pause')?.({});
    mock.handlers.get('stop')?.({});
    mock.handlers.get('previoustrack')?.({});
    mock.handlers.get('nexttrack')?.({});
    mock.handlers.get('seekto')?.({ seekTime: 500 });
    mock.handlers.get('seekto')?.({});
    mock.handlers.get('seekbackward')?.({});
    mock.handlers.get('seekforward')?.({ seekOffset: 30 });
    expect(a.play).toHaveBeenCalledTimes(1);
    expect(a.pause).toHaveBeenCalledTimes(1);
    expect(a.stop).toHaveBeenCalledTimes(1);
    expect(a.previous).toHaveBeenCalledTimes(1);
    expect(a.next).toHaveBeenCalledTimes(1);
    expect(a.seekTo).toHaveBeenNthCalledWith(1, 200);
    expect(a.seekTo).toHaveBeenNthCalledWith(2, 0);
    expect(a.seekTo).toHaveBeenNthCalledWith(3, 35);
    expect(MEDIA_SESSION_SEEK_STEP).toBe(10);
  });

  it('has no seek handlers for radio or unknown durations', () => {
    renderHook(() => useMediaSession(state({ canSeek: false }), actions()));
    expect(mock.handlers.get('seekto')).toBeNull();
    expect(mock.handlers.get('seekforward')).toBeNull();
    expect(typeof mock.handlers.get('nexttrack')).toBe('function');

    renderHook(() => useMediaSession(state({ duration: 0 }), actions()));
    expect(mock.handlers.get('seekbackward')).toBeNull();
  });

  it('clears everything when the queue empties and on unmount', () => {
    const { rerender, unmount } = renderHook((s: MediaSessionState) => useMediaSession(s, actions()), { initialProps: state() });
    rerender(state({ track: null }));
    expect(mock.session.metadata).toBeNull();
    expect(mock.session.playbackState).toBe('none');
    expect(mock.handlers.get('play')).toBeNull();

    rerender(state());
    expect(typeof mock.handlers.get('play')).toBe('function');
    unmount();
    expect(mock.handlers.get('play')).toBeNull();
  });

  it('tolerates browsers that reject actions or position states', () => {
    mock.session.setActionHandler.mockImplementation(() => { throw new Error('unsupported'); });
    mock.session.setPositionState.mockImplementation(() => { throw new Error('bad state'); });
    expect(() => renderHook(() => useMediaSession(state(), actions())).unmount()).not.toThrow();
    expect(() => renderHook(() => useMediaSession(state({ canSeek: false }), actions())).unmount()).not.toThrow();
  });
});
