/**
 * The desktop app's Hi-Res engine, as the page sees it (`boogiebox-audio` crate, `types.rs`). Commands go out as
 * JSON through the shell's `audio_command`; events come back on the `audio-event` Tauri event.
 */

export type NativeOutputMode = 'shared' | 'exclusive';

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
}

export type NativeCommand =
  | { type: 'load'; item: NativePlayItem; startSec: number; autoplay: boolean }
  | { type: 'setNext'; item: NativePlayItem | null }
  | { type: 'play' }
  | { type: 'pause' }
  | { type: 'stop' }
  | { type: 'seek'; sec: number }
  | { type: 'setVolume'; linear: number }
  | { type: 'refreshUrl'; trackId: string; url: string };

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
  | { type: 'error'; trackId: string | null; code: NativeErrorCode; message: string };

/** Settings › Audio output (saved by the desktop app, not per user). */
export interface NativeAudioSettings {
  deviceId: string | null;
  mode: NativeOutputMode;
  /** 20 (Low), 50 (Normal) or 150 (Safe). */
  bufferMs: number;
  nearestRate: boolean;
}
