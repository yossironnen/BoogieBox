/**
 * OS media controls (lock screen, notification, media keys, Windows SMTC) through the
 * MediaSession API. A thin adapter plus one hook the Player calls; every action routes to the
 * same handlers the dock buttons use, so Play On targets work without special cases.
 */

import { useEffect, useRef } from 'react';
import { api } from '../api';
import type { Track } from '../types';

/** Seconds skipped by the OS "seek backward/forward" buttons. */
export const MEDIA_SESSION_SEEK_STEP = 10;

/** Transport callbacks the OS controls invoke. */
export interface MediaSessionActions {
  play: () => void;
  pause: () => void;
  stop: () => void;
  previous: () => void;
  next: () => void;
  seekTo: (seconds: number) => void;
}

/** What the OS controls show. */
export interface MediaSessionState {
  track: Track | null;
  isPlaying: boolean;
  position: number;
  duration: number;
  /** False for radio, anything without a known duration, and targets that cannot seek. */
  canSeek: boolean;
  /** Album name shown for mixes instead of the track's own album. */
  albumOverride?: string | null;
}

type ActionName = 'play' | 'pause' | 'stop' | 'previoustrack' | 'nexttrack' | 'seekto' | 'seekbackward' | 'seekforward';
const ALL_ACTIONS: ActionName[] = ['play', 'pause', 'stop', 'previoustrack', 'nexttrack', 'seekto', 'seekbackward', 'seekforward'];
const SEEK_ACTIONS: ActionName[] = ['seekto', 'seekbackward', 'seekforward'];

/** The browser's MediaSession, or null when unsupported (old WebViews, tests without a mock). */
export function getMediaSession(): MediaSession | null {
  if (typeof navigator === 'undefined') return null;
  return (navigator as Navigator & { mediaSession?: MediaSession }).mediaSession ?? null;
}

function absoluteUrl(url: string): string {
  try {
    return new URL(url, window.location.href).href;
  } catch {
    return url;
  }
}

/** Album id whose art represents `track`: its own album, or the first album of a mix collage. */
export function artworkAlbumId(track: Track): Track['album_id'] {
  return track.album_id ?? track.cover_album_ids?.[0] ?? null;
}

/** MediaMetadata init for `track` (artwork as absolute URLs; the art endpoint needs no session). */
export function buildMediaMetadata(track: Track, albumOverride?: string | null): MediaMetadataInit {
  const albumId = artworkAlbumId(track);
  return {
    title: track.title || track.file_name || '',
    artist: track.artist ?? '',
    album: albumOverride ?? track.album ?? '',
    artwork: albumId
      ? ([300, 800] as const).map((size) => ({
        src: absoluteUrl(api.albumArtUrl(albumId, size)),
        sizes: `${size}x${size}`,
      }))
      : [],
  };
}

function setHandler(session: MediaSession, action: ActionName, handler: MediaSessionActionHandler | null) {
  try {
    session.setActionHandler(action, handler);
  } catch {
    // Browsers throw for actions they do not support (e.g. "stop" on older Safari).
  }
}

/** Removes metadata, position and every handler. */
export function clearMediaSession(session: MediaSession | null = getMediaSession()) {
  if (!session) return;
  session.metadata = null;
  session.playbackState = 'none';
  try {
    session.setPositionState?.();
  } catch {
    // Older implementations reject an empty call; nothing to clear there.
  }
  for (const action of ALL_ACTIONS) setHandler(session, action, null);
}

function setPosition(session: MediaSession, duration: number, position: number) {
  try {
    session.setPositionState?.({
      duration,
      position: Math.max(0, Math.min(position, duration)),
      playbackRate: 1,
    });
  } catch {
    // Invalid states (e.g. duration still 0 while loading) are skipped.
  }
}

/** Keeps the OS media controls in step with the player. Call once, from the Player. */
export function useMediaSession(state: MediaSessionState, actions: MediaSessionActions) {
  const actionsRef = useRef(actions);
  actionsRef.current = actions;
  const positionRef = useRef(state.position);
  positionRef.current = state.position;
  const durationRef = useRef(state.duration);
  durationRef.current = state.duration;

  const { track, isPlaying, position, duration, canSeek, albumOverride } = state;
  const hasTrack = track != null;
  const seekable = hasTrack && canSeek && duration > 0;

  // Handlers: bound while there is a track; seek handlers only when seeking makes sense.
  useEffect(() => {
    const session = getMediaSession();
    if (!session || !hasTrack) return undefined;
    const call = (fn: () => void) => () => fn();
    setHandler(session, 'play', call(() => actionsRef.current.play()));
    setHandler(session, 'pause', call(() => actionsRef.current.pause()));
    setHandler(session, 'stop', call(() => actionsRef.current.stop()));
    setHandler(session, 'previoustrack', call(() => actionsRef.current.previous()));
    setHandler(session, 'nexttrack', call(() => actionsRef.current.next()));
    if (seekable) {
      const clamp = (t: number) => Math.max(0, Math.min(t, durationRef.current));
      setHandler(session, 'seekto', (details) => {
        if (typeof details.seekTime === 'number') actionsRef.current.seekTo(clamp(details.seekTime));
      });
      setHandler(session, 'seekbackward', (details) => {
        actionsRef.current.seekTo(clamp(positionRef.current - (details.seekOffset ?? MEDIA_SESSION_SEEK_STEP)));
      });
      setHandler(session, 'seekforward', (details) => {
        actionsRef.current.seekTo(clamp(positionRef.current + (details.seekOffset ?? MEDIA_SESSION_SEEK_STEP)));
      });
    } else {
      for (const action of SEEK_ACTIONS) setHandler(session, action, null);
    }
    return undefined;
  }, [hasTrack, seekable]);

  // Clear everything when the queue empties or the Player unmounts.
  useEffect(() => {
    if (!hasTrack) clearMediaSession();
  }, [hasTrack]);
  useEffect(() => () => clearMediaSession(), []);

  // Metadata on track change.
  const trackKey = track ? `${track.id}|${track.title}|${track.artist}|${artworkAlbumId(track)}|${albumOverride ?? ''}` : '';
  useEffect(() => {
    const session = getMediaSession();
    if (!session || !track || typeof MediaMetadata === 'undefined') return;
    session.metadata = new MediaMetadata(buildMediaMetadata(track, albumOverride));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [trackKey]);

  useEffect(() => {
    const session = getMediaSession();
    if (!session || !hasTrack) return;
    session.playbackState = isPlaying ? 'playing' : 'paused';
  }, [hasTrack, isPlaying]);

  // Position: re-sent on load, play/pause and seeks. The OS extrapolates in between, so a
  // steady timeupdate stream is only forwarded when it drifts from that extrapolation.
  const lastPositionRef = useRef<{ position: number; at: number; playing: boolean } | null>(null);
  // A new track or duration resets the extrapolation baseline (runs before the effect below).
  useEffect(() => {
    lastPositionRef.current = null;
  }, [trackKey, duration]);
  useEffect(() => {
    const session = getMediaSession();
    if (!session) return;
    if (!seekable) {
      lastPositionRef.current = null;
      if (hasTrack) {
        try {
          session.setPositionState?.();
        } catch {
          // See clearMediaSession.
        }
      }
      return;
    }
    const now = Date.now();
    const last = lastPositionRef.current;
    const expected = last ? last.position + (last.playing ? (now - last.at) / 1000 : 0) : null;
    const changedState = !last || last.playing !== isPlaying;
    if (!changedState && expected != null && Math.abs(expected - position) < 1.5) return;
    setPosition(session, duration, position);
    lastPositionRef.current = { position, at: now, playing: isPlaying };
  }, [duration, hasTrack, isPlaying, position, seekable, trackKey]);
}
