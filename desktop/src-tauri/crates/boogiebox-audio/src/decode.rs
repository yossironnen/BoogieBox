//! Decoding with symphonia: interleaved, left-justified `i32` samples plus the stream's format.
//!
//! Every format symphonia decodes is handed on as `i32` (FLAC/ALAC/WAV/AIFF integers are shifted to the top of
//! the word, which the phase 0 spike measured, so the top bytes are the original samples; lossy float decoders
//! are scaled to the same range). The bit-perfect path takes the top bytes; the DSP path converts to float.

use std::io::{self, Seek, SeekFrom};

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_AAC, CODEC_ID_ALAC, CODEC_ID_FLAC, CODEC_ID_MP3, CODEC_ID_VORBIS,
};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;

use crate::gapless;
use crate::source::SourceError;
use crate::types::ErrorCode;

/// What the decoder knows about a stream.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamInfo {
    /// Sample rate (Hz).
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// Source bit depth (lossless formats).
    pub bits: Option<u16>,
    /// `FLAC`, `ALAC`, `MP3`, `AAC`, `Vorbis`, `PCM`.
    pub codec: String,
    /// Total frames, when the container says.
    pub frames: Option<u64>,
}

impl StreamInfo {
    /// Duration (s), when the frame count is known.
    pub fn duration_sec(&self) -> Option<f64> {
        self.frames
            .filter(|_| self.sample_rate > 0)
            .map(|f| f as f64 / f64::from(self.sample_rate))
    }

    /// True for lossless codecs (bit-perfect candidates).
    pub fn lossless(&self) -> bool {
        matches!(self.codec.as_str(), "FLAC" | "ALAC" | "PCM")
    }
}

/// A decode failure, sorted into the engine's error codes.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// Reading the source failed.
    #[error("read failed: {0}")]
    Io(io::Error),
    /// The format or codec is not supported.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// The data is broken beyond skipping a packet.
    #[error("decode failed: {0}")]
    Broken(String),
}

impl DecodeError {
    /// The engine error code for this failure.
    pub fn code(&self) -> ErrorCode {
        match self {
            DecodeError::Io(e) => match SourceError::from_io(e) {
                Some(SourceError::TokenExpired) => ErrorCode::TokenExpired,
                _ => ErrorCode::Network,
            },
            DecodeError::Unsupported(_) => ErrorCode::FormatUnsupported,
            DecodeError::Broken(_) => ErrorCode::Decode,
        }
    }

    /// True when the engine dropped the item (not worth reporting).
    pub fn cancelled(&self) -> bool {
        matches!(self, DecodeError::Io(e) if SourceError::from_io(e) == Some(&SourceError::Cancelled))
    }
}

fn map_err(e: SymError) -> DecodeError {
    match e {
        SymError::IoError(e) => DecodeError::Io(e),
        SymError::Unsupported(s) => DecodeError::Unsupported(s.to_owned()),
        other => DecodeError::Broken(other.to_string()),
    }
}

fn codec_name(id: symphonia::core::codecs::audio::AudioCodecId) -> &'static str {
    match id {
        CODEC_ID_FLAC => "FLAC",
        CODEC_ID_ALAC => "ALAC",
        CODEC_ID_MP3 => "MP3",
        CODEC_ID_AAC => "AAC",
        CODEC_ID_VORBIS => "Vorbis",
        // Everything else symphonia decodes here is a PCM variant (WAV, AIFF).
        _ => "PCM",
    }
}

/// ALAC keeps its bit depth in the magic cookie, not in the codec parameters (phase 0 finding).
/// The cookie is an `ALACSpecificConfig` (24 bytes), sometimes still wrapped in its 12-byte `alac` box.
pub fn alac_bits(extra: &[u8]) -> Option<u16> {
    let config = if extra.len() >= 36 && &extra[4..8] == b"alac" {
        &extra[12..]
    } else {
        extra
    };
    config.get(5).map(|b| u16::from(*b)).filter(|b| *b > 0)
}

/// An open, decodable stream.
pub struct TrackDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    /// Format details.
    pub info: StreamInfo,
    samples: Vec<i32>,
    /// Frames the decoder has produced, counted from the stream start (or the frame a seek landed on).
    pos: u64,
    /// Frames before this one are dropped: AAC priming, or the part of a block before an accurate seek's target.
    skip_until: u64,
    /// Frames from this one on are dropped (AAC padding).
    end_at: Option<u64>,
    /// AAC priming frames: the stream's time 0 is this many frames into the decoded sound.
    delay: u64,
}

impl std::fmt::Debug for TrackDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrackDecoder")
            .field("info", &self.info)
            .finish()
    }
}

impl TrackDecoder {
    /// Probes `source` (with an extension hint) and opens its default audio track.
    pub fn open(mut source: Box<dyn MediaSource>, hint: Option<&str>) -> Result<Self, DecodeError> {
        // An unreadable box only costs the trim; the probe below reports real read errors. Live streams can't
        // seek (and aren't MP4 files).
        let gap = if source.is_seekable() {
            let gap = gapless::read(&mut source).ok().flatten();
            source.seek(SeekFrom::Start(0)).map_err(DecodeError::Io)?;
            gap
        } else {
            None
        };
        let mss = MediaSourceStream::new(source, Default::default());
        let mut h = Hint::new();
        if let Some(ext) = hint {
            h.with_extension(ext);
        }
        let format = symphonia::default::get_probe()
            .probe(
                &h,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(map_err)?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| DecodeError::Unsupported("no audio track".into()))?;
        let track_id = track.id;
        let frames = track.num_frames;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|c| c.audio())
            .ok_or_else(|| DecodeError::Unsupported("no audio codec parameters".into()))?
            .clone();
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(map_err)?;
        let codec = codec_name(params.codec);
        let bits = params
            .bits_per_sample
            .and_then(|b| u16::try_from(b).ok())
            .or_else(|| {
                (params.codec == CODEC_ID_ALAC)
                    .then(|| params.extra_data.as_deref().and_then(alac_bits))
                    .flatten()
            })
            .filter(|_| codec != "MP3" && codec != "AAC" && codec != "Vorbis");
        let sample_rate = params.sample_rate.unwrap_or(0);
        // symphonia doesn't apply MP4 edit lists: cut AAC's priming and padding here (MP3 is trimmed by symphonia).
        let (delay, valid) = gap
            .filter(|_| codec == "AAC" && sample_rate > 0)
            .map(|g| g.frames(sample_rate))
            .filter(|(d, _)| frames.is_none_or(|n| *d < n))
            .map(|(d, v)| (d, v.filter(|v| frames.is_none_or(|n| d + v <= n))))
            .unwrap_or((0, None));
        let info = StreamInfo {
            sample_rate,
            channels: params
                .channels
                .as_ref()
                .map(|c| c.count() as u16)
                .unwrap_or(0),
            bits,
            codec: codec.to_owned(),
            frames: valid.or(frames.map(|n| n - delay)),
        };
        Ok(Self {
            format,
            decoder,
            track_id,
            info,
            samples: Vec::new(),
            pos: 0,
            skip_until: delay,
            end_at: valid.map(|v| delay + v),
            delay,
        })
    }

    /// The next block of interleaved samples, or `None` at the end of the stream.
    pub fn next_block(&mut self) -> Result<Option<&[i32]>, DecodeError> {
        loop {
            if self.end_at.is_some_and(|e| self.pos >= e) {
                return Ok(None);
            }
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(None),
                // A chained stream or a truncated file: treat as the end.
                Err(SymError::ResetRequired) => return Ok(None),
                Err(SymError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    return Ok(None)
                }
                Err(e) => return Err(map_err(e)),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let buf = match self.decoder.decode(&packet) {
                Ok(b) => b,
                // A damaged packet: skip it, as symphonia recommends.
                Err(SymError::DecodeError(e)) => {
                    tracing::debug!(target: "audio", "skipped a damaged packet: {e}");
                    continue;
                }
                Err(e) => return Err(map_err(e)),
            };
            let channels = buf.spec().channels().count().max(1);
            if self.info.channels == 0 {
                self.info.channels = channels as u16;
            }
            if self.info.sample_rate == 0 {
                self.info.sample_rate = buf.spec().rate();
            }
            copy_samples(&buf, &mut self.samples);
            let frames = (self.samples.len() / channels) as u64;
            let start = self.pos;
            self.pos += frames;
            let keep_from = self.skip_until.saturating_sub(start).min(frames);
            let keep_to = self
                .end_at
                .map_or(frames, |e| e.saturating_sub(start).min(frames));
            if keep_to <= keep_from {
                if self.end_at.is_some_and(|e| start >= e) {
                    return Ok(None);
                }
                continue;
            }
            self.samples.truncate(keep_to as usize * channels);
            self.samples.drain(..keep_from as usize * channels);
            return Ok(Some(&self.samples));
        }
    }

    /// Seeks to `sec` (sample-accurate); returns the position reached.
    pub fn seek(&mut self, sec: f64) -> Result<f64, DecodeError> {
        let rate = f64::from(self.info.sample_rate.max(1));
        let time = Time::try_from_secs_f64(sec.max(0.0) + self.delay as f64 / rate)
            .ok_or_else(|| DecodeError::Broken(format!("bad seek time {sec}")))?;
        let seeked = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(map_err)?;
        self.decoder.reset();
        // Accurate mode lands on or before the target; drop the frames in between.
        let early = seeked
            .required_ts
            .get()
            .saturating_sub(seeked.actual_ts.get());
        let required = self.ts_to_frames(seeked.required_ts.get().max(0) as u64);
        self.skip_until = required;
        self.pos = required.saturating_sub(self.ts_to_frames(early.max(0) as u64));
        Ok(required.saturating_sub(self.delay) as f64 / rate)
    }

    /// Track timestamps are in the track's time base; convert to frames at the sample rate.
    fn ts_to_frames(&self, ts: u64) -> u64 {
        let base = self
            .format
            .tracks()
            .iter()
            .find(|t| t.id == self.track_id)
            .and_then(|t| t.time_base);
        match base {
            Some(tb) => {
                let (n, d) = (u64::from(tb.numer.get()), u64::from(tb.denom.get()));
                let rate = u64::from(self.info.sample_rate.max(1));
                // ts * n / d seconds * rate frames/s, in 128-bit to stay exact.
                ((u128::from(ts) * u128::from(n) * u128::from(rate)) / u128::from(d.max(1))) as u64
            }
            None => ts,
        }
    }
}

fn copy_samples(buf: &GenericAudioBufferRef<'_>, out: &mut Vec<i32>) {
    buf.copy_to_vec_interleaved(out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_media::{ramp_wav, wav_bytes};

    fn open_bytes(bytes: Vec<u8>, hint: &str) -> TrackDecoder {
        TrackDecoder::open(Box::new(io::Cursor::new(bytes)), Some(hint)).unwrap()
    }

    #[test]
    fn wav_decodes_left_justified_with_its_format() {
        let mut d = open_bytes(ramp_wav(44_100, 2, 16, 1000), "wav");
        assert_eq!(d.info.sample_rate, 44_100);
        assert_eq!(d.info.channels, 2);
        assert_eq!(d.info.bits, Some(16));
        assert_eq!(d.info.codec, "PCM");
        assert!(d.info.lossless());
        assert_eq!(d.info.duration_sec(), Some(1000.0 / 44_100.0));
        let mut all = Vec::new();
        while let Some(b) = d.next_block().unwrap() {
            all.extend_from_slice(b);
        }
        assert_eq!(all.len(), 2000);
        // Frame i holds sample value i (left) and -i (right), shifted to the top 16 bits.
        assert_eq!(all[2 * 7], 7 << 16);
        assert_eq!(all[2 * 7 + 1], -7 << 16);
        assert!(all.iter().all(|s| s & 0xFFFF == 0));
    }

    #[test]
    fn seeks_to_the_exact_frame() {
        let mut d = open_bytes(ramp_wav(44_100, 2, 16, 44_100 * 2), "wav");
        let at = d.seek(1.0).unwrap();
        assert!((at - 1.0).abs() < 1e-6);
        let first = d.next_block().unwrap().unwrap()[0] >> 16;
        assert!(
            (first - (44_100 % 32_768)).abs() <= 1,
            "frame 44100 (ramp wraps at 32768): {first}"
        );
    }

    #[test]
    fn broken_and_unknown_input_map_to_error_codes() {
        let err = TrackDecoder::open(Box::new(io::Cursor::new(vec![0u8; 64])), None).unwrap_err();
        assert_eq!(err.code(), ErrorCode::FormatUnsupported);
        let io_err = DecodeError::Io(io::Error::other(SourceError::TokenExpired));
        assert_eq!(io_err.code(), ErrorCode::TokenExpired);
        assert_eq!(
            DecodeError::Io(io::Error::other(SourceError::Network("x".into()))).code(),
            ErrorCode::Network
        );
        assert!(DecodeError::Io(io::Error::other(SourceError::Cancelled)).cancelled());
        assert_eq!(DecodeError::Broken("x".into()).code(), ErrorCode::Decode);
        // A WAV cut short ends early instead of failing.
        let mut cut = wav_bytes(48_000, 1, 16, &vec![1i32 << 16; 4800]);
        cut.truncate(cut.len() - 1001);
        let mut d = open_bytes(cut, "wav");
        while d.next_block().unwrap().is_some() {}
    }

    /// FFmpeg AAC, 0.5 s of a 440 Hz sine at 44.1 kHz stereo: 22,050 frames of sound in 23 packets (23,552
    /// decoded frames), edit list starting at 1,024.
    const AAC_SINE: &[u8] = include_bytes!("../testdata/sine-0.5s-aac.m4a");

    fn decode_all(d: &mut TrackDecoder) -> Vec<i32> {
        let mut all = Vec::new();
        while let Some(b) = d.next_block().unwrap() {
            all.extend_from_slice(b);
        }
        all
    }

    #[test]
    fn aac_priming_and_padding_are_cut_for_gapless() {
        let mut d = open_bytes(AAC_SINE.to_vec(), "m4a");
        assert_eq!(d.info.codec, "AAC");
        assert_eq!(d.info.frames, Some(22_050));
        let all = decode_all(&mut d);
        assert_eq!(all.len() / 2, 22_050);
        // The sine starts at once, not after a block of near-silent priming.
        let peak = |s: &[i32]| s.iter().map(|v| (*v >> 16).unsigned_abs()).max().unwrap();
        assert!(peak(&all[..2 * 64]) > 500, "{}", peak(&all[..2 * 64]));
        // Seeking counts from the real start, and the end is cut there too.
        let at = d.seek(0.25).unwrap();
        assert!((at - 0.25).abs() < 1e-6, "{at}");
        assert_eq!(decode_all(&mut d).len() / 2, 11_025);
    }

    #[test]
    fn alac_bit_depth_comes_from_the_cookie() {
        let mut config = vec![0u8; 24];
        config[5] = 24;
        assert_eq!(alac_bits(&config), Some(24));
        let mut boxed = vec![0, 0, 0, 36];
        boxed.extend_from_slice(b"alac");
        boxed.extend_from_slice(&[0; 4]);
        boxed.extend_from_slice(&config);
        assert_eq!(alac_bits(&boxed), Some(24));
        assert_eq!(alac_bits(&[]), None);
    }
}
