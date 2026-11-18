//! Device sample formats and the conversions between decoded samples and device bytes.

use crate::dsp::Dither;
use crate::types::Format;

/// How the device stores one sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleKind {
    /// 32-bit float.
    F32,
    /// 16-bit integer.
    I16,
    /// 24-bit integer, 3 bytes.
    I24Packed,
    /// 24 valid bits in a 32-bit container.
    I24In32,
    /// 32-bit integer.
    I32,
}

impl SampleKind {
    /// Bytes per sample.
    pub fn bytes(self) -> usize {
        match self {
            SampleKind::I16 => 2,
            SampleKind::I24Packed => 3,
            SampleKind::F32 | SampleKind::I24In32 | SampleKind::I32 => 4,
        }
    }

    /// Valid bits.
    pub fn bits(self) -> u16 {
        match self {
            SampleKind::I16 => 16,
            SampleKind::I24Packed | SampleKind::I24In32 => 24,
            SampleKind::F32 | SampleKind::I32 => 32,
        }
    }

    /// Short name for the UI/logs.
    pub fn name(self) -> &'static str {
        match self {
            SampleKind::F32 => "f32",
            SampleKind::I16 => "s16",
            SampleKind::I24Packed => "s24",
            SampleKind::I24In32 => "s24in32",
            SampleKind::I32 => "s32",
        }
    }

    /// Integer formats can carry samples bit for bit.
    pub fn is_int(self) -> bool {
        self != SampleKind::F32
    }
}

/// What the device was opened with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceFormat {
    /// Sample rate (Hz).
    pub sample_rate: u32,
    /// Channels.
    pub channels: u16,
    /// Sample storage.
    pub kind: SampleKind,
}

impl DeviceFormat {
    /// Bytes per frame.
    pub fn frame_bytes(&self) -> usize {
        usize::from(self.channels) * self.kind.bytes()
    }

    /// As an event [`Format`].
    pub fn to_format(&self) -> Format {
        Format {
            sample_rate: self.sample_rate,
            bits: Some(self.kind.bits()),
            channels: self.channels,
            codec: self.kind.name().to_owned(),
        }
    }
}

const I32_SCALE: f32 = 2_147_483_648.0;

/// A left-justified `i32` sample as float in -1..1.
pub fn i32_to_f32(s: i32) -> f32 {
    s as f32 / I32_SCALE
}

/// Remaps interleaved frames from `from` channels to `to`; returns true when channels were mixed down.
/// Mono goes to the first two outputs; 5.1/7.1 fold centre and surrounds into the front pair.
pub fn map_channels(input: &[f32], from: u16, to: u16, out: &mut Vec<f32>) -> bool {
    let (from, to) = (usize::from(from.max(1)), usize::from(to.max(1)));
    out.clear();
    if from == to {
        out.extend_from_slice(input);
        return false;
    }
    let frames = input.len() / from;
    out.reserve(frames * to);
    const H: f32 = std::f32::consts::FRAC_1_SQRT_2;
    for f in input.chunks_exact(from) {
        if from > 2 && to > 2 {
            // Multichannel in and out: keep what fits, silence the rest.
            let keep = from.min(to);
            out.extend_from_slice(&f[..keep]);
            out.extend(std::iter::repeat_n(0.0, to - keep));
            continue;
        }
        let (l, r) = match from {
            1 => (f[0], f[0]),
            2 => (f[0], f[1]),
            // FL FR FC LFE BL BR [SL SR]: centre and surrounds fold into the front pair, LFE is dropped.
            6 | 8 => {
                let (back, surrounds) = if from == 8 {
                    ((f[4] + f[6], f[5] + f[7]), 2.0)
                } else {
                    ((f[4], f[5]), 1.0)
                };
                let norm = 1.0 / (1.0 + H + H * surrounds);
                (
                    (f[0] + H * f[2] + H * back.0) * norm,
                    (f[1] + H * f[2] + H * back.1) * norm,
                )
            }
            _ => (f[0], f[1]),
        };
        if to == 1 {
            out.push((l + r) * 0.5);
        } else {
            out.push(l);
            out.push(r);
            out.extend(std::iter::repeat_n(0.0, to - 2));
        }
    }
    to < from
}

/// Float samples to device bytes (clamped).
pub fn encode_f32(samples: &[f32], kind: SampleKind, out: &mut Vec<u8>) {
    out.reserve(samples.len() * kind.bytes());
    for s in samples {
        let s = s.clamp(-1.0, 1.0);
        match kind {
            SampleKind::F32 => out.extend_from_slice(&s.to_le_bytes()),
            _ => {
                let v = (f64::from(s) * 2_147_483_648.0).clamp(-2_147_483_648.0, 2_147_483_647.0)
                    as i32;
                push_int(v, kind, out);
            }
        }
    }
}

/// Left-justified integer samples to an integer device format, bit for bit (the top bytes).
pub fn encode_i32_exact(samples: &[i32], kind: SampleKind, out: &mut Vec<u8>) {
    out.reserve(samples.len() * kind.bytes());
    for s in samples {
        push_int(*s, kind, out);
    }
}

fn push_int(v: i32, kind: SampleKind, out: &mut Vec<u8>) {
    let le = v.to_le_bytes();
    match kind {
        SampleKind::I16 => out.extend_from_slice(&le[2..4]),
        SampleKind::I24Packed => out.extend_from_slice(&le[1..4]),
        // 24 valid bits, left-justified in 32: the low byte is zero.
        SampleKind::I24In32 => out.extend_from_slice(&[0, le[1], le[2], le[3]]),
        SampleKind::I32 => out.extend_from_slice(&le),
        SampleKind::F32 => out.extend_from_slice(&i32_to_f32(v).to_le_bytes()),
    }
}

/// One device sample as float in -1..1.
pub fn read_sample(b: &[u8], kind: SampleKind) -> f32 {
    match kind {
        SampleKind::F32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        SampleKind::I16 => f32::from(i16::from_le_bytes([b[0], b[1]])) / 32_768.0,
        SampleKind::I24Packed => i32_to_f32(i32::from_le_bytes([0, b[0], b[1], b[2]])),
        SampleKind::I24In32 | SampleKind::I32 => {
            i32_to_f32(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        }
    }
}

/// Writes one float sample (-1..1, clamped) into `out` (one sample's bytes) without dither: exact for
/// values on the device's grid.
pub fn write_float(sample: f32, kind: SampleKind, out: &mut [u8]) {
    let s = sample.clamp(-1.0, 1.0);
    if kind == SampleKind::F32 {
        out.copy_from_slice(&s.to_le_bytes());
        return;
    }
    let v = (f64::from(s) * 2_147_483_648.0).clamp(-2_147_483_648.0, 2_147_483_647.0) as i32;
    write_int(v, kind, out);
}

/// Writes one changed sample with TPDF dither at the device's bit depth (floats need none).
pub fn write_dithered(sample: f32, kind: SampleKind, dither: &mut Dither, out: &mut [u8]) {
    if kind == SampleKind::F32 {
        write_float(sample, kind, out);
        return;
    }
    write_int(dither.quantize(sample, kind.bits()), kind, out);
}

fn write_int(v: i32, kind: SampleKind, out: &mut [u8]) {
    let le = v.to_le_bytes();
    match kind {
        SampleKind::I16 => out.copy_from_slice(&le[2..4]),
        SampleKind::I24Packed => out.copy_from_slice(&le[1..4]),
        SampleKind::I24In32 => out.copy_from_slice(&[0, le[1], le[2], le[3]]),
        SampleKind::I32 => out.copy_from_slice(&le),
        SampleKind::F32 => out.copy_from_slice(&i32_to_f32(v).to_le_bytes()),
    }
}

/// Scales device bytes in place, ramping linearly from `from` to `to` across the buffer (no clicks), with
/// TPDF dither (D7). Unity gain throughout leaves the bytes untouched (bit-perfect).
pub fn apply_volume(
    bytes: &mut [u8],
    format: &DeviceFormat,
    from: f32,
    to: f32,
    dither: &mut Dither,
) {
    if from == 1.0 && to == 1.0 {
        return;
    }
    let fb = format.frame_bytes();
    let sb = format.kind.bytes();
    let frames = bytes.len() / fb.max(1);
    for (i, frame) in bytes.chunks_exact_mut(fb).enumerate() {
        let g = if frames > 1 {
            from + (to - from) * (i as f32 / (frames - 1) as f32)
        } else {
            to
        };
        for sample in frame.chunks_exact_mut(sb) {
            let v = read_sample(sample, format.kind) * g;
            write_dithered(v, format.kind, dither, sample);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_encoding_keeps_the_top_bits() {
        let s = [0x1234_5600_i32, -0x0100_0000];
        let mut out = Vec::new();
        encode_i32_exact(&s, SampleKind::I24In32, &mut out);
        assert_eq!(&out[..4], &[0x00, 0x56, 0x34, 0x12]);
        out.clear();
        encode_i32_exact(&s, SampleKind::I24Packed, &mut out);
        assert_eq!(&out[..3], &[0x56, 0x34, 0x12]);
        out.clear();
        encode_i32_exact(&s, SampleKind::I16, &mut out);
        assert_eq!(&out[..2], &[0x34, 0x12]);
        out.clear();
        encode_i32_exact(&s[..1], SampleKind::I32, &mut out);
        assert_eq!(out, 0x1234_5600_i32.to_le_bytes());
        out.clear();
        encode_i32_exact(&[1 << 30], SampleKind::F32, &mut out);
        assert_eq!(f32::from_le_bytes([out[0], out[1], out[2], out[3]]), 0.5);
    }

    #[test]
    fn float_encoding_clamps_and_round_trips() {
        let mut out = Vec::new();
        encode_f32(&[0.5, 2.0, -2.0], SampleKind::I16, &mut out);
        assert_eq!(i16::from_le_bytes([out[0], out[1]]), 16_384);
        assert_eq!(i16::from_le_bytes([out[2], out[3]]), i16::MAX);
        assert_eq!(i16::from_le_bytes([out[4], out[5]]), i16::MIN);
        for kind in [
            SampleKind::F32,
            SampleKind::I16,
            SampleKind::I24Packed,
            SampleKind::I24In32,
            SampleKind::I32,
        ] {
            let mut b = Vec::new();
            encode_f32(&[0.25], kind, &mut b);
            assert!((read_sample(&b, kind) - 0.25).abs() < 1e-4, "{kind:?}");
            assert_eq!(b.len(), kind.bytes());
        }
    }

    #[test]
    fn channels_fold_and_spread() {
        let mut out = Vec::new();
        assert!(!map_channels(&[0.1, 0.2], 2, 2, &mut out));
        assert_eq!(out, [0.1, 0.2]);
        assert!(!map_channels(&[0.5], 1, 2, &mut out));
        assert_eq!(out, [0.5, 0.5]);
        assert!(!map_channels(&[0.5, -0.5], 2, 6, &mut out));
        assert_eq!(out, [0.5, -0.5, 0.0, 0.0, 0.0, 0.0]);
        assert!(map_channels(&[0.4, 0.2], 2, 1, &mut out));
        assert_eq!(out, [0.3_f32]);
        // 5.1: centre only lands equally on both sides.
        assert!(map_channels(
            &[0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            6,
            2,
            &mut out
        ));
        assert!((out[0] - out[1]).abs() < 1e-6 && out[0] > 0.0);
        assert!(map_channels(&[1.0; 8], 8, 2, &mut out));
        assert!(out.iter().all(|v| *v <= 1.0 + 1e-6));
        assert!(map_channels(&[1.0; 3], 3, 2, &mut out));
        assert!(!map_channels(&[1.0; 6], 6, 8, &mut out));
        assert_eq!(out.len(), 8);
    }

    #[test]
    fn single_samples_write_in_place() {
        let mut b = [0u8; 4];
        write_float(0.5, SampleKind::I24In32, &mut b);
        assert_eq!(b, [0, 0, 0, 0x40]);
        write_float(0.25, SampleKind::F32, &mut b);
        assert_eq!(f32::from_le_bytes(b), 0.25);
        let mut d = Dither::default();
        let mut two = [0u8; 2];
        write_dithered(0.5, SampleKind::I16, &mut d, &mut two);
        assert!((i32::from(i16::from_le_bytes(two)) - 16_384).abs() <= 1);
        write_dithered(-0.5, SampleKind::F32, &mut d, &mut b);
        assert_eq!(f32::from_le_bytes(b), -0.5);
        let mut three = [0u8; 3];
        write_float(-1.0, SampleKind::I24Packed, &mut three);
        assert_eq!(three, [0, 0, 0x80]);
    }

    #[test]
    fn volume_ramps_and_unity_is_untouched() {
        let fmt = DeviceFormat {
            sample_rate: 48_000,
            channels: 1,
            kind: SampleKind::I16,
        };
        let mut b = Vec::new();
        encode_f32(&[0.5; 5], SampleKind::I16, &mut b);
        let orig = b.clone();
        let mut d = Dither::default();
        apply_volume(&mut b, &fmt, 1.0, 1.0, &mut d);
        assert_eq!(b, orig);
        apply_volume(&mut b, &fmt, 1.0, 0.0, &mut d);
        let first = i16::from_le_bytes([b[0], b[1]]);
        let last = i16::from_le_bytes([b[8], b[9]]);
        assert!(
            (i32::from(first) - 16_384).abs() <= 1,
            "dithered by at most one step"
        );
        assert!(last.abs() <= 1);
        assert_eq!(fmt.frame_bytes(), 2);
        assert_eq!(fmt.to_format().codec, "s16");
    }
}
