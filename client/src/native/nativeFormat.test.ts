import { describe, expect, it } from 'vitest';
import {
  deviceGoneNotice,
  deviceRateLong,
  deviceRateShort,
  errorNotice,
  formatPart,
  khz,
  nativeBadge,
  resampledNotice,
} from './nativeFormat';
import type { NativeFormat, NativePathKind } from './nativeTypes';

const flac = (rate: number, bits: number | null = 24, channels = 2): NativeFormat => ({ sampleRate: rate, bits, channels, codec: 'FLAC' });
const out = (rate: number, codec = 's24in32', bits = 24): NativeFormat => ({ sampleRate: rate, bits, channels: 2, codec });

describe('native format wording', () => {
  it('writes rates and formats like the approved copy', () => {
    expect(khz(44_100)).toBe('44.1');
    expect(khz(192_000)).toBe('192');
    expect(khz(176_400)).toBe('176.4');
    expect(formatPart(flac(192_000))).toBe('24/192');
    expect(formatPart(flac(44_100, 16))).toBe('16/44.1');
    expect(formatPart({ sampleRate: 44_100, bits: null, channels: 2, codec: 'MP3' })).toBe('MP3');
    expect(formatPart({ sampleRate: 44_100, bits: null, channels: 2, codec: 'Vorbis' })).toBe('Vorbis');
  });

  const badge = (path: NativePathKind[], source = flac(192_000), output = out(192_000), extra = {}) =>
    nativeBadge({ source, output, path, deviceName: 'FiiO Q series', ...extra });

  it('labels every badge state', () => {
    const bp = badge([{ kind: 'bitPerfect' }]);
    expect(bp.label).toBe('24/192 · Bit-perfect');
    expect(bp.accent).toBe(true);
    expect(bp.note).toBe('Every sample reaches the DAC unchanged.');
    expect(bp.rows).toEqual([
      ['File', 'FLAC · 24-bit · 192 kHz'],
      ['To the DAC', '24-bit · 192 kHz · exclusive'],
      ['Device', 'FiiO Q series'],
    ]);

    const vol = badge([{ kind: 'volumeApplied' }]);
    expect(vol.label).toBe('24/192 · Volume applied');
    expect(vol.accent).toBe(false);
    expect(vol.note).toBe('Set the volume to 100% for bit-perfect playback.');

    const rs = badge([{ kind: 'resampled', toHz: 88_200 }], flac(176_400), out(88_200));
    expect(rs.label).toBe('24/176.4 · Resampled to 88.2 kHz');
    expect(rs.note).toBe("This DAC can't play 176.4 kHz.");

    const shared = badge([{ kind: 'sharedMixer' }, { kind: 'resampled', toHz: 48_000 }], flac(44_100, 16), out(48_000, 'f32', 32));
    expect(shared.label).toBe('16/44.1 · Shared');
    expect(shared.rows[1]).toEqual(['To the DAC', '48 kHz · shared']);
    expect(shared.note).toBe('Windows mixes and may resample it.');

    const server = badge([{ kind: 'serverConverted' }], { sampleRate: 48_000, bits: 24, channels: 2, codec: 'PCM' }, out(48_000), { originalCodec: 'OPUS' });
    expect(server.label).toBe('Opus · Converted by server');
    expect(server.rows[0]).toEqual(['File', 'Opus · 48 kHz']);

    const down = badge([{ kind: 'downmixed' }], flac(48_000, 24, 6), out(48_000));
    expect(down.label).toBe('5.1 · Mixed down to stereo');

    const radio = badge([{ kind: 'sharedMixer' }], { sampleRate: 44_100, bits: null, channels: 2, codec: 'MP3' }, out(48_000, 'f32', 32), { radio: true });
    expect(radio.label).toBe('Radio · Shared');
    expect(nativeBadge({ source: flac(96_000), output: out(96_000), path: [], deviceName: null }).rows[2]).toEqual(['Device', 'System default']);
  });

  it('words the notices', () => {
    expect(resampledNotice(flac(352_800), [{ kind: 'resampled', toHz: 176_400 }])).toBe(
      "This DAC can't play 352.8 kHz. Playing at 176.4 kHz instead.",
    );
    expect(resampledNotice(flac(44_100), [{ kind: 'sharedMixer' }, { kind: 'resampled', toHz: 48_000 }])).toBeNull();
    expect(resampledNotice(flac(96_000), [{ kind: 'bitPerfect' }])).toBeNull();
    expect(errorNotice('device_busy', 'FiiO Q series', null)?.text).toBe(
      'Another app is using the FiiO Q series. Playing in Shared mode until it lets go.',
    );
    expect(errorNotice('exclusive_not_allowed', null, null)?.text).toContain('Allow applications to take exclusive control');
    expect(errorNotice('device_lost', 'FiiO Q series', null)).toEqual({
      text: "The FiiO Q series was disconnected. Paused. Playback picks up here if it's back within 30 seconds.",
      tone: 'danger',
    });
    expect(errorNotice('network', null, 'Lucretia')?.text).toBe(
      'Can\'t reach the server. "Lucretia" stopped. Check the connection and press Play.',
    );
    expect(errorNotice('decode', null, null)).toBeNull();
    expect(deviceGoneNotice('FiiO Q series')).toBe(
      "The FiiO Q series didn't come back. Playback stopped. Press Play to carry on with another device.",
    );
  });

  it('describes device rates', () => {
    expect(deviceRateShort({ maxRate: 384_000 })).toBe('up to 384 kHz');
    expect(deviceRateShort({ maxRate: 48_000, singleRate: true })).toBe('48 kHz only');
    expect(deviceRateShort({})).toBe('');
    expect(deviceRateLong({ maxRate: 384_000, maxBits: 32 })).toBe('Up to 384 kHz · 32-bit');
    expect(deviceRateLong({ maxRate: 48_000, maxBits: 16, singleRate: true })).toBe('48 kHz · 16-bit only');
    expect(deviceRateLong({ maxRate: 48_000 })).toBe('');
  });
});
