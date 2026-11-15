//! Synthetic test audio (WAV files built in memory).

/// A PCM WAV from interleaved left-justified `i32` samples, stored at `bits` (16 or 24).
pub fn wav_bytes(rate: u32, channels: u16, bits: u16, samples: &[i32]) -> Vec<u8> {
    let bytes_per = usize::from(bits / 8);
    let mut data = Vec::with_capacity(samples.len() * bytes_per);
    for s in samples {
        let le = s.to_le_bytes();
        data.extend_from_slice(&le[4 - bytes_per..]);
    }
    let block_align = channels * bits / 8;
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(block_align)).to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

/// Left-justified samples of a ramp: frame `i` is `i % 32768` on the first channel and its negative on
/// the second, so any frame can be recognised in the output.
pub fn ramp(channels: u16, frames: usize, offset: usize) -> Vec<i32> {
    let mut v = Vec::with_capacity(frames * usize::from(channels));
    for i in 0..frames {
        let value = ((i + offset) % 32_768) as i32;
        for c in 0..channels {
            v.push(if c % 2 == 0 {
                value << 16
            } else {
                (-value) << 16
            });
        }
    }
    v
}

/// [`wav_bytes`] of a [`ramp`].
pub fn ramp_wav(rate: u32, channels: u16, bits: u16, frames: usize) -> Vec<u8> {
    wav_bytes(rate, channels, bits, &ramp(channels, frames, 0))
}
