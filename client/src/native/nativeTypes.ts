/**
 * The desktop app's Hi-Res engine, as the page sees it (`boogiebox-audio` crate, `types.rs`). Commands go out as
 * JSON through the shell's `audio_command`; events come back on the `audio-event` Tauri event.
 */

import type { ParametricEqBand } from '../audio/eq';

export type NativeOutputMode = 'shared' | 'exclusive';

/** An EQ band as the engine takes it (the web player's parametric band; extra fields are ignored). */
export type NativeEqBand = Pick<ParametricEqBand, 'type' | 'frequencyHz' | 'gainDb' | 'q' | 'enabled'>;

/** EQ, levelling, crossfade and Pure mode for the engine. */
export interface NativeSoundSettings {
  /** Nothing changes the samples. */
  pure: boolean;
  /** The user's own curve. */
  eq: NativeEqBand[];
  /** Songs may bring their own curve (saved EQ, Auto EQ). */
  itemEq: boolean;
  levelling: boolean;
  /** 0: gapless. */
  crossfadeSec: number;
}

/** One song's levelling gain and own curve. */
export interface NativeItemSound {
  gainDb: number;
  eq: NativeEqBand[] | null;
}

export interface NativePlayItem {
  /** Unique per load: the engine reports starts and ends by this id. */
  trackId: string;
  url: string;
  formatHint?: string | null;
  durationSec?: number | null;
  /** The server decodes this file for the app (Opus, WMA). */
  serverDecoded?: boolean;
  /** Internet radio: no seeking, no length. */
  live?: boolean;
  /** Volume levelling gain (dB). */
  gainDb?: number;
  /** The song's own curve (saved or Auto EQ); null: the user's EQ. */
  eq?: NativeEqBand[] | null;
}

export type NativeCommand =
  | { type: 'load'; item: NativePlayItem; startSec: number; autoplay: boolean }
  | { type: 'setNext'; item: NativePlayItem | null }
  | { type: 'play' }
  | { type: 'pause' }
  | { type: 'stop' }
  | { type: 'seek'; sec: number }
  | { type: 'setVolume'; linear: number }
  | { type: 'refreshUrl'; trackId: string; url: string }
  | { type: 'setSound'; sound: NativeSoundSettings }
  | { type: 'setItemSound'; trackId: string; gainDb: number; eq: NativeEqBand[] | null };

export type NativeStatus = 'idle' | 'loading' | 'playing' | 'paused' | 'stopped';

export interface NativeFormat {
  sampleRate: number;
  bits: number | null;
  channels: number;
  /** `FLAC`, `ALAC`, `MP3`, `AAC`, `Vorbis`, `PCM`; device side: `s16`, `s24in32`, `f32`… */
  codec: string;
}

export type NativePathKind =
  | { kind: 'bitPerfect' }
  | { kind: 'volumeApplied' }
  | { kind: 'dspApplied' }
  | { kind: 'eqApplied' }
  | { kind: 'levelled' }
  | { kind: 'crossfade' }
  | { kind: 'resampled'; toHz: number }
  | { kind: 'sharedMixer' }
  | { kind: 'downmixed' }
  | { kind: 'serverConverted' };

export type NativeErrorCode =
  | 'network'
  | 'decode'
  | 'device_busy'
  | 'device_lost'
  | 'format_unsupported'
  | 'token_expired'
  | 'exclusive_not_allowed';

export interface NativeDevice {
  id: string;
  name: string;
  isDefault: boolean;
  /** Highest exclusive-mode rate (Hz), when known. */
  maxRate?: number;
  maxBits?: number;
  /** Exclusive mode takes one rate only. */
  singleRate?: boolean;
}

export type NativeEvent =
  | { type: 'state'; status: NativeStatus; trackId: string | null; positionSec: number; durationSec: number | null }
  | { type: 'trackStarted'; trackId: string; source: NativeFormat; output: NativeFormat; path: NativePathKind[] }
  | { type: 'trackEnded'; trackId: string }
  | { type: 'needNext'; trackId: string }
  | { type: 'pathChanged'; trackId: string; path: NativePathKind[] }
  | { type: 'deviceChanged'; devices: NativeDevice[]; active: string | null }
  | { type: 'tokenExpired'; trackId: string }
  | { type: 'error'; trackId: string | null; code: NativeErrorCode; message: string }
  /** ~20 Hz while playing: levels and the latest ~21 ms of sound (mono, ~24 kHz, -127..127). */
  | { type: 'levels'; peak: number; rms: number; wave: number[] };

/** What Windows' media controls show while "This PC" plays (`media_controls.rs`). */
export interface NativeNowPlaying {
  title: string;
  artist?: string | null;
  album?: string | null;
  albumArtist?: string | null;
  /** Absolute album art URL (needs no sign-in). */
  artUrl?: string | null;
  durationSec?: number | null;
  /** Internet radio: no timeline. */
  live?: boolean;
}

/** A button pressed in Windows (media keys, the flyout, the lock screen, the taskbar thumbnail). */
export interface NativeMediaAction {
  action: 'play' | 'pause' | 'toggle' | 'next' | 'previous' | 'stop' | 'seek';
  sec?: number;
}

/** Settings › Audio output (saved by the desktop app, not per user). */
export interface NativeAudioSettings {
  deviceId: string | null;
  mode: NativeOutputMode;
  /** 20 (Low), 50 (Normal) or 150 (Safe). */
  bufferMs: number;
  nearestRate: boolean;
  /** Pure mode; null/absent: on in Exclusive mode, off in Shared mode. */
  pure?: boolean | null;
}

/** Pure mode as it applies now (on by default in Exclusive mode). */
export function pureModeOn(settings: NativeAudioSettings | null | undefined): boolean {
  if (!settings) return true;
  return settings.pure ?? settings.mode === 'exclusive';
}
