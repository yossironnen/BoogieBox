//! Streaming sample-rate conversion (rubato sinc) for the shared-mode path and the D9 fallback.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};

const CHUNK: usize = 1024;

/// Converts an interleaved float stream from one rate to another, chunk by chunk.
pub struct StreamResampler {
    inner: Async<f32>,
    channels: usize,
    pending: Vec<f32>,
    scratch: Vec<f32>,
    /// Output frames still to drop: the filter's delay at the start of the stream.
    delay_left: usize,
    /// Input frames taken and output frames handed on (after the delay), for an exact length at the end.
    in_total: u64,
    out_total: u64,
    from: u32,
    to: u32,
}

impl std::fmt::Debug for StreamResampler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamResampler")
            .field("from", &self.from)
            .field("to", &self.to)
            .finish()
    }
}

impl StreamResampler {
    /// A resampler from `from` Hz to `to` Hz for `channels` interleaved channels.
    pub fn new(from: u32, to: u32, channels: u16) -> Result<Self, String> {
        let params = SincInterpolationParameters::new(128, WindowFunction::Blackman2)
            .oversampling_factor(256)
            .interpolation(SincInterpolationType::Cubic);
        let channels = usize::from(channels.max(1));
        let inner = Async::<f32>::new_sinc(
            f64::from(to) / f64::from(from.max(1)),
            1.1,
            &params,
            CHUNK,
            channels,
            FixedAsync::Input,
        )
        .map_err(|e| e.to_string())?;
        let delay_left = inner.output_delay();
        Ok(Self {
            scratch: vec![0.0; inner.output_frames_max() * channels],
            inner,
            channels,
            pending: Vec::new(),
            delay_left,
            in_total: 0,
            out_total: 0,
            from,
            to,
        })
    }

    /// Input rate.
    pub fn from_rate(&self) -> u32 {
        self.from
    }

    /// Appends converted frames for `input` (interleaved) to `out`; keeps a partial chunk for later.
    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) -> Result<(), String> {
        self.in_total += (input.len() / self.channels) as u64;
        self.pending.extend_from_slice(input);
        while self.pending.len() / self.channels >= self.inner.input_frames_next() {
            let need = self.inner.input_frames_next();
            self.run(need, None, out)?;
        }
        Ok(())
    }

    /// Converts what is left and the filter's tail (end of stream, or before a rate change), so the whole
    /// stream comes out at exactly `input frames x ratio`.
    pub fn flush(&mut self, out: &mut Vec<f32>) -> Result<(), String> {
        let target = (self.in_total as f64 * f64::from(self.to) / f64::from(self.from.max(1)))
            .round() as u64;
        let start = out.len();
        let left = self.pending.len() / self.channels;
        self.run(left, Some(left), out)?;
        // Push silence through until the delayed frames are out.
        let mut guard = 0;
        while self.out_total < target && guard < 64 {
            self.run(0, Some(0), out)?;
            guard += 1;
        }
        let extra = self.out_total.saturating_sub(target) as usize;
        let keep = out.len().saturating_sub(extra * self.channels).max(start);
        out.truncate(keep);
        self.out_total = self.out_total.min(target);
        Ok(())
    }

    fn run(
        &mut self,
        frames: usize,
        partial: Option<usize>,
        out: &mut Vec<f32>,
    ) -> Result<(), String> {
        let channels = self.channels;
        let in_frames = self.pending.len() / channels;
        let input =
            InterleavedSlice::new(&self.pending[..in_frames * channels], channels, in_frames)
                .map_err(|e| e.to_string())?;
        let out_frames = self.scratch.len() / channels;
        let mut output = InterleavedSlice::new_mut(&mut self.scratch, channels, out_frames)
            .map_err(|e| e.to_string())?;
        let mut indexing = Indexing::new();
        indexing.partial_len = partial;
        let (used, made) = self
            .inner
            .process_into_buffer(&input, &mut output, Some(&indexing))
            .map_err(|e| e.to_string())?;
        let used = if partial.is_some() {
            frames.min(in_frames)
        } else {
            used
        };
        self.pending.drain(..used * channels);
        let drop = self.delay_left.min(made);
        self.delay_left -= drop;
        out.extend_from_slice(&self.scratch[drop * channels..made * channels]);
        self.out_total += (made - drop) as u64;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_keeps_its_length_and_pitch() {
        let (from, to) = (44_100u32, 48_000u32);
        let freq = 1_000.0f32;
        let input: Vec<f32> = (0..from as usize)
            .flat_map(|i| {
                let v = (2.0 * std::f32::consts::PI * freq * i as f32 / from as f32).sin() * 0.5;
                [v, v]
            })
            .collect();
        let mut r = StreamResampler::new(from, to, 2).unwrap();
        assert_eq!(r.from_rate(), from);
        let mut out = Vec::new();
        for chunk in input.chunks(2 * 333) {
            r.push(chunk, &mut out).unwrap();
        }
        r.flush(&mut out).unwrap();
        let frames = out.len() / 2;
        assert!(
            (frames as i64 - to as i64).abs() <= 2,
            "one second in, one second out: {frames}"
        );
        // Compare a stretch in the middle with the ideal sine at the new rate (delay already removed).
        let max_err = (10_000..11_000)
            .map(|i| {
                let ideal = (2.0 * std::f32::consts::PI * freq * i as f32 / to as f32).sin() * 0.5;
                (out[2 * i] - ideal).abs()
            })
            .fold(0.0f32, f32::max);
        // The filter delay is a whole number of frames plus a fraction; half a sample of a 1 kHz tone at
        // 48 kHz and amplitude 0.5 is ~0.033.
        assert!(max_err < 0.033, "aligned within half a sample: {max_err}");
    }
}
