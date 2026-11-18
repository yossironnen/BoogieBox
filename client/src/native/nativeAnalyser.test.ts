import { describe, expect, it } from 'vitest';
import { isNativeAnalyser, NativeAnalyser } from './nativeAnalyser';

describe('NativeAnalyser', () => {
  it('stretches the engine waveform over the meter buffer', () => {
    const a = new NativeAnalyser();
    expect(isNativeAnalyser(a)).toBe(true);
    expect(isNativeAnalyser({})).toBe(false);
    a.push([127, -127, 0, 64]);
    const out = new Uint8Array(8);
    a.getByteTimeDomainData(out);
    expect(Array.from(out)).toEqual([255, 255, 1, 1, 128, 128, 192, 192]);
    a.clear();
    a.getByteTimeDomainData(out);
    expect(out.every((v) => v === 128)).toBe(true);
    a.fftSize = 256;
    expect(a.frequencyBinCount).toBe(128);
  });

  it('finds a tone in the spectrum', () => {
    const a = new NativeAnalyser();
    a.smoothingTimeConstant = 0;
    // 3 kHz at the waveform's 24 kHz: a quarter of the way to Nyquist.
    a.push(Array.from({ length: 512 }, (_, i) => Math.round(100 * Math.sin((2 * Math.PI * 3_000 * i) / 24_000))));
    const bins = new Uint8Array(64);
    a.getByteFrequencyData(bins);
    const loudest = bins.indexOf(Math.max(...bins));
    expect(loudest).toBeGreaterThanOrEqual(15);
    expect(loudest).toBeLessThanOrEqual(17);
    expect(bins[50]).toBeLessThan(bins[loudest]);
  });
});
