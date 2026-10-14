/**
 * Play On orchestration for the whole app: the playback target, the renderer list, the target's server session,
 * the PlayerState bridge, handoffs between this browser and renderers, takeover prompts and inline notices.
 */

import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type SetStateAction } from 'react';
import { api, RemoteApiError } from '../api';
import type { PlayerState } from '../components/Player';
import type { Track } from '../types';
import { subscribeRemoteEvents } from './remoteEvents';
import {
  applyLocalSeek,
  applyPositionTick,
  applySessionSnapshot,
  interpolatePosition,
  sessionQueueTracks,
  sessionToLocalState,
  sessionToPlayerState,
  trackToItemRef,
  translatePlayerStateChange,
  type RemoteAction,
  type RemoteSessionView,
} from './remoteBridge';
import type {
  PlaybackTarget,
  RemoteEvent,
  RemoteRepeat,
  RemoteSessionState,
  RemoteSessionStatus,
  RendererInfo,
  SessionInUseInfo,
} from './remoteTypes';
import { usePlaybackTarget } from './usePlaybackTarget';

const LOCAL: PlaybackTarget = { kind: 'local' };
const REMOTE_PLAY_TOKEN = 0;
const NOTICE_MS = 6000;
const TAKEOVER_NOTICE_MS = 15000;
const VOLUME_DEBOUNCE_MS = 150;
const DISCOVERY_SPIN_MS = 2500;
const KNOWN_TRACKS_LIMIT = 10000;

export interface PlayOnNotice {
  key: number;
  text: string;
  tone: 'muted' | 'warning' | 'danger';
  action?: { label: string; run: () => void };
}

/** What the Player needs to render a renderer session. */
export interface PlayerRemoteView {
  udn: string;
  deviceName: string;
  status: RemoteSessionStatus;
  positionSec: number;
  /** `Date.now()` of `positionSec`. */
  positionAt: number;
  durationSec: number | null;
  /** 0–1, or null when the device doesn't report it. */
  volume: number | null;
  muted: boolean;
  canSeek: boolean;
  canSetVolume: boolean;
  shuffle: boolean;
  repeat: RemoteRepeat;
}

export interface PlayerRemoteActions {
  next: () => void;
  prev: () => void;
  seek: (sec: number) => void;
  setVolume: (volume: number) => void;
  setMuted: (muted: boolean) => void;
  setShuffle: (on: boolean) => void;
  setRepeat: (mode: RemoteRepeat) => void;
}

export interface PlayOnTakeoverPrompt {
  info: SessionInUseInfo;
  /** The retried action replaces their queue (load/transfer), not just a control change. */
  replacesQueue: boolean;
  busy: boolean;
  /** Retries with `takeover: true`; resolves when done. */
  confirm: () => Promise<void>;
  cancel: () => void;
}

export interface PlayOnApi {
  /** False when Play On is switched off on the server. */
  available: boolean;
  target: PlaybackTarget;
  renderers: RendererInfo[];
  renderersLoaded: boolean;
  discovering: boolean;
  /** Keeps the renderer list live while a picker is open; returns the release function. */
  watchRenderers: () => () => void;
  refreshRenderers: () => void;
  selectTarget: (target: PlaybackTarget) => void;
  /** Renderer a handoff is currently moving to. */
  pendingUdn: string | null;
  /** Display name of the target renderer. */
  deviceName: string | null;
  remoteView: PlayerRemoteView | null;
  remoteActions: PlayerRemoteActions;
  /** The PlayerState every surface renders (the session's while a renderer is the target). */
  playerState: PlayerState;
  dispatch: (action: SetStateAction<PlayerState>) => void;
  notice: PlayOnNotice | null;
  takeover: PlayOnTakeoverPrompt | null;
  /** Opens Settings → Advanced → Play On; null when the user can't change those settings. */
  openSettings: (() => void) | null;
}

export const PlayOnContext = createContext<PlayOnApi | null>(null);

/** Play On state for components below `PlayOnContext.Provider`; null outside it (e.g. isolated tests). */
export function usePlayOnContext(): PlayOnApi | null {
  return useContext(PlayOnContext);
}

export interface UsePlayOnOptions {
  /** Signed in and set up. */
  enabled: boolean;
  currentUserId: string | null;
  localState: PlayerState;
  setLocalState: (action: SetStateAction<PlayerState>, options?: { alreadyRecorded?: boolean }) => void;
  /** Latest local playback position (seconds). */
  getLocalPosition: () => number;
  /** Admin-only navigation to the Play On settings. */
  onOpenSettings?: (() => void) | null;
}

function errorText(error: unknown): string {
  return error instanceof Error && error.message ? error.message : 'Play On request failed';
}

export function usePlayOn({ enabled, currentUserId, localState, setLocalState, getLocalPosition, onOpenSettings = null }: UsePlayOnOptions): PlayOnApi {
  const [target, setTargetState] = usePlaybackTarget();
  const targetUdn = target.kind === 'renderer' ? target.udn : null;
  const targetUdnRef = useRef(targetUdn);
  targetUdnRef.current = targetUdn;

  const [available, setAvailable] = useState(true);
  const [renderers, setRenderers] = useState<RendererInfo[]>([]);
  const [renderersLoaded, setRenderersLoaded] = useState(false);
  const [discovering, setDiscovering] = useState(false);
  const [watchers, setWatchers] = useState(0);
  const [view, setView] = useState<RemoteSessionView | null>(null);
  const viewRef = useRef(view);
  viewRef.current = view;
  const [optimistic, setOptimistic] = useState<PlayerState | null>(null);
  const [transient, setTransient] = useState<PlayOnNotice | null>(null);
  const [prompt, setPrompt] = useState<{ info: SessionInUseInfo; replacesQueue: boolean; retry: () => Promise<void> } | null>(null);
  const [promptBusy, setPromptBusy] = useState(false);
  const [pendingUdn, setPendingUdn] = useState<string | null>(null);
  const [volumeOverride, setVolumeOverride] = useState<number | null>(null);

  const knownTracksRef = useRef(new Map<string, Track>());
  const tracksByQidRef = useRef(new Map<number, Track>());
  const ownSnapshotRef = useRef<RemoteSessionState | null>(null);
  const chainRef = useRef<Promise<void>>(Promise.resolve());
  const inFlightRef = useRef(0);
  const noticeKeyRef = useRef(0);
  const noticeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const volumeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const volumeSeqRef = useRef(0);
  const rendererFetchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const startupCheckedRef = useRef(false);
  const localStateRef = useRef(localState);
  localStateRef.current = localState;
  const renderersRef = useRef(renderers);
  renderersRef.current = renderers;

  const setTarget = useCallback((next: PlaybackTarget) => {
    targetUdnRef.current = next.kind === 'renderer' ? next.udn : null;
    setTargetState(next);
  }, [setTargetState]);

  const showNotice = useCallback((notice: Omit<PlayOnNotice, 'key'>, ms = NOTICE_MS) => {
    if (noticeTimerRef.current) clearTimeout(noticeTimerRef.current);
    noticeKeyRef.current += 1;
    const key = noticeKeyRef.current;
    setTransient({ ...notice, key });
    noticeTimerRef.current = setTimeout(() => {
      setTransient((current) => (current?.key === key ? null : current));
    }, ms);
  }, []);

  useEffect(() => () => {
    if (noticeTimerRef.current) clearTimeout(noticeTimerRef.current);
    if (volumeTimerRef.current) clearTimeout(volumeTimerRef.current);
    if (rendererFetchTimerRef.current) clearTimeout(rendererFetchTimerRef.current);
  }, []);

  const rememberTracks = useCallback((tracks: Track[]) => {
    const known = knownTracksRef.current;
    if (known.size > KNOWN_TRACKS_LIMIT) known.clear();
    for (const track of tracks) {
      if (track.remoteQid == null) known.set(String(track.id), track);
    }
  }, []);

  const applyState = useCallback((state: RemoteSessionState) => {
    if (state.ownerUserId && state.ownerUserId === currentUserId) ownSnapshotRef.current = state;
    if (state.udn !== targetUdnRef.current) return;
    setView((current) => applySessionSnapshot(current, state, Date.now()));
  }, [currentUserId]);

  // ─── Renderer list ──────────────────────────────────────────────────────
  const loadRenderers = useCallback(() => {
    api.remote.renderers()
      .then((response) => {
        setAvailable(true);
        setRenderers(response.renderers);
      })
      .catch((error: unknown) => {
        if (error instanceof RemoteApiError && error.status === 404) {
          setAvailable(false);
          setRenderers([]);
        }
      })
      .finally(() => setRenderersLoaded(true));
  }, []);

  const scheduleRendererFetch = useCallback(() => {
    if (rendererFetchTimerRef.current) clearTimeout(rendererFetchTimerRef.current);
    rendererFetchTimerRef.current = setTimeout(() => {
      rendererFetchTimerRef.current = null;
      loadRenderers();
    }, 250);
  }, [loadRenderers]);

  useEffect(() => {
    if (enabled) loadRenderers();
  }, [enabled, loadRenderers]);

  useEffect(() => {
    if (enabled && watchers > 0) loadRenderers();
  }, [enabled, watchers > 0, loadRenderers]); // eslint-disable-line react-hooks/exhaustive-deps

  const watchRenderers = useCallback(() => {
    setWatchers((count) => count + 1);
    let released = false;
    return () => {
      if (released) return;
      released = true;
      setWatchers((count) => Math.max(0, count - 1));
    };
  }, []);

  const refreshRenderers = useCallback(() => {
    setDiscovering(true);
    api.remote.refresh().catch(() => {});
    setTimeout(() => {
      setDiscovering(false);
      loadRenderers();
    }, DISCOVERY_SPIN_MS);
  }, [loadRenderers]);

  const activeRenderer = useMemo(
    () => (targetUdn ? renderers.find((r) => r.udn === targetUdn) ?? null : null),
    [renderers, targetUdn],
  );
  const deviceName = targetUdn ? activeRenderer?.name ?? view?.state.rendererName ?? 'Speaker' : null;
  const deviceNameRef = useRef(deviceName);
  deviceNameRef.current = deviceName;

  // A stored renderer that is gone or offline at startup: play here instead (plan 8.7).
  useEffect(() => {
    if (!renderersLoaded || startupCheckedRef.current) return;
    startupCheckedRef.current = true;
    const udn = targetUdnRef.current;
    if (!udn) return;
    if (!available) { setTarget(LOCAL); return; }
    const renderer = renderers.find((r) => r.udn === udn);
    if (!renderer?.online) {
      setTarget(LOCAL);
      showNotice({ text: `${renderer?.name ?? 'The speaker'} is offline, playing here`, tone: 'muted' });
    }
  }, [available, renderers, renderersLoaded, setTarget, showNotice]);

  // Play On switched off on the server while a renderer was the target.
  useEffect(() => {
    if (!available && targetUdnRef.current) setTarget(LOCAL);
  }, [available, setTarget]);

  // ─── Target session ─────────────────────────────────────────────────────
  const fetchSession = useCallback((udn: string) => {
    api.remote.session(udn)
      .then((state) => {
        if (targetUdnRef.current !== udn) return;
        if (state) applyState(state);
        else setView(null);
      })
      .catch(() => {});
  }, [applyState]);

  useEffect(() => {
    setView((current) => (current && current.state.udn === targetUdn ? current : null));
    setOptimistic(null);
    setVolumeOverride(null);
    if (targetUdn && enabled) fetchSession(targetUdn);
  }, [enabled, fetchSession, targetUdn]);

  // ─── Command runner ─────────────────────────────────────────────────────
  const execute = useCallback((udn: string, action: RemoteAction, takeover: boolean): Promise<RemoteSessionState> => {
    switch (action.kind) {
      case 'load': return api.remote.load(udn, { ...action.body, takeover });
      case 'queue': return api.remote.queue(udn, action.op, takeover);
      default: return api.remote.command(udn, action.command, takeover);
    }
  }, []);

  const run = useCallback((udn: string, actions: RemoteAction[], takeover = false): Promise<void> => {
    if (!actions.length) return Promise.resolve();
    inFlightRef.current += 1;
    const task = chainRef.current.then(async () => {
      for (let i = 0; i < actions.length; i++) {
        if (targetUdnRef.current !== udn) return;
        try {
          applyState(await execute(udn, actions[i], takeover));
        } catch (error) {
          const remaining = actions.slice(i);
          if (error instanceof RemoteApiError && error.inUse) {
            setPrompt({ info: error.inUse, replacesQueue: remaining[0].kind === 'load', retry: () => run(udn, remaining, true) });
          } else if (error instanceof RemoteApiError && error.status === 404 && targetUdnRef.current === udn) {
            setView(null);
          } else {
            const unsupported = error instanceof RemoteApiError && error.code === 'unsupported';
            // Same words as the server's own fixed-volume notice, which may arrive just before this reply.
            const text = unsupported && /volume/i.test(errorText(error))
              ? `${deviceNameRef.current ?? 'This speaker'}'s volume is fixed. Use your amplifier`
              : errorText(error);
            showNotice({ text, tone: unsupported ? 'muted' : 'danger' });
          }
          return;
        }
      }
    }).finally(() => {
      inFlightRef.current -= 1;
      if (inFlightRef.current === 0) setOptimistic(null);
    });
    chainRef.current = task.catch(() => {});
    return task;
  }, [applyState, execute, showNotice]);

  // ─── PlayerState bridge ─────────────────────────────────────────────────
  const remoteTracks = useMemo(() => {
    if (!view) return [];
    const tracks = sessionQueueTracks(view.state.queue, knownTracksRef.current, tracksByQidRef.current);
    tracksByQidRef.current = new Map(tracks.map((track) => [track.remoteQid as number, track]));
    return tracks;
  }, [view?.state.queue]); // eslint-disable-line react-hooks/exhaustive-deps

  const remoteStatus = view?.state.status ?? null;
  const remoteCurrentQid = view?.state.currentQid ?? null;
  const remoteQueueSource = view?.state.queueSource;
  const remotePlayerState = useMemo(
    () => sessionToPlayerState(viewRef.current?.state ?? null, remoteTracks, REMOTE_PLAY_TOKEN),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [remoteTracks, remoteStatus === 'playing' || remoteStatus === 'loading', remoteCurrentQid, remoteQueueSource, view == null],
  );

  const playerState = targetUdn ? optimistic ?? remotePlayerState : localState;
  const playerStateRef = useRef(playerState);
  playerStateRef.current = playerState;

  const dispatch = useCallback((action: SetStateAction<PlayerState>) => {
    const udn = targetUdnRef.current;
    if (!udn) {
      setLocalState(action);
      return;
    }
    const prev = playerStateRef.current;
    const next = typeof action === 'function' ? action(prev) : action;
    if (next === prev) return;
    rememberTracks(next.queue);
    const actions = translatePlayerStateChange(prev, next);
    if (!actions.length) return;
    playerStateRef.current = next;
    setOptimistic(next);
    void run(udn, actions);
  }, [rememberTracks, run, setLocalState]);

  // ─── Handoffs ───────────────────────────────────────────────────────────
  const continueLocally = useCallback((session: RemoteSessionState, position: number, playing: boolean) => {
    const tracks = sessionQueueTracks(session.queue, knownTracksRef.current, tracksByQidRef.current);
    const local = sessionToLocalState(session, tracks, position, localStateRef.current.playToken);
    setLocalState({ ...local, isPlaying: playing && local.isPlaying }, { alreadyRecorded: true });
  }, [setLocalState]);

  const heldByOther = useCallback((session: RemoteSessionState | null) => (
    session != null && session.ownerUserId != null && session.ownerUserId !== currentUserId
    && (session.status === 'playing' || session.status === 'loading' || session.status === 'paused' || session.status === 'lostControl')
  ), [currentUserId]);

  const toRenderer = useCallback(async (udn: string, takeover: boolean) => {
    const fromUdn = targetUdnRef.current;
    const current = viewRef.current;
    setPendingUdn(udn);
    try {
      let state: RemoteSessionState | null = null;
      if (fromUdn && current && current.state.queue.length && !heldByOther(current.state)) {
        state = await api.remote.transfer(fromUdn, udn, takeover);
      } else if (!fromUdn && localStateRef.current.queue.length) {
        const local = localStateRef.current;
        rememberTracks(local.queue);
        const index = Math.min(Math.max(0, local.currentIndex), local.queue.length - 1);
        const position = getLocalPosition();
        state = await api.remote.load(udn, {
          items: local.queue.map(trackToItemRef),
          startIndex: index,
          startAtSec: position > 1 ? position : undefined,
          queueSource: local.queueSource,
          resumed: position > 1,
          takeover,
        });
        if (!local.isPlaying) state = await api.remote.command(udn, { cmd: 'pause' }, true);
        setLocalState((prev) => ({ ...prev, isPlaying: false }));
      }
      if (state?.ownerUserId === currentUserId) ownSnapshotRef.current = state;
      targetUdnRef.current = udn;
      setView(state ? applySessionSnapshot(null, state, Date.now()) : null);
      setTarget({ kind: 'renderer', udn });
    } catch (error) {
      if (error instanceof RemoteApiError && error.inUse) {
        setPrompt({ info: error.inUse, replacesQueue: true, retry: () => toRenderer(udn, true) });
      } else {
        showNotice({ text: errorText(error), tone: 'danger' });
      }
    } finally {
      setPendingUdn(null);
    }
  }, [currentUserId, getLocalPosition, heldByOther, rememberTracks, setLocalState, setTarget, showNotice]);

  const toLocal = useCallback(() => {
    const udn = targetUdnRef.current;
    const current = viewRef.current;
    setTarget(LOCAL);
    setView(null);
    if (!udn || !current) return;
    if (heldByOther(current.state)) return;
    if (current.state.queue.length) {
      const fallbackDuration = current.state.queue.find((e) => e.qid === current.state.currentQid)?.duration ?? 0;
      continueLocally(current.state, interpolatePosition(current, Date.now(), fallbackDuration ?? 0), true);
    }
    api.remote.release(udn).catch(() => {});
  }, [continueLocally, heldByOther, setTarget]);

  const selectTarget = useCallback((next: PlaybackTarget) => {
    const currentUdn = targetUdnRef.current;
    if (next.kind === 'local') {
      if (currentUdn) toLocal();
      return;
    }
    if (next.udn === currentUdn || pendingUdn) return;
    void toRenderer(next.udn, false);
  }, [pendingUdn, toLocal, toRenderer]);

  // ─── Events ─────────────────────────────────────────────────────────────
  const copyQueueHere = useCallback((snapshot: RemoteSessionState) => {
    continueLocally(snapshot, snapshot.positionSec, false);
    setTransient(null);
  }, [continueLocally]);

  const handleEvent = (event: RemoteEvent) => {
    const udn = targetUdnRef.current;
    switch (event.type) {
      case 'connected':
        if (udn) fetchSession(udn);
        if (watchers > 0 || udn) loadRenderers();
        return;
      case 'renderers':
        scheduleRendererFetch();
        return;
      case 'session':
        if (event.state.udn === udn) applyState(event.state);
        else if (event.state.ownerUserId === currentUserId) ownSnapshotRef.current = event.state;
        if (watchers > 0) scheduleRendererFetch();
        return;
      case 'position': {
        if (event.udn === udn) setView((current) => applyPositionTick(current, event, Date.now()));
        // Keep the "continue here" snapshot current while this user still holds the session.
        const own = ownSnapshotRef.current;
        if (own?.udn === event.udn && viewRef.current?.state.ownerUserId === currentUserId) {
          ownSnapshotRef.current = { ...own, positionSec: event.positionSec, status: event.status, sampledAtMs: event.sampledAtMs };
        }
        return;
      }
      case 'sessionEnded':
        if (event.udn === udn) { setView(null); setOptimistic(null); }
        if (watchers > 0) scheduleRendererFetch();
        return;
      case 'notice':
        if (event.udn === udn) {
          showNotice({ text: event.message, tone: event.level === 'error' ? 'danger' : event.level === 'warning' ? 'warning' : 'muted' });
        }
        return;
      case 'takeover': {
        if (event.udn !== udn || event.fromUserId !== currentUserId) return;
        const snapshot = ownSnapshotRef.current?.udn === udn ? ownSnapshotRef.current : null;
        const device = deviceNameRef.current ?? 'the speaker';
        setTarget(LOCAL);
        setView(null);
        setOptimistic(null);
        const text = `${event.byUserName} took over ${device}`;
        if (snapshot?.queue.length && !localStateRef.current.queue.length) {
          // Nothing queued here (e.g. the page was reloaded while on the speaker): keep their queue, paused, so
          // the player and this notice stay visible.
          continueLocally(snapshot, snapshot.positionSec, false);
          showNotice({ text, tone: 'muted' }, TAKEOVER_NOTICE_MS);
          return;
        }
        setLocalState((prev) => ({ ...prev, isPlaying: false }));
        showNotice({
          text,
          tone: 'muted',
          ...(snapshot?.queue.length ? { action: { label: 'Copy queue here', run: () => copyQueueHere(snapshot) } } : {}),
        }, TAKEOVER_NOTICE_MS);
        return;
      }
    }
  };
  const handleEventRef = useRef(handleEvent);
  handleEventRef.current = handleEvent;

  const wantEvents = enabled && available && (targetUdn != null || watchers > 0);
  useEffect(() => {
    if (!wantEvents) return undefined;
    return subscribeRemoteEvents((event) => handleEventRef.current(event));
  }, [wantEvents]);

  // ─── Player controls ────────────────────────────────────────────────────
  const command = useCallback((value: RemoteAction) => {
    const udn = targetUdnRef.current;
    if (udn) void run(udn, [value]);
  }, [run]);

  const remoteActions = useMemo<PlayerRemoteActions>(() => ({
    next: () => command({ kind: 'command', command: { cmd: 'next' } }),
    prev: () => command({ kind: 'command', command: { cmd: 'prev' } }),
    seek: (sec: number) => {
      const position = Math.max(0, sec);
      setView((current) => applyLocalSeek(current, position, Date.now()));
      command({ kind: 'command', command: { cmd: 'seek', value: position } });
    },
    setVolume: (volume: number) => {
      const udn = targetUdnRef.current;
      if (!udn) return;
      const value = Math.max(0, Math.min(1, volume));
      setVolumeOverride(value);
      volumeSeqRef.current += 1;
      const seq = volumeSeqRef.current;
      if (volumeTimerRef.current) clearTimeout(volumeTimerRef.current);
      volumeTimerRef.current = setTimeout(() => {
        volumeTimerRef.current = null;
        void run(udn, [{ kind: 'command', command: { cmd: 'volume', value: Math.round(value * 100) } }])
          .finally(() => { if (volumeSeqRef.current === seq) setVolumeOverride(null); });
      }, VOLUME_DEBOUNCE_MS);
    },
    setMuted: (muted: boolean) => command({ kind: 'command', command: { cmd: 'mute', value: muted } }),
    setShuffle: (on: boolean) => command({ kind: 'command', command: { cmd: 'shuffle', value: on } }),
    setRepeat: (mode: RemoteRepeat) => command({ kind: 'command', command: { cmd: 'repeat', value: mode } }),
  }), [command, run]);

  const remoteView = useMemo<PlayerRemoteView | null>(() => {
    if (!targetUdn || !view) return null;
    const s = view.state;
    return {
      udn: targetUdn,
      deviceName: deviceName ?? s.rendererName,
      status: s.status,
      positionSec: view.positionSec,
      positionAt: view.positionAt,
      durationSec: s.durationSec,
      volume: volumeOverride ?? (s.volume != null ? s.volume / 100 : null),
      muted: Boolean(s.muted),
      canSeek: s.canSeek,
      canSetVolume: s.canSetVolume && (activeRenderer?.capabilities.volume ?? true),
      shuffle: s.shuffle,
      repeat: s.repeat,
    };
  }, [activeRenderer, deviceName, targetUdn, view, volumeOverride]);

  // ─── Notices ────────────────────────────────────────────────────────────
  const statusNotice = useMemo<PlayOnNotice | null>(() => {
    if (!targetUdn || !remoteStatus) return null;
    const name = deviceName ?? 'The speaker';
    if (remoteStatus === 'lostControl') {
      return {
        key: -1,
        text: `${name} is playing another source`,
        tone: 'muted',
        action: { label: 'Resume here', run: () => command({ kind: 'command', command: { cmd: 'play' } }) },
      };
    }
    if (remoteStatus === 'offline') {
      return { key: -2, text: `${name} is unreachable`, tone: 'danger', action: { label: 'Use this browser', run: toLocal } };
    }
    return null;
  }, [command, deviceName, remoteStatus, targetUdn, toLocal]);

  const takeover = useMemo<PlayOnTakeoverPrompt | null>(() => {
    if (!prompt) return null;
    return {
      info: prompt.info,
      replacesQueue: prompt.replacesQueue,
      busy: promptBusy,
      confirm: () => {
        setPromptBusy(true);
        return prompt.retry().finally(() => {
          setPromptBusy(false);
          setPrompt(null);
        });
      },
      cancel: () => { if (!promptBusy) setPrompt(null); },
    };
  }, [prompt, promptBusy]);

  return {
    available,
    target,
    renderers,
    renderersLoaded,
    discovering,
    watchRenderers,
    refreshRenderers,
    selectTarget,
    pendingUdn,
    deviceName,
    remoteView,
    remoteActions,
    playerState,
    dispatch,
    notice: transient ?? statusNotice,
    takeover,
    openSettings: onOpenSettings,
  };
}
