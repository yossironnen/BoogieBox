/**
 * Wording for the desktop app's Hi-Res output: the dock badge, its tooltip and the notices. Every string comes
 * from `wip/native-hires-approved-copy.md` (approved 2026-11-15).
 */

import type { NativeErrorCode, NativeFormat, NativePathKind } from './nativeTypes';

/** `44.1`, `96`, `176.4` from a rate in Hz. */
export function khz(rate: number): string {
  const k = rate / 1000;
  return Number.isInteger(k) ? String(k) : String(Math.round(k * 10) / 10);
}

const LOSSY = new Set(['MP3', 'AAC', 'VORBIS', 'OPUS', 'WMA']);

/** Codec name for display (`FLAC`, `Vorbis`…); the engine reports server-decoded files as PCM. */
function codecLabel(codec: string): string {
  const upper = codec.toUpperCase();
  if (upper === 'VORBIS') return 'Vorbis';
  if (upper === 'OPUS') return 'Opus';
  return codec;
}

/** The badge's format part: `24/192` for lossless files, the codec name for lossy ones. */
export function formatPart(source: NativeFormat): string {
  if (source.bits && !LOSSY.has(source.codec.toUpperCase())) return `${source.bits}/${khz(source.sampleRate)}`;
  return codecLabel(source.codec);
}

function channelsLabel(channels: number): string {
  if (channels === 6) return '5.1';
  if (channels === 8) return '7.1';
  return `${channels} channels`;
}

export interface NativeBadge {
  label: string;
  /** Bit-perfect: shown in the accent colour. */
  accent: boolean;
  icon: 'bitPerfect' | 'volume' | 'resampled' | 'shared' | 'server' | 'downmix';
  /** Tooltip rows: File, To the DAC, Device. */
  rows: Array<[string, string]>;
  /** Tooltip last line. */
  note: string;
}

export interface BadgeInput {
  source: NativeFormat;
  output: NativeFormat;
  path: NativePathKind[];
  deviceName: string | null;
  /** Internet radio. */
  radio?: boolean;
  /** The file's own format when the server decoded it (`Opus`, `WMA`). */
  originalCodec?: string | null;
}

const has = (path: NativePathKind[], kind: NativePathKind['kind']) => path.some((p) => p.kind === kind);

function fileRow(source: NativeFormat, originalCodec?: string | null): string {
  if (originalCodec) return `${codecLabel(originalCodec)} · ${khz(source.sampleRate)} kHz`;
  const codec = codecLabel(source.codec);
  return source.bits && !LOSSY.has(source.codec.toUpperCase())
    ? `${codec} · ${source.bits}-bit · ${khz(source.sampleRate)} kHz`
    : `${codec} · ${khz(source.sampleRate)} kHz`;
}

function dacRow(output: NativeFormat, shared: boolean): string {
  const mode = shared ? 'shared' : 'exclusive';
  return output.codec === 'f32'
    ? `${khz(output.sampleRate)} kHz · ${mode}`
    : `${output.bits ?? 16}-bit · ${khz(output.sampleRate)} kHz · ${mode}`;
}

/** The dock badge for what the engine is playing. */
export function nativeBadge(input: BadgeInput): NativeBadge {
  const { source, output, path } = input;
  const shared = has(path, 'sharedMixer');
  const rows: Array<[string, string]> = [
    ['File', input.radio ? `Radio · ${khz(source.sampleRate)} kHz` : fileRow(source, input.originalCodec)],
    ['To the DAC', dacRow(output, shared)],
    ['Device', input.deviceName ?? 'System default'],
  ];
  const fmt = formatPart(source);
  const sharedNote = 'Windows mixes and may resample it.';
  if (input.radio) {
    return { label: 'Radio · Shared', accent: false, icon: 'shared', rows, note: sharedNote };
  }
  if (has(path, 'serverConverted')) {
    return {
      label: `${input.originalCodec ? codecLabel(input.originalCodec) : fmt} · Converted by server`,
      accent: false,
      icon: 'server',
      rows,
      note: 'The server decodes this file for the app.',
    };
  }
  if (has(path, 'downmixed')) {
    return {
      label: `${channelsLabel(source.channels)} · Mixed down to stereo`,
      accent: false,
      icon: 'downmix',
      rows,
      note: 'Surround channels are folded into left and right.',
    };
  }
  if (shared) return { label: `${fmt} · Shared`, accent: false, icon: 'shared', rows, note: sharedNote };
  const resampled = path.find((p): p is { kind: 'resampled'; toHz: number } => p.kind === 'resampled');
  if (resampled) {
    return {
      label: `${fmt} · Resampled to ${khz(resampled.toHz)} kHz`,
      accent: false,
      icon: 'resampled',
      rows,
      note: `This DAC can't play ${khz(source.sampleRate)} kHz.`,
    };
  }
  if (has(path, 'bitPerfect')) {
    return { label: `${fmt} · Bit-perfect`, accent: true, icon: 'bitPerfect', rows, note: 'Every sample reaches the DAC unchanged.' };
  }
  return {
    label: `${fmt} · Volume applied`,
    accent: false,
    icon: 'volume',
    rows,
    note: 'Set the volume to 100% for bit-perfect playback.',
  };
}

/** The notice for a resampled file (once per file). */
export function resampledNotice(source: NativeFormat, path: NativePathKind[]): string | null {
  const r = path.find((p): p is { kind: 'resampled'; toHz: number } => p.kind === 'resampled');
  if (!r || has(path, 'sharedMixer')) return null;
  return `This DAC can't play ${khz(source.sampleRate)} kHz. Playing at ${khz(r.toHz)} kHz instead.`;
}

export type NoticeTone = 'muted' | 'warning' | 'danger';

/** The notice for an engine error; `null` for errors the player shows its own way. */
export function errorNotice(
  code: NativeErrorCode,
  deviceName: string | null,
  title: string | null,
): { text: string; tone: NoticeTone } | null {
  const device = deviceName ?? 'DAC';
  switch (code) {
    case 'device_busy':
      return { text: `Another app is using the ${device}. Playing in Shared mode until it lets go.`, tone: 'warning' };
    case 'exclusive_not_allowed':
      return {
        text:
          'Exclusive mode is turned off for this device. Playing in Shared mode. To allow it: Windows Sound settings › '
          + 'the device › Advanced › "Allow applications to take exclusive control".',
        tone: 'warning',
      };
    case 'device_lost':
      return { text: `The ${device} was disconnected. Paused. Playback picks up here if it's back within 30 seconds.`, tone: 'danger' };
    case 'network':
    case 'token_expired':
      return {
        text: `Can't reach the server. ${title ? `"${title}" stopped. ` : ''}Check the connection and press Play.`,
        tone: 'danger',
      };
    default:
      return null;
  }
}

/** After a lost device stayed away for 30 s. */
export function deviceGoneNotice(deviceName: string | null): string {
  return `The ${deviceName ?? 'DAC'} didn't come back. Playback stopped. Press Play to carry on with another device.`;
}

/** Picker detail for a device: `up to 384 kHz` / `48 kHz only` (empty when unknown). */
export function deviceRateShort(device: { maxRate?: number; singleRate?: boolean }): string {
  if (!device.maxRate) return '';
  return device.singleRate ? `${khz(device.maxRate)} kHz only` : `up to ${khz(device.maxRate)} kHz`;
}

/** Settings card detail: `Up to 384 kHz · 32-bit` / `48 kHz · 16-bit only` (empty when unknown). */
export function deviceRateLong(device: { maxRate?: number; maxBits?: number; singleRate?: boolean }): string {
  if (!device.maxRate || !device.maxBits) return '';
  return device.singleRate
    ? `${khz(device.maxRate)} kHz · ${device.maxBits}-bit only`
    : `Up to ${khz(device.maxRate)} kHz · ${device.maxBits}-bit`;
}
