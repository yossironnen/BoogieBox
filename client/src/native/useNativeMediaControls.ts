/**
 * Windows' media controls while "This PC" is the output (plan §11): tells the desktop app what plays (title,
 * artist, album, artwork, length) and runs the buttons pressed in Windows (media keys, the flyout, the lock screen,
 * the taskbar thumbnail) through the Player's own actions. The page's Media Session stays off meanwhile.
 */

import { useEffect, useRef } from 'react';
import { api } from '../api';
import { platform } from '../platform';
import { artworkAlbumId } from '../platform/mediaSession';
import type { Track } from '../types';
import type { NativeMediaAction, NativeNowPlaying } from './nativeTypes';

export interface NativeMediaActions {
  play: () => void;
  pause: () => void;
  toggle: () => void;
  stop: () => void;
  previous: () => void;
  next: () => void;
  seekTo: (seconds: number) => void;
}

export interface NativeMediaOptions {
  /** "This PC" is the output. */
  enabled: boolean;
  track: Track | null;
  /** Internet radio (no timeline, no seeking). */
  live: boolean;
  /** Album name shown for mixes. */
  albumOverride?: string | null;
}

function absolute(url: string): string {
  try {
    return new URL(url, window.location.href).href;
  } catch {
    return url;
  }
}

/** What Windows shows for `track`. */
export function nowPlayingFor(track: Track, live: boolean, albumOverride?: string | null): NativeNowPlaying {
  const albumId = artworkAlbumId(track);
  return {
    title: track.title || track.file_name || '',
    artist: track.artist ?? null,
    album: albumOverride ?? track.album ?? null,
    artUrl: albumId != null ? absolute(api.albumArtUrl(albumId, 300)) : null,
    durationSec: live ? null : (track.duration ?? null),
    live,
  };
}

/** Runs one Windows button press. */
export function runMediaAction(action: NativeMediaAction, actions: NativeMediaActions, canSeek: boolean): void {
  switch (action.action) {
    case 'play': actions.play(); return;
    case 'pause': actions.pause(); return;
    case 'toggle': actions.toggle(); return;
    case 'stop': actions.stop(); return;
    case 'next': actions.next(); return;
    case 'previous': actions.previous(); return;
    case 'seek':
      if (canSeek && typeof action.sec === 'number' && Number.isFinite(action.sec)) actions.seekTo(Math.max(0, action.sec));
      return;
    default:
  }
}

export function useNativeMediaControls(options: NativeMediaOptions, actions: NativeMediaActions): void {
  const { enabled, track, live, albumOverride } = options;
  const actionsRef = useRef(actions);
  actionsRef.current = actions;
  const liveRef = useRef(live);
  liveRef.current = live;
  const shownRef = useRef(false);

  const key = enabled && track
    ? `${track.id}|${track.title}|${track.artist}|${artworkAlbumId(track)}|${albumOverride ?? ''}|${track.duration ?? ''}|${live}`
    : '';
  useEffect(() => {
    const audio = platform.audio;
    if (!audio) return;
    if (key && track) {
      shownRef.current = true;
      audio.nowPlaying(nowPlayingFor(track, live, albumOverride)).catch(() => {});
    } else if (shownRef.current) {
      shownRef.current = false;
      audio.clearNowPlaying().catch(() => {});
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  // Leaving the page (or the Player unmounting): Windows' controls go away too.
  useEffect(() => () => {
    if (shownRef.current) platform.audio?.clearNowPlaying().catch(() => {});
  }, []);

  useEffect(() => {
    const audio = platform.audio;
    if (!enabled || !audio) return undefined;
    let off: (() => void) | null = null;
    let cancelled = false;
    audio.onMediaControl((action) => runMediaAction(action, actionsRef.current, !liveRef.current)).then((unsubscribe) => {
      if (cancelled) unsubscribe();
      else off = unsubscribe;
    }).catch(() => {});
    return () => {
      cancelled = true;
      off?.();
    };
  }, [enabled]);
}
