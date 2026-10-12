/**
 * One shared `EventSource` on `/api/remote/events` for every Play On hook: opened on the first subscriber, closed
 * after the last, reconnected with backoff, and closed while the tab is hidden for more than five minutes.
 */

import { api } from '../api';
import type { RemoteEvent } from './remoteTypes';

type Listener = (event: RemoteEvent) => void;

const EVENT_NAMES = ['renderers', 'session', 'position', 'takeover', 'notice', 'sessionEnded'] as const;
export const REMOTE_EVENTS_HIDDEN_CLOSE_MS = 5 * 60_000;
const RECONNECT_MIN_MS = 1000;
const RECONNECT_MAX_MS = 60_000;

const listeners = new Set<Listener>();
let source: EventSource | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let hiddenTimer: ReturnType<typeof setTimeout> | null = null;
let reconnectDelay = RECONNECT_MIN_MS;
let pausedWhileHidden = false;

function emit(event: RemoteEvent) {
  for (const listener of [...listeners]) {
    try { listener(event); } catch (error) { console.warn('[remote] event listener failed', error); }
  }
}

function closeSource() {
  if (reconnectTimer) { clearTimeout(reconnectTimer); reconnectTimer = null; }
  source?.close();
  source = null;
}

function scheduleReconnect() {
  if (reconnectTimer || !listeners.size || pausedWhileHidden) return;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    openSource();
  }, reconnectDelay);
  reconnectDelay = Math.min(reconnectDelay * 2, RECONNECT_MAX_MS);
}

function openSource() {
  if (source || !listeners.size || typeof EventSource === 'undefined') return;
  const next = new EventSource(api.remote.eventsUrl(), { withCredentials: true });
  source = next;
  next.onopen = () => {
    reconnectDelay = RECONNECT_MIN_MS;
    emit({ type: 'connected' });
  };
  next.onerror = () => {
    // The browser retries on its own unless the stream was refused (401/404 → CLOSED).
    if (next.readyState === EventSource.CLOSED && source === next) {
      source = null;
      scheduleReconnect();
    }
  };
  for (const name of EVENT_NAMES) {
    next.addEventListener(name, (message) => {
      try {
        emit(JSON.parse((message as MessageEvent<string>).data) as RemoteEvent);
      } catch {
        // Ignore malformed frames.
      }
    });
  }
}

function onVisibilityChange() {
  if (document.hidden) {
    if (!hiddenTimer) {
      hiddenTimer = setTimeout(() => {
        hiddenTimer = null;
        pausedWhileHidden = true;
        closeSource();
      }, REMOTE_EVENTS_HIDDEN_CLOSE_MS);
    }
    return;
  }
  if (hiddenTimer) { clearTimeout(hiddenTimer); hiddenTimer = null; }
  if (pausedWhileHidden) {
    pausedWhileHidden = false;
    reconnectDelay = RECONNECT_MIN_MS;
    openSource();
  }
}

/** Subscribes to Play On events; returns the unsubscribe function. */
export function subscribeRemoteEvents(listener: Listener): () => void {
  listeners.add(listener);
  if (listeners.size === 1 && typeof document !== 'undefined') {
    document.addEventListener('visibilitychange', onVisibilityChange);
  }
  openSource();
  return () => {
    listeners.delete(listener);
    if (listeners.size) return;
    closeSource();
    if (hiddenTimer) { clearTimeout(hiddenTimer); hiddenTimer = null; }
    pausedWhileHidden = false;
    reconnectDelay = RECONNECT_MIN_MS;
    if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', onVisibilityChange);
  };
}
