/**
 * Drives the desktop app's Hi-Res engine from the Player's state (plan §9.3). The page keeps the queue (D1): the
 * engine gets the current item (`load`) and the one after it (`setNext`, gapless). When the engine starts that next
 * item the Player moves its index along without a reload (`onAdvance`), like its own zero-gap hand-off.
 *
 * Every load and every next item gets its own engine id (`<track id>#<n>`), so repeat-one and the same song twice
 * in a row are still told apart.
 *
 * Sound (phase 5): the user's EQ, levelling, crossfade and Pure mode go to the engine as `setSound`; each song's
 * levelling gain and own curve ride on its play item and follow later changes with `setItemSound`.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../api';
import { isLiveTrack } from '../radioStations';
import type { ClientEntityId, Track } from '../types';
import { platform } from '../platform';
import { NativeLinks, type ResolvedLink } from './nativeLinks';
import { errorNotice, nativeBadge, resampledNotice, deviceGoneNotice, type NativeBadge, type NoticeTone } from './nativeFormat';
import type {
  NativeCommand,
  NativeEvent,
  NativeFormat,
  NativeItemSound,
  NativePathKind,
  NativePlayItem,
  NativeSoundSettings,
  NativeStatus,
} from './nativeTypes';

/** Links are fetched for the current item and this many after it. */
export const LINK_LOOKAHEAD = 10;

export type RepeatMode = 'off' | 'one' | 'all';

export interface NativePlayerOptions {
  /** "This PC" (the desktop engine) is the output and the engine is there. */
  enabled: boolean;
  queue: Track[];
  currentIndex: number;
  isPlaying: boolean;
  playToken: number;
  resumeAt?: { playToken: number; trackId: ClientEntityId; sec: number };
  /** Linear volume the engine should use (0 when muted, the sleep timer's fade included). */
  volume: number;
  repeatMode: RepeatMode;
  /** Shown in the tooltip and notices. */
  deviceName: string | null;
  /** The engine moved on to `queue[index]` by itself (already playing). */
  onAdvance: (index: number) => void;
  /** The last item ended. */
  onFinished: () => void;
  /** Where a loaded song should start (a remember-progress playlist); null or absent: from the start. */
  startAt?: (track: Track) => number | null;
  notify: (text: string, tone: NoticeTone) => void;
  /** EQ, levelling, crossfade, Pure mode, and each song's gain and own curve (by queue track id). */
  sound?: NativeSound;
  /** The engine's latest sound for the VU meter (~20 Hz while playing). */
  onLevels?: (wave: number[]) => void;
}

export interface NativeSound {
  settings: NativeSoundSettings;
  items: Record<string, NativeItemSound>;
}

export interface NativePlayerView {
  status: NativeStatus;
  positionSec: number;
  durationSec: number | null;
  badge: NativeBadge | null;
  /** The current item can't be played (shown with the player's own error line). */
  error: string | null;
  /** Queue track id the engine is playing (null while loading). */
  trackId: string | null;
  seek: (sec: number) => void;
}

interface Playing {
  source: NativeFormat;
  output: NativeFormat;
  path: NativePathKind[];
  trackId: string;
  radio: boolean;
  originalCodec: string | null;
}

/** The index after `index`, following repeat (null at the end of the queue). */
export function nextIndexAfter(index: number, length: number, repeat: RepeatMode): number | null {
  if (length === 0) return null;
  if (repeat === 'one') return index;
  if (index + 1 < length) return index + 1;
  return repeat === 'all' ? 0 : null;
}

/** The engine's item for a track and its link. */
export function playItemFor(
  engineId: string,
  track: Track,
  link: ResolvedLink,
  sound?: NativeItemSound | null,
): NativePlayItem {
  return {
    trackId: engineId,
    url: link.url,
    formatHint: link.format?.toLowerCase() ?? null,
    durationSec: link.durationSec ?? track.duration ?? null,
    serverDecoded: link.serverDecoded,
    live: link.live,
    gainDb: sound?.gainDb ?? 0,
    eq: sound?.eq ?? null,
  };
}

const soundKey = (sound: NativeItemSound | null | undefined) => JSON.stringify([sound?.gainDb ?? 0, sound?.eq ?? null]);

const CANT_PLAY = "This song can't be played.";

function send(command: NativeCommand): void {
  platform.audio?.command(command).catch((error) => {
    console.warn('[Native] command failed', command.type, error);
  });
}

export function useNativePlayer(options: NativePlayerOptions): NativePlayerView {
  const { enabled, queue, currentIndex, isPlaying, playToken, volume, repeatMode } = options;
  const optionsRef = useRef(options);
  optionsRef.current = options;
  const currentTrack = queue[currentIndex] ?? null;

  const [status, setStatus] = useState<NativeStatus>('idle');
  const [position, setPosition] = useState(0);
  const [duration, setDuration] = useState<number | null>(null);
  const [playing, setPlaying] = useState<Playing | null>(null);
  const [error, setError] = useState<string | null>(null);

  const linksRef = useRef<NativeLinks | null>(null);
  const clientRef = useRef('');
  /** Numbers engine ids (`<track id>#<n>`). */
  const seqRef = useRef(0);
  /** The latest load: an older one still waiting for its link gives up. Kept apart from `seqRef`, or a next-item
   *  refresh finishing first would cancel the load (Next pressed: the song changed, the old one kept playing). */
  const loadSeqRef = useRef(0);
  /** A load is waiting for its link; the next item is sent once it's done. */
  const loadingRef = useRef(false);
  /** Engine id → the queue item it stands for. */
  const enginesRef = useRef(new Map<string, { trackId: string; index: number }>());
  const currentEngineRef = useRef<string | null>(null);
  const nextRef = useRef<{ engineId: string; trackId: string; index: number } | null>(null);
  /** The next item asked for (`<track id>@<index>`, or '' for none): set before the link fetch so overlapping
   *  calls don't send it twice (a second id for the same item would make the engine restart). */
  const wantedNextRef = useRef<string | null>(null);
  /** Track id the Player was moved to because the engine already plays it. */
  const handoffRef = useRef<string | null>(null);
  const lostRef = useRef(false);
  const wasEnabledRef = useRef(false);
  /** The engine id of the last load (before it starts, `currentEngineRef` still names the old one). */
  const loadedEngineRef = useRef<string | null>(null);
  /** What each engine id was last told about its gain and curve. */
  const sentSoundRef = useRef(new Map<string, string>());

  /** A song's gain and curve now, remembered as sent for `engineId`. */
  const itemSound = useCallback((engineId: string, trackId: string): NativeItemSound | null => {
    const sound = optionsRef.current.sound?.items[trackId] ?? null;
    sentSoundRef.current.set(engineId, soundKey(sound));
    return sound;
  }, []);

  const links = useCallback((): NativeLinks => {
    if (!linksRef.current) {
      linksRef.current = new NativeLinks(
        async (ids) => (await api.nativeStreamTokens(ids, clientRef.current)).tokens,
        api.absoluteUrl,
      );
    }
    return linksRef.current;
  }, []);

  // Machine name for the server's Activity page.
  useEffect(() => {
    if (!enabled || clientRef.current) return;
    platform.audio?.machineName().then((name) => { clientRef.current = name; }).catch(() => {});
  }, [enabled]);

  /** Queues (or clears) the item after the current one. */
  const sendNext = useCallback(async () => {
    const o = optionsRef.current;
    const current = o.queue[o.currentIndex];
    const index = current && !isLiveTrack(current) ? nextIndexAfter(o.currentIndex, o.queue.length, o.repeatMode) : null;
    const track = index == null ? null : o.queue[index];
    if (!track || isLiveTrack(track)) {
      wantedNextRef.current = '';
      if (nextRef.current) {
        nextRef.current = null;
        send({ type: 'setNext', item: null });
      }
      return;
    }
    const trackId = String(track.id);
    const wanted = `${trackId}@${index}`;
    if (wantedNextRef.current === wanted) return;
    wantedNextRef.current = wanted;
    const link = (await links().get([trackId])).get(trackId);
    if (!link || wantedNextRef.current !== wanted) return;
    const engineId = `${trackId}#${++seqRef.current}`;
    enginesRef.current.set(engineId, { trackId, index: index! });
    nextRef.current = { engineId, trackId, index: index! };
    send({ type: 'setNext', item: playItemFor(engineId, track, link, itemSound(engineId, trackId)) });
  }, [itemSound, links]);

  const handleEvent = useCallback((event: NativeEvent) => {
    const o = optionsRef.current;
    switch (event.type) {
      case 'state':
        setStatus(event.status);
        setPosition(event.positionSec);
        setDuration(event.durationSec);
        if (event.status === 'stopped' && lostRef.current) {
          lostRef.current = false;
          o.notify(deviceGoneNotice(o.deviceName), 'danger');
        }
        if (event.status === 'playing') lostRef.current = false;
        return;
      case 'trackStarted': {
        const info = enginesRef.current.get(event.trackId);
        currentEngineRef.current = event.trackId;
        if (!info) return;
        const track = o.queue[info.index];
        setPlaying({
          source: event.source,
          output: event.output,
          path: event.path,
          trackId: info.trackId,
          radio: isLiveTrack(track),
          originalCodec: event.path.some((p) => p.kind === 'serverConverted') ? track?.format?.toUpperCase() ?? null : null,
        });
        setError(null);
        const resampled = resampledNotice(event.source, event.path);
        if (resampled) o.notify(resampled, 'muted');
        if (nextRef.current?.engineId === event.trackId) {
          nextRef.current = null;
          wantedNextRef.current = null;
          handoffRef.current = info.trackId;
          o.onAdvance(info.index);
        }
        return;
      }
      case 'trackEnded':
        if (event.trackId === currentEngineRef.current && !nextRef.current) {
          currentEngineRef.current = null;
          o.onFinished();
        }
        return;
      case 'needNext':
        void sendNext();
        return;
      case 'pathChanged':
        setPlaying((p) => (p ? { ...p, path: event.path } : p));
        return;
      case 'levels':
        o.onLevels?.(event.wave);
        return;
      case 'tokenExpired': {
        const info = enginesRef.current.get(event.trackId);
        if (!info) return;
        void links().refresh(info.trackId).then((link) => {
          if (link) send({ type: 'refreshUrl', trackId: event.trackId, url: link.url });
        });
        return;
      }
      case 'error': {
        const info = event.trackId ? enginesRef.current.get(event.trackId) : undefined;
        const track = info ? o.queue[info.index] : o.queue[o.currentIndex];
        if (event.code === 'device_lost') lostRef.current = true;
        const notice = errorNotice(event.code, o.deviceName, track?.title ?? null);
        if (notice) o.notify(notice.text, notice.tone);
        else if (!event.trackId || event.trackId === currentEngineRef.current || info?.index === o.currentIndex) setError(CANT_PLAY);
        return;
      }
      default:
    }
  }, [links, sendNext]);

  // Engine events while this output is chosen.
  useEffect(() => {
    if (!enabled || !platform.audio) return undefined;
    let unsubscribe: (() => void) | null = null;
    let cancelled = false;
    platform.audio.onEvent(handleEvent).then((off) => {
      if (cancelled) off();
      else unsubscribe = off;
    }).catch(() => {});
    return () => {
      cancelled = true;
      unsubscribe?.();
    };
  }, [enabled, handleEvent]);

  // Leaving this output: the engine stops (the page plays again, or a speaker does).
  useEffect(() => {
    if (enabled) {
      wasEnabledRef.current = true;
      return;
    }
    if (!wasEnabledRef.current) return;
    wasEnabledRef.current = false;
    send({ type: 'stop' });
    currentEngineRef.current = null;
    loadedEngineRef.current = null;
    nextRef.current = null;
    setPlaying(null);
    setStatus('idle');
  }, [enabled]);

  // The sound settings, before any load (effects run in order) and whenever they change.
  const soundSettingsKey = JSON.stringify(options.sound?.settings ?? null);
  useEffect(() => {
    const settings = optionsRef.current.sound?.settings;
    if (!enabled || !settings) return;
    send({ type: 'setSound', sound: settings });
  }, [enabled, soundSettingsKey]);

  // Load the current item on every explicit play (new playToken or another song), unless the engine already
  // moved on to it by itself.
  useEffect(() => {
    if (!enabled || !currentTrack) return;
    const trackId = String(currentTrack.id);
    if (handoffRef.current === trackId) {
      handoffRef.current = null;
      void sendNext();
      return;
    }
    handoffRef.current = null;
    const seq = ++loadSeqRef.current;
    loadingRef.current = true;
    const o = optionsRef.current;
    const resume = o.resumeAt && o.resumeAt.playToken === playToken && String(o.resumeAt.trackId) === trackId
      ? o.resumeAt.sec
      : (o.startAt?.(currentTrack) ?? currentTrack.startAtSec ?? 0);
    setError(null);
    setPlaying(null);
    setPosition(resume);
    setStatus('loading');
    nextRef.current = null;
    wantedNextRef.current = null;
    const upcoming = o.queue.slice(currentIndex, currentIndex + 1 + LINK_LOOKAHEAD).map((t) => String(t.id));
    void (async () => {
      let resolved: Map<string, ResolvedLink>;
      try {
        resolved = await links().get(upcoming);
      } catch {
        if (seq === loadSeqRef.current) {
          loadingRef.current = false;
          const notice = errorNotice('network', optionsRef.current.deviceName, currentTrack.title ?? null);
          if (notice) optionsRef.current.notify(notice.text, notice.tone);
          setStatus('stopped');
        }
        return;
      }
      if (seq !== loadSeqRef.current) return;
      loadingRef.current = false;
      const link = resolved.get(trackId);
      if (!link) {
        setError(CANT_PLAY);
        setStatus('stopped');
        return;
      }
      const engineId = `${trackId}#${++seqRef.current}`;
      enginesRef.current.set(engineId, { trackId, index: currentIndex });
      loadedEngineRef.current = engineId;
      send({
        type: 'load',
        item: playItemFor(engineId, currentTrack, link, itemSound(engineId, trackId)),
        startSec: link.live ? 0 : resume,
        autoplay: optionsRef.current.isPlaying,
      });
      await sendNext();
    })();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, playToken, currentTrack?.id]);

  // Queue edits, repeat changes and moves: keep the engine's next item right (a load sends its own).
  useEffect(() => {
    if (!enabled || !currentEngineRef.current || loadingRef.current) return;
    void sendNext();
  }, [enabled, queue, currentIndex, repeatMode, sendNext]);

  // A song's gain or curve arrived or changed (loudness values, a saved curve, an edit in the EQ panel).
  const itemsKey = JSON.stringify(options.sound?.items ?? {});
  useEffect(() => {
    if (!enabled) return;
    const engineIds = new Set(
      [loadedEngineRef.current, currentEngineRef.current, nextRef.current?.engineId].filter((id): id is string => !!id),
    );
    for (const engineId of engineIds) {
      const info = enginesRef.current.get(engineId);
      if (!info) continue;
      const sound = optionsRef.current.sound?.items[info.trackId] ?? null;
      const key = soundKey(sound);
      if (sentSoundRef.current.get(engineId) === key) continue;
      sentSoundRef.current.set(engineId, key);
      send({ type: 'setItemSound', trackId: engineId, gainDb: sound?.gainDb ?? 0, eq: sound?.eq ?? null });
    }
  }, [enabled, itemsKey]);

  useEffect(() => {
    if (!enabled) return;
    send({ type: isPlaying ? 'play' : 'pause' });
  }, [enabled, isPlaying]);

  useEffect(() => {
    if (!enabled) return;
    send({ type: 'setVolume', linear: Math.max(0, Math.min(1, volume)) });
  }, [enabled, volume]);

  const seek = useCallback((sec: number) => {
    setPosition(sec);
    send({ type: 'seek', sec });
  }, []);

  const badge = playing
    ? nativeBadge({
      source: playing.source,
      output: playing.output,
      path: playing.path,
      deviceName: options.deviceName,
      radio: playing.radio,
      originalCodec: playing.originalCodec,
    })
    : null;

  return { status, positionSec: position, durationSec: duration, badge, error, trackId: playing?.trackId ?? null, seek };
}
