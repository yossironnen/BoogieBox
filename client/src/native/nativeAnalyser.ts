/**
 * Stands in for a Web Audio `AnalyserNode` while the desktop app's engine plays, so the dock's VU meter and wave
 * visualizer draw the engine's sound. The engine sends ~21 ms of mono sound (~24 kHz) 20 times a second
 * (`levels` events); this keeps the latest and answers the two reads the meters make.
 */

/** The waveform's sample rate (the engine decimates to about this). */
export const NATIVE_WAVE_RATE = 24_000;
const MIN_DB = -100;
const MAX_DB = -30;

export class NativeAnalyser {
  /** Marks the stand-in (the VU meter doesn't wire it into an audio graph). */
  readonly native = true;
  fftSize = 2048;
  smoothingTimeConstant = 0.8;
  private wave = new Float32Array(512);
  private smoothed: Float32Array | null = null;

  get frequencyBinCount(): number {
    return this.fftSize / 2;
  }

  /** The latest waveform from a `levels` event (-127..127). */
  push(wave: readonly number[]): void {
    if (wave.length === 0) return;
    if (this.wave.length !== wave.length) this.wave = new Float32Array(wave.length);
    for (let i = 0; i < wave.length; i += 1) this.wave[i] = Math.max(-1, Math.min(1, wave[i] / 127));
  }

  /** Silence (playback stopped or paused). */
  clear(): void {
    this.wave.fill(0);
  }

  /** The waveform stretched over `out` (128 is silence, as Web Audio does). */
  getByteTimeDomainData(out: Uint8Array): void {
    const n = this.wave.length;
    for (let i = 0; i < out.length; i += 1) {
      const v = this.wave[Math.min(n - 1, Math.floor((i * n) / out.length))];
      out[i] = Math.max(0, Math.min(255, Math.round(128 + v * 127)));
    }
  }

  /** A spectrum of the waveform (0 to ~12 kHz) in `out.length` bins, scaled like Web Audio's -100..-30 dB. */
  getByteFrequencyData(out: Uint8Array): void {
    const n = this.wave.length;
    const bins = out.length;
    if (!this.smoothed || this.smoothed.length !== bins) this.smoothed = new Float32Array(bins);
    for (let k = 0; k < bins; k += 1) {
      // Bin k covers k/bins of the waveform's Nyquist range.
      const freq = (k * n) / (2 * bins);
      let re = 0;
      let im = 0;
      for (let t = 0; t < n; t += 1) {
        // Hann window keeps the bins from smearing into each other.
        const w = 0.5 - 0.5 * Math.cos((2 * Math.PI * t) / (n - 1));
        const a = (2 * Math.PI * freq * t) / n;
        re += this.wave[t] * w * Math.cos(a);
        im -= this.wave[t] * w * Math.sin(a);
      }
      const mag = Math.sqrt(re * re + im * im) / (n / 4);
      const s = this.smoothingTimeConstant;
      this.smoothed[k] = s * this.smoothed[k] + (1 - s) * mag;
      const db = 20 * Math.log10(Math.max(this.smoothed[k], 1e-10));
      out[k] = Math.max(0, Math.min(255, Math.round(((db - MIN_DB) / (MAX_DB - MIN_DB)) * 255)));
    }
  }
}

/** True for the stand-in. */
export function isNativeAnalyser(a: unknown): a is NativeAnalyser {
  return typeof a === 'object' && a !== null && (a as { native?: unknown }).native === true;
}
