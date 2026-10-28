/**
 * Play On (UPnP/DLNA renderer) wire types, mirroring the Rust `remote` module's JSON.
 */

import type { SleepMode, SleepTimer } from '../audio/sleepTimer';
import type { RadioReason } from '../types';

export type RemoteSessionStatus = 'idle' | 'loading' | 'playing' | 'paused' | 'stopped' | 'lostControl' | 'offline';
export type RemoteRepeat = 'off' | 'one' | 'all';
export type RemoteFormatPolicy = 'auto' | 'direct' | 'lossless' | 'mp3';

export interface RemoteTrackItem {
  kind: 'track';
  id: string;
  title: string;
  artist: string | null;
  album: string | null;
  albumId: string | null;
  duration: number | null;
  radioReason?: RadioReason | null;
}

export interface RemoteMixItem {
  kind: 'mix';
  id: string;
  title: string;
  duration: number | null;
}

/** One server queue entry; `qid` is stable across reorders. */
export type RemoteQueueEntry = (RemoteTrackItem | RemoteMixItem) & { qid: number };

/** Server-owned session snapshot (`GET /api/remote/sessions/{udn}`, SSE `session`). */
export interface RemoteSessionState {
  udn: string;
  rendererName: string;
  status: RemoteSessionStatus;
  /** Queue in play order. */
  queue: RemoteQueueEntry[];
  currentQid: number | null;
  positionSec: number;
  durationSec: number | null;
  sampledAtMs: number;
  volume: number | null;
  muted: boolean | null;
  shuffle: boolean;
  repeat: RemoteRepeat;
  queueSource: unknown;
  ownerUserId: string | null;
  ownerName: string | null;
  lastControllerUserId: string | null;
  canSeek: boolean;
  canSetVolume: boolean;
  revision: number;
  error: string | null;
  /** Sleep timer on this speaker (absent from older servers). */
  sleep?: SleepTimer | null;
}

export interface RendererSessionSummary {
  status: RemoteSessionStatus;
  ownerUserId: string | null;
  ownerName: string | null;
  nowPlaying: string | null;
  heldByOther: boolean;
}

export interface RendererInfo {
  udn: string;
  /** Alias when set, otherwise the device's friendly name. */
  name: string;
  friendlyName: string;
  alias: string | null;
  manufacturer: string | null;
  model: string | null;
  vendor: string;
  online: boolean;
  hidden: boolean;
  formatPolicy: RemoteFormatPolicy;
  hasIcon: boolean;
  capabilities: { setNext: boolean; seek: boolean; volume: boolean };
  lastSeenAt: string;
  session: RendererSessionSummary | null;
}

export interface RenderersResponse {
  running: boolean;
  renderers: RendererInfo[];
}

/** Admin diagnostics (`GET /api/remote/diagnostics`). */
export interface RemoteDiagnostics {
  enabled: boolean;
  running: boolean;
  detectedInterfaces: string[];
  searchInterfaces: string[];
  lastSearchMs: number | null;
  lastError: string | null;
  httpPort: number;
  renderers: number;
  online: number;
  sessions: number;
}

export type RemoteEvent =
  | { type: 'renderers' }
  | { type: 'session'; state: RemoteSessionState }
  | { type: 'position'; udn: string; status: RemoteSessionStatus; positionSec: number; durationSec: number | null; sampledAtMs: number }
  | { type: 'takeover'; udn: string; fromUserId: string; byUserId: string; byUserName: string }
  | { type: 'notice'; udn: string; level: 'info' | 'warning' | 'error'; message: string }
  | { type: 'sessionEnded'; udn: string }
  /** Client-side: the event stream (re)connected; refetch anything that may have been missed. */
  | { type: 'connected' };

/** 409 body when another user is actively holding the renderer. */
export interface SessionInUseInfo {
  code: 'session_in_use';
  udn: string;
  deviceName: string;
  ownerUserId: string;
  ownerName: string | null;
  status: RemoteSessionStatus;
  nowPlaying: { title: string } | null;
}

export interface RemoteItemRef {
  kind: 'track' | 'mix';
  id: string;
  radioReason?: RadioReason | null;
}

export interface RemoteLoadBody {
  items: RemoteItemRef[];
  startIndex: number;
  startAtSec?: number;
  queueSource?: unknown;
  shuffle?: boolean;
  repeat?: RemoteRepeat;
  /** Handoff: the start item already played elsewhere, so the server does not count it again. */
  resumed?: boolean;
  takeover?: boolean;
}

export type RemoteCommand =
  | { cmd: 'play' | 'pause' | 'toggle' | 'stop' | 'next' | 'prev' }
  | { cmd: 'seek'; value: number }
  | { cmd: 'jump'; value: number }
  | { cmd: 'shuffle' | 'mute'; value: boolean }
  | { cmd: 'repeat'; value: RemoteRepeat }
  | { cmd: 'volume'; value: number }
  | { cmd: 'sleep'; value: { mode: SleepMode; minutes?: number } }
  | { cmd: 'sleepClear' };

export type RemoteQueueOp =
  | { op: 'enqueue'; items: RemoteItemRef[]; mode: 'end' | 'next' }
  | { op: 'remove'; qid: number }
  | { op: 'move'; qid: number; toIndex: number }
  | { op: 'clear' };

/** Where playback happens, per browser. */
export type PlaybackTarget = { kind: 'local' } | { kind: 'renderer'; udn: string };
