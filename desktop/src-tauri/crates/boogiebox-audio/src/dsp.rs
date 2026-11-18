//! Sound processing for the native engine (`wip/native-hires-engine-plan.md` §10): the web player's EQ as
//! biquad filters, levelling gain, the crossfade curve, TPDF dither and the VU meter's levels.
//!
//! The filters use the Web Audio `BiquadFilterNode` formulas (Audio EQ Cookbook), so a curve sounds the same on
//! "This PC" as in the browser: peaking with `Q`, shelves with slope 1 (Q unused), high/low-pass with
//! `Q` in dB. Everything here runs on the output thread, so nothing allocates.

use serde::{Deserialize, Serialize};

/// Most bands a curve can have (graphic EQ: 10, parametric: 7).
pub const MAX_BANDS: usize = 10;
/// Most channels the EQ filters (more are passed through unchanged).
pub const MAX_CHANNELS: usize = 8;

/// A filter type, as the web player names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BandKind {
    /// Bell around the frequency.
    Peaking,
    /// Lifts or cuts below the frequency.
    LowShelf,
    /// Lifts or cuts above the frequency.
    HighShelf,
    /// Cuts below the frequency.
    Highpass,
    /// Cuts above the frequency.
    Lowpass,
}

/// One EQ band (the web player's `ParametricEqBand`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqBand {
    /// Filter type.
    #[serde(rename = "type")]
    pub kind: BandKind,
    /// Centre or corner frequency (Hz).
    pub frequency_hz: f64,
    /// Gain (dB); unused by high/low-pass.
    #[serde(default)]
    pub gain_db: f64,
    /// Width (peaking) or resonance in dB (high/low-pass).
    pub q: f64,
    /// A disabled band does nothing.
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// Normalised biquad coefficients (`a0` = 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coeffs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Coeffs {
    const IDENTITY: Coeffs = Coeffs {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    /// The filter for `band` at `rate`; `None` when it leaves the sound unchanged (disabled, 0 dB bell or
    /// shelf) or can't be built (a frequency at or above Nyquist).
    pub fn for_band(band: &EqBand, rate: u32) -> Option<Coeffs> {
        let rate = f64::from(rate);
        let usable = band.frequency_hz > 0.0 && band.frequency_hz < rate / 2.0;
        if !band.enabled || !usable {
            return None;
        }
        let gain = band.gain_db.clamp(-40.0, 40.0);
        let gain_type = matches!(
            band.kind,
            BandKind::Peaking | BandKind::LowShelf | BandKind::HighShelf
        );
        if gain_type && gain.abs() < 1e-9 {
            return None;
        }
        let w0 = std::f64::consts::TAU * band.frequency_hz / rate;
        let (sin, cos) = w0.sin_cos();
        let a = 10f64.powf(gain / 40.0);
        let (b0, b1, b2, a0, a1, a2) = match band.kind {
            BandKind::Peaking => {
                let alpha = sin / (2.0 * band.q.max(1e-4));
                (
                    1.0 + alpha * a,
                    -2.0 * cos,
                    1.0 - alpha * a,
                    1.0 + alpha / a,
                    -2.0 * cos,
                    1.0 - alpha / a,
                )
            }
            BandKind::LowShelf | BandKind::HighShelf => {
                // Slope S = 1: alpha = sin(w0)/2 · √2.
                let k = 2.0 * a.sqrt() * (sin / 2.0) * std::f64::consts::SQRT_2;
                let (ap, am) = (a + 1.0, a - 1.0);
                if band.kind == BandKind::LowShelf {
                    (
                        a * (ap - am * cos + k),
                        2.0 * a * (am - ap * cos),
                        a * (ap - am * cos - k),
                        ap + am * cos + k,
                        -2.0 * (am + ap * cos),
                        ap + am * cos - k,
                    )
                } else {
                    (
                        a * (ap + am * cos + k),
                        -2.0 * a * (am + ap * cos),
                        a * (ap + am * cos - k),
                        ap - am * cos + k,
                        2.0 * (am - ap * cos),
                        ap - am * cos - k,
                    )
                }
            }
            BandKind::Highpass | BandKind::Lowpass => {
                // Web Audio reads Q in dB for these two.
                let alpha = sin / (2.0 * 10f64.powf(band.q / 20.0));
                let (b0, b1) = if band.kind == BandKind::Highpass {
                    ((1.0 + cos) / 2.0, -(1.0 + cos))
                } else {
                    ((1.0 - cos) / 2.0, 1.0 - cos)
                };
                (b0, b1, b0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
            }
        };
        Some(Coeffs {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        })
    }

    /// Magnitude response (dB) at `hz` for a filter running at `rate`.
    pub fn response_db(&self, hz: f64, rate: u32) -> f64 {
        let w = std::f64::consts::TAU * hz / f64::from(rate);
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let num_re = self.b0 + self.b1 * c1 + self.b2 * c2;
        let num_im = -(self.b1 * s1 + self.b2 * s2);
        let den_re = 1.0 + self.a1 * c1 + self.a2 * c2;
        let den_im = -(self.a1 * s1 + self.a2 * s2);
        let num = num_re.hypot(num_im);
        let den = den_re.hypot(den_im);
        20.0 * (num / den).log10()
    }
}

/// A curve's filters at one sample rate. Plain data, so it can be copied to the output thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EqPlan {
    coeffs: [Coeffs; MAX_BANDS],
    len: usize,
}

impl Default for EqPlan {
    fn default() -> Self {
        Self {
            coeffs: [Coeffs::IDENTITY; MAX_BANDS],
            len: 0,
        }
    }
}

impl EqPlan {
    /// The filters for `bands` at `rate` (bands that change nothing are left out).
    pub fn new(bands: &[EqBand], rate: u32) -> Self {
        let mut plan = Self::default();
        for c in bands.iter().filter_map(|b| Coeffs::for_band(b, rate)) {
            if plan.len == MAX_BANDS {
                break;
            }
            plan.coeffs[plan.len] = c;
            plan.len += 1;
        }
        plan
    }

    /// No filters: the curve leaves the sound unchanged.
    pub fn is_identity(&self) -> bool {
        self.len == 0
    }

    /// The whole curve's response (dB) at `hz`.
    pub fn response_db(&self, hz: f64, rate: u32) -> f64 {
        self.coeffs[..self.len]
            .iter()
            .map(|c| c.response_db(hz, rate))
            .sum()
    }
}

/// True when the curve changes the sound at `rate`.
pub fn eq_active(bands: &[EqBand], rate: u32) -> bool {
    !EqPlan::new(bands, rate).is_identity()
}

/// Running EQ: a plan and each channel's filter memory (transposed direct form II).
#[derive(Debug, Clone, Copy)]
pub struct Equalizer {
    plan: EqPlan,
    state: [[[f64; 2]; MAX_BANDS]; MAX_CHANNELS],
}

impl Default for Equalizer {
    fn default() -> Self {
        Self {
            plan: EqPlan::default(),
            state: [[[0.0; 2]; MAX_BANDS]; MAX_CHANNELS],
        }
    }
}

impl Equalizer {
    /// The plan in use.
    pub fn plan(&self) -> &EqPlan {
        &self.plan
    }

    /// Switches to `plan`, keeping filter memory where a band carries on (no click for small changes).
    pub fn set_plan(&mut self, plan: EqPlan) {
        if plan.len != self.plan.len {
            self.reset();
        }
        self.plan = plan;
    }

    /// Forgets filter memory (a new stream).
    pub fn reset(&mut self) {
        self.state = [[[0.0; 2]; MAX_BANDS]; MAX_CHANNELS];
    }

    /// Filters interleaved `frames` of `channels` in place.
    pub fn process(&mut self, frames: &mut [f32], channels: usize) {
        if self.plan.len == 0 || channels == 0 {
            return;
        }
        let filtered = channels.min(MAX_CHANNELS);
        for frame in frames.chunks_exact_mut(channels) {
            for (ch, sample) in frame.iter_mut().take(filtered).enumerate() {
                let mut x = f64::from(*sample);
                for (c, z) in self.plan.coeffs[..self.plan.len]
                    .iter()
                    .zip(self.state[ch].iter_mut())
                {
                    let y = c.b0 * x + z[0];
                    z[0] = c.b1 * x - c.a1 * y + z[1];
                    z[1] = c.b2 * x - c.a2 * y;
                    x = y;
                }
                *sample = x as f32;
            }
        }
    }
}

/// Linear gain for `db`.
pub fn db_to_linear(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// Equal-power crossfade gains (outgoing, incoming) at `progress` 0..=1: the sum of powers stays 1, so the
/// join doesn't dip in loudness.
pub fn crossfade_gains(progress: f64) -> (f32, f32) {
    let p = progress.clamp(0.0, 1.0) * std::f64::consts::FRAC_PI_2;
    (p.cos() as f32, p.sin() as f32)
}

/// Triangular (TPDF) dither for the last quantisation step: noise of ±1 LSB at the device's bit depth, so
/// low-level detail turns into a little hiss instead of distortion. Only used when samples were changed.
#[derive(Debug, Clone)]
pub struct Dither {
    state: u64,
}

impl Default for Dither {
    fn default() -> Self {
        Self {
            state: 0x9E37_79B9_7F4A_7C15,
        }
    }
}

impl Dither {
    fn next_unit(&mut self) -> f64 {
        // xorshift64*: fast, good enough for noise.
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        let v = self.state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (v >> 11) as f64 / (1u64 << 53) as f64
    }

    /// `sample` (-1..1) as a left-justified `i32` at `bits` valid bits, with TPDF noise of ±1 LSB.
    pub fn quantize(&mut self, sample: f32, bits: u16) -> i32 {
        let bits = bits.clamp(8, 32);
        let lsb = 2f64.powi(32 - i32::from(bits));
        let noise = (self.next_unit() - self.next_unit()) * lsb;
        let v = f64::from(sample) * 2_147_483_648.0 + noise;
        let q = (v / lsb).round() * lsb;
        q.clamp(-2_147_483_648.0, 2_147_483_648.0 - lsb) as i32
    }
}

/// Peak and RMS of what plays, plus a short waveform for the dock's VU meter.
#[derive(Debug, Clone)]
pub struct LevelMeter {
    peak: f32,
    sum_sq: f64,
    count: u64,
    wave: Vec<f32>,
    write: usize,
    step: usize,
    phase: usize,
    acc: f32,
}

/// Points in the meter's waveform (about 85 ms at ~24 kHz).
pub const WAVE_POINTS: usize = 2048;
/// The waveform's sample rate, roughly (the device rate divided by a whole number).
pub const WAVE_RATE: u32 = 24_000;

impl LevelMeter {
    /// A meter for a device at `rate`.
    pub fn new(rate: u32) -> Self {
        Self {
            peak: 0.0,
            sum_sq: 0.0,
            count: 0,
            wave: vec![0.0; WAVE_POINTS],
            write: 0,
            step: (rate / WAVE_RATE).max(1) as usize,
            phase: 0,
            acc: 0.0,
        }
    }

    /// Adds interleaved frames of `channels`.
    pub fn add(&mut self, frames: &[f32], channels: usize) {
        if channels == 0 {
            return;
        }
        for frame in frames.chunks_exact(channels) {
            let mono = frame.iter().take(2).sum::<f32>() / channels.min(2) as f32;
            for s in frame {
                self.peak = self.peak.max(s.abs());
            }
            self.sum_sq += f64::from(mono * mono);
            self.count += 1;
            self.acc += mono;
            self.phase += 1;
            if self.phase == self.step {
                self.wave[self.write] = self.acc / self.step as f32;
                self.write = (self.write + 1) % WAVE_POINTS;
                self.phase = 0;
                self.acc = 0.0;
            }
        }
    }

    /// Peak and RMS since the last call, and the latest `points` waveform samples (oldest first).
    pub fn take(&mut self, points: usize, wave: &mut Vec<f32>) -> (f32, f32) {
        let rms = if self.count > 0 {
            (self.sum_sq / self.count as f64).sqrt() as f32
        } else {
            0.0
        };
        let peak = self.peak;
        self.peak = 0.0;
        self.sum_sq = 0.0;
        self.count = 0;
        let points = points.min(WAVE_POINTS);
        wave.clear();
        let start = (self.write + WAVE_POINTS - points) % WAVE_POINTS;
        wave.extend((0..points).map(|i| self.wave[(start + i) % WAVE_POINTS]));
        (peak, rms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band(kind: BandKind, hz: f64, gain: f64, q: f64) -> EqBand {
        EqBand {
            kind,
            frequency_hz: hz,
            gain_db: gain,
            q,
            enabled: true,
        }
    }

    /// Runs a sine through the EQ and measures its level change (dB) once settled.
    fn measured_gain(eq: &mut Equalizer, hz: f64, rate: u32) -> f64 {
        let n = rate as usize;
        let mut buf: Vec<f32> = (0..n)
            .map(|i| (std::f64::consts::TAU * hz * i as f64 / f64::from(rate)).sin() as f32 * 0.25)
            .collect();
        eq.process(&mut buf, 1);
        let tail = &buf[n / 2..];
        let rms =
            (tail.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / tail.len() as f64).sqrt();
        20.0 * (rms / (0.25 / std::f64::consts::SQRT_2)).log10()
    }

    #[test]
    fn bells_and_shelves_hit_their_gain() {
        let rate = 96_000;
        let bell = Coeffs::for_band(&band(BandKind::Peaking, 1_000.0, 6.0, 1.1), rate).unwrap();
        assert!((bell.response_db(1_000.0, rate) - 6.0).abs() < 1e-9);
        assert!(bell.response_db(20.0, rate).abs() < 0.05);
        let low = Coeffs::for_band(&band(BandKind::LowShelf, 100.0, -4.0, 1.0), rate).unwrap();
        assert!((low.response_db(10.0, rate) + 4.0).abs() < 0.05);
        assert!(low.response_db(10_000.0, rate).abs() < 0.05);
        let high = Coeffs::for_band(&band(BandKind::HighShelf, 8_000.0, 3.0, 1.0), rate).unwrap();
        assert!((high.response_db(30_000.0, rate) - 3.0).abs() < 0.1);
        // Slope 1 shelves are half their gain at the corner.
        assert!((high.response_db(8_000.0, rate) - 1.5).abs() < 0.01);
    }

    #[test]
    fn pass_filters_read_q_in_db_like_web_audio() {
        let rate = 48_000;
        // The level at the corner is Q itself: -3.01 dB is the Butterworth corner, 0 dB a slight bump.
        let hp = Coeffs::for_band(&band(BandKind::Highpass, 80.0, 0.0, -3.0103), rate).unwrap();
        assert!((hp.response_db(80.0, rate) + 3.01).abs() < 0.02);
        let flat_q = Coeffs::for_band(&band(BandKind::Highpass, 80.0, 0.0, 0.0), rate).unwrap();
        assert!(flat_q.response_db(80.0, rate).abs() < 0.02);
        assert!(hp.response_db(20.0, rate) < -20.0);
        // Q = 6 dB: a 6 dB resonance at the corner.
        let lp = Coeffs::for_band(&band(BandKind::Lowpass, 10_000.0, 0.0, 6.0), rate).unwrap();
        assert!((lp.response_db(10_000.0, rate) - 6.0).abs() < 0.05);
    }

    #[test]
    fn flat_or_impossible_bands_are_left_out() {
        let rate = 44_100;
        let mut off = band(BandKind::Peaking, 1_000.0, 6.0, 1.0);
        off.enabled = false;
        assert!(Coeffs::for_band(&off, rate).is_none());
        assert!(Coeffs::for_band(&band(BandKind::Peaking, 1_000.0, 0.0, 1.0), rate).is_none());
        assert!(Coeffs::for_band(&band(BandKind::Peaking, 30_000.0, 3.0, 1.0), rate).is_none());
        assert!(Coeffs::for_band(&band(BandKind::Highpass, 0.0, 0.0, 0.0), rate).is_none());
        assert!(!eq_active(&[off], rate));
        assert!(eq_active(
            &[band(BandKind::Lowpass, 5_000.0, 0.0, 0.0)],
            rate
        ));
        let many = [band(BandKind::Peaking, 500.0, 1.0, 1.0); 12];
        assert_eq!(EqPlan::new(&many, rate).len, MAX_BANDS);
    }

    #[test]
    fn the_equalizer_shapes_a_sine_like_its_response() {
        // The graphic EQ's "Rock" curve (10 bells at Q 1.1).
        let freqs = [31.0, 62.0, 125.0, 250.0, 500.0, 1e3, 2e3, 4e3, 8e3, 16e3];
        let gains = [4.0, 3.0, 2.0, 0.0, -1.0, 0.0, 2.0, 3.0, 4.0, 3.0];
        let bands: Vec<EqBand> = freqs
            .iter()
            .zip(gains)
            .map(|(f, g)| band(BandKind::Peaking, *f, g, 1.1))
            .collect();
        for rate in [44_100, 96_000, 192_000] {
            let plan = EqPlan::new(&bands, rate);
            for hz in [62.0, 440.0, 3_000.0, 8_000.0] {
                let mut eq = Equalizer::default();
                eq.set_plan(plan);
                let got = measured_gain(&mut eq, hz, rate);
                let want = plan.response_db(hz, rate);
                assert!((got - want).abs() < 0.1, "{rate} Hz, {hz}: {got} vs {want}");
            }
        }
        let mut flat = Equalizer::default();
        let mut buf = [0.5f32, -0.25];
        flat.process(&mut buf, 2);
        assert_eq!(buf, [0.5, -0.25], "no plan: untouched");
    }

    #[test]
    fn crossfade_keeps_power_and_dither_stays_within_one_step() {
        for p in [0.0, 0.3, 0.5, 1.0] {
            let (o, i) = crossfade_gains(p);
            assert!((o * o + i * i - 1.0).abs() < 1e-6);
        }
        assert_eq!(crossfade_gains(0.0), (1.0, 0.0));
        assert!((db_to_linear(-6.0) - 0.501).abs() < 1e-3);

        let mut d = Dither::default();
        let lsb16 = 1 << 16;
        let x = 0.123_456_7f32;
        let exact = f64::from(x) * 2_147_483_648.0;
        let mut sum = 0.0;
        for _ in 0..10_000 {
            let q = d.quantize(x, 16);
            assert_eq!(q % lsb16, 0, "on the 16-bit grid");
            assert!((f64::from(q) - exact).abs() <= 2.0 * f64::from(lsb16));
            sum += f64::from(q);
        }
        // Unbiased: the average lands on the exact value.
        assert!((sum / 10_000.0 - exact).abs() < f64::from(lsb16) * 0.05);
        assert_eq!(d.quantize(2.0, 24), i32::MAX - 255, "clamped to full scale");
    }

    #[test]
    fn the_meter_reports_peak_rms_and_a_waveform() {
        let mut m = LevelMeter::new(48_000);
        let frames: Vec<f32> = (0..4_800)
            .flat_map(|i| {
                let s = if i % 2 == 0 { 0.5 } else { -0.5 };
                [s, s]
            })
            .collect();
        m.add(&frames, 2);
        let mut wave = Vec::new();
        let (peak, rms) = m.take(256, &mut wave);
        assert!((peak - 0.5).abs() < 1e-6);
        assert!((rms - 0.5).abs() < 1e-3);
        assert_eq!(wave.len(), 256);
        let (peak, rms) = m.take(8, &mut wave);
        assert_eq!((peak, rms), (0.0, 0.0), "reset after reading");
    }
}
