//! Output backends. Each runs its own thread and pulls device-format bytes from the current ring buffer
//! through [`RenderCore`], which is shared by every backend: ring handoff on load/seek, pause, the volume
//! ramp and the played-frames counter the engine turns into a position.

pub mod null;
#[cfg(windows)]
pub mod wasapi;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::convert::{apply_volume, DeviceFormat};
use crate::types::{ErrorCode, OutputMode, PathKind};

/// An error opening or running an output.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct OutputError {
    /// Kind.
    pub code: ErrorCode,
    /// Detail.
    pub message: String,
}

impl OutputError {
    /// An error of `code`.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// State shared between the engine and an output thread. Only atomics and `try_lock` on the audio side.
#[derive(Debug, Default)]
pub struct OutputShared {
    pending: Mutex<Option<(u64, rtrb::Consumer<u8>)>>,
    paused: AtomicBool,
    volume: AtomicU32,
    /// Generation (high 16 bits) and frames of it that reached the device (low 48 bits).
    played: AtomicU64,
    underruns: AtomicU64,
    error: Mutex<Option<OutputError>>,
    stop: AtomicBool,
}

const FRAMES_MASK: u64 = (1 << 48) - 1;

impl OutputShared {
    /// New state at full volume.
    pub fn new() -> Arc<Self> {
        let s = Self::default();
        s.volume.store(1.0f32.to_bits(), Ordering::Relaxed);
        Arc::new(s)
    }

    /// Hands the output a fresh ring (load/seek); whatever the old one still held is dropped.
    pub fn set_source(&self, generation: u64, consumer: rtrb::Consumer<u8>) {
        let mut g = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        *g = Some((generation, consumer));
    }

    /// Pauses (silence, nothing consumed) or resumes.
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    /// Linear volume 0..=1.
    pub fn set_volume(&self, linear: f32) {
        self.volume
            .store(linear.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    /// Current linear volume.
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    /// Frames of ring `generation` that have reached the device, once the output plays that ring.
    pub fn played(&self, generation: u64) -> Option<u64> {
        let v = self.played.load(Ordering::Acquire);
        ((v >> 48) == (generation & 0xFFFF)).then_some(v & FRAMES_MASK)
    }

    /// Buffers that ran dry while a ring was playing.
    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    /// Records a fatal device error (the thread then stops).
    pub fn fail(&self, error: OutputError) {
        let mut g = self.error.lock().unwrap_or_else(|p| p.into_inner());
        g.get_or_insert(error);
    }

    /// The device error, once.
    pub fn take_error(&self) -> Option<OutputError> {
        self.error.lock().unwrap_or_else(|p| p.into_inner()).take()
    }

    /// Asks the output thread to end.
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// True once [`Self::request_stop`] was called.
    pub fn stopping(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

/// Fills device buffers from the current ring. Used on the output thread; never blocks or allocates after
/// the first call.
pub struct RenderCore {
    shared: Arc<OutputShared>,
    format: DeviceFormat,
    current: Option<rtrb::Consumer<u8>>,
    generation: u64,
    last_volume: f32,
    /// Every frame handed to the device (sound and silence filler).
    device_total: u64,
    /// Buffers still (partly) in the device: (first device frame, frames, frames of sound at the start).
    in_flight: std::collections::VecDeque<(u64, u64, u64)>,
    /// Sound frames of the current ring that the device has played out of retired buffers.
    played_done: u64,
    /// The current ring has delivered sound (an empty read before that is buffering, not an underrun).
    primed: bool,
}

impl RenderCore {
    /// A core for a device of `format`.
    pub fn new(shared: Arc<OutputShared>, format: DeviceFormat) -> Self {
        let last_volume = shared.volume();
        Self {
            shared,
            format,
            current: None,
            generation: 0,
            last_volume,
            device_total: 0,
            in_flight: std::collections::VecDeque::with_capacity(64),
            played_done: 0,
            primed: false,
        }
    }

    /// Fills `out` (whole frames) for the device. `queued_frames` is what the device still holds from
    /// earlier buffers (sound or silence filler), so only sound that has left the device counts as played.
    pub fn render(&mut self, out: &mut [u8], queued_frames: u64) {
        if let Ok(mut pending) = self.shared.pending.try_lock() {
            if let Some((generation, consumer)) = pending.take() {
                self.current = Some(consumer);
                self.generation = generation;
                // Sound still queued from the old ring no longer counts for anyone.
                self.in_flight.clear();
                self.played_done = 0;
                self.primed = false;
            }
        }
        let fb = self.format.frame_bytes().max(1);
        let mut filled = 0;
        if !self.shared.paused.load(Ordering::Relaxed) {
            if let Some(consumer) = self.current.as_mut() {
                let want = (consumer.slots().min(out.len()) / fb) * fb;
                if let Ok(chunk) = consumer.read_chunk(want) {
                    let (a, b) = chunk.as_slices();
                    out[..a.len()].copy_from_slice(a);
                    out[a.len()..a.len() + b.len()].copy_from_slice(b);
                    filled = a.len() + b.len();
                    chunk.commit_all();
                }
                if filled < out.len() && self.primed && !consumer.is_abandoned() {
                    self.shared.underruns.fetch_add(1, Ordering::Relaxed);
                }
                self.primed |= filled > 0;
            }
        }
        out[filled..].fill(0);
        let volume = self.shared.volume();
        apply_volume(&mut out[..filled], &self.format, self.last_volume, volume);
        self.last_volume = volume;
        // Which sound frames have left the device: buffers are queued in order, sound first in each.
        let device_played = self.device_total.saturating_sub(queued_frames);
        let mut in_device = 0;
        while let Some(&(start, len, sound)) = self.in_flight.front() {
            if start + len <= device_played {
                self.played_done += sound;
                self.in_flight.pop_front();
            } else {
                in_device = sound.min(device_played.saturating_sub(start));
                break;
            }
        }
        let frames = (out.len() / fb) as u64;
        if frames > 0 && self.in_flight.len() < self.in_flight.capacity() {
            self.in_flight
                .push_back((self.device_total, frames, (filled / fb) as u64));
        } else if let Some(last) = self.in_flight.back_mut() {
            // Never grow on the audio thread: merge into the last entry instead.
            last.1 += frames;
            last.2 += (filled / fb) as u64;
        }
        self.device_total += frames;
        let played = (self.played_done + in_device) & FRAMES_MASK;
        self.shared.played.store(
            ((self.generation & 0xFFFF) << 48) | played,
            Ordering::Release,
        );
    }
}

/// An open output device.
pub trait Output: Send {
    /// The format the device was opened with (what the ring must hold).
    fn format(&self) -> DeviceFormat;
    /// State shared with the output thread.
    fn shared(&self) -> &Arc<OutputShared>;
    /// What this output does to samples (e.g. Windows' shared mixer).
    fn path(&self) -> Vec<PathKind>;
}

/// Opens outputs for the engine (the real device, or [`null::NullFactory`] in tests).
pub trait OutputFactory: Send {
    /// Opens `device_id` (or the default) in `mode`.
    fn open(
        &self,
        device_id: Option<&str>,
        mode: OutputMode,
    ) -> Result<Box<dyn Output>, OutputError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::SampleKind;

    #[test]
    fn render_swaps_rings_pauses_and_counts() {
        let shared = OutputShared::new();
        let fmt = DeviceFormat {
            sample_rate: 48_000,
            channels: 1,
            kind: SampleKind::I16,
        };
        let mut core = RenderCore::new(shared.clone(), fmt);
        let mut out = [7u8; 8];
        core.render(&mut out, 0);
        assert_eq!(out, [0; 8], "nothing to play yet: silence");
        assert_eq!(shared.played(0), Some(0));

        let (mut p, c) = rtrb::RingBuffer::<u8>::new(64);
        for b in [1u8, 0, 2, 0, 3, 0] {
            p.push(b).unwrap();
        }
        shared.set_source(1, c);
        core.render(&mut out, 1);
        assert_eq!(out, [1, 0, 2, 0, 3, 0, 0, 0]);
        assert_eq!(
            shared.played(1),
            Some(0),
            "this buffer has only just gone to the device"
        );
        assert_eq!(shared.played(0), None, "an older ring is not reported");
        assert_eq!(
            shared.underruns(),
            0,
            "a ring that is still filling is not a dropout"
        );

        shared.set_paused(true);
        p.push(9).unwrap();
        p.push(0).unwrap();
        core.render(&mut out, 0);
        assert_eq!(out, [0; 8]);
        assert_eq!(shared.played(1), Some(3));
        shared.set_paused(false);
        shared.set_volume(0.5);
        core.render(&mut out, 0);
        assert_eq!(
            i16::from_le_bytes([out[0], out[1]]),
            4,
            "ramped from 1.0 to 0.5 over one frame"
        );
        assert_eq!(shared.underruns(), 1, "ran dry after sound started");

        shared.fail(OutputError::new(ErrorCode::DeviceLost, "gone"));
        shared.fail(OutputError::new(ErrorCode::Network, "second is ignored"));
        assert_eq!(
            shared.take_error().map(|e| e.code),
            Some(ErrorCode::DeviceLost)
        );
        assert!(shared.take_error().is_none());
        assert!(!shared.stopping());
        shared.request_stop();
        assert!(shared.stopping());
    }
}
