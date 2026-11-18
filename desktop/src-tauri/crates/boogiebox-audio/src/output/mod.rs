//! Output backends. Each runs its own thread and pulls samples from the current ring buffer through
//! [`RenderCore`], which is shared by every backend: ring handoff on load/seek, pause, the volume ramp and
//! the played-frames counter the engine turns into a position.
//!
//! A ring holds either device-format bytes (bit-exact playback, Pure mode) or 32-bit floats at the device's
//! rate and channels (sound processing on). Float rings get the EQ here, per song ([`PlanTable`]), so a change
//! is heard within one device buffer; then the volume, TPDF dither and the conversion to the device format.

pub mod null;
#[cfg(windows)]
pub mod wasapi;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::convert::{apply_volume, read_sample, write_dithered, write_float, DeviceFormat};
use crate::dsp::{Dither, EqPlan, Equalizer, LevelMeter};
use crate::negotiate::DeviceCaps;
use crate::types::{Device, ErrorCode, OutputMode, PathKind};

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
    pending: Mutex<Option<(u64, rtrb::Consumer<u8>, RingKind)>>,
    plans: Mutex<Option<PlanTable>>,
    meter: Mutex<Option<LevelMeter>>,
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
    pub fn set_source(&self, generation: u64, consumer: rtrb::Consumer<u8>, kind: RingKind) {
        let mut g = self.pending.lock().unwrap_or_else(|p| p.into_inner());
        *g = Some((generation, consumer, kind));
    }

    /// The EQ for each song in a float ring (taken on the next buffer).
    pub fn set_plans(&self, table: PlanTable) {
        *self.plans.lock().unwrap_or_else(|p| p.into_inner()) = Some(table);
    }

    /// Peak and RMS since the last call and the latest `points` of the meter's waveform; `None` before the
    /// output has played anything.
    pub fn take_levels(&self, points: usize, wave: &mut Vec<f32>) -> Option<(f32, f32)> {
        let mut g = self.meter.lock().unwrap_or_else(|p| p.into_inner());
        g.as_mut().map(|m| m.take(points, wave))
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

/// What a ring holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RingKind {
    /// Device-format bytes.
    #[default]
    Device,
    /// Little-endian `f32` at the device's rate and channels.
    Float,
}

/// Most songs a [`PlanTable`] covers (the one playing and the ones queued behind it in the ring).
pub const MAX_PLANS: usize = 4;

/// The EQ of each song in a float ring: from ring frame `start` on, `plan` applies; `dither` when the
/// samples were changed before the output (else samples of up to 24 bits go out exactly).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlanTable {
    /// The ring these frames belong to.
    pub generation: u64,
    entries: [(u64, EqPlan, bool); MAX_PLANS],
    len: usize,
}

impl PlanTable {
    /// An empty table for ring `generation`.
    pub fn new(generation: u64) -> Self {
        Self {
            generation,
            ..Self::default()
        }
    }

    /// Adds a song starting at ring frame `start` (in order; extra songs are dropped).
    pub fn push(&mut self, start: u64, plan: EqPlan, dither: bool) {
        if self.len < MAX_PLANS {
            self.entries[self.len] = (start, plan, dither);
            self.len += 1;
        }
    }

    /// The entry for ring frame `frame` and the frame where the next one starts.
    fn at(&self, frame: u64) -> (EqPlan, bool, Option<u64>) {
        let entries = &self.entries[..self.len];
        let idx = entries.iter().rposition(|e| e.0 <= frame).unwrap_or(0);
        match entries.get(idx) {
            Some(&(_, plan, dither)) => (plan, dither, entries.get(idx + 1).map(|e| e.0)),
            // No table yet: unchanged (gains and fades come with one).
            None => (EqPlan::default(), false, None),
        }
    }
}

/// Fills device buffers from the current ring. Used on the output thread; never blocks, and allocates only
/// when a device buffer is larger than any before.
pub struct RenderCore {
    shared: Arc<OutputShared>,
    format: DeviceFormat,
    current: Option<rtrb::Consumer<u8>>,
    kind: RingKind,
    generation: u64,
    /// Frames read from the current ring.
    ring_frames: u64,
    plans: PlanTable,
    eq: Equalizer,
    dither: Dither,
    floats: Vec<f32>,
    previous: Vec<f32>,
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
        *shared.meter.lock().unwrap_or_else(|p| p.into_inner()) =
            Some(LevelMeter::new(format.sample_rate));
        Self {
            shared,
            format,
            current: None,
            kind: RingKind::Device,
            generation: 0,
            ring_frames: 0,
            plans: PlanTable::default(),
            eq: Equalizer::default(),
            dither: Dither::default(),
            floats: Vec::new(),
            previous: Vec::new(),
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
            if let Some((generation, consumer, kind)) = pending.take() {
                self.current = Some(consumer);
                self.kind = kind;
                self.generation = generation;
                self.ring_frames = 0;
                self.plans = PlanTable::new(generation);
                self.eq.reset();
                // Sound still queued from the old ring no longer counts for anyone.
                self.in_flight.clear();
                self.played_done = 0;
                self.primed = false;
            }
        }
        if let Ok(mut plans) = self.shared.plans.try_lock() {
            if plans
                .as_ref()
                .is_some_and(|t| t.generation == self.generation)
            {
                if let Some(t) = plans.take() {
                    self.plans = t;
                }
            }
        }
        let fb = self.format.frame_bytes().max(1);
        let channels = usize::from(self.format.channels.max(1));
        let ring_fb = match self.kind {
            RingKind::Device => fb,
            RingKind::Float => 4 * channels,
        };
        let frames_out = out.len() / fb;
        let mut got = 0;
        if !self.shared.paused.load(Ordering::Relaxed) {
            if let Some(consumer) = self.current.as_mut() {
                let want = consumer.slots().min(frames_out * ring_fb) / ring_fb * ring_fb;
                if let Ok(chunk) = consumer.read_chunk(want) {
                    let (a, b) = chunk.as_slices();
                    got = (a.len() + b.len()) / ring_fb;
                    match self.kind {
                        RingKind::Device => {
                            out[..a.len()].copy_from_slice(a);
                            out[a.len()..a.len() + b.len()].copy_from_slice(b);
                        }
                        RingKind::Float => {
                            self.floats.clear();
                            for bytes in a.as_chunks::<4>().0.iter().chain(b.as_chunks::<4>().0) {
                                self.floats.push(f32::from_le_bytes(*bytes));
                            }
                        }
                    }
                    chunk.commit_all();
                }
                if got < frames_out && self.primed && !consumer.is_abandoned() {
                    self.shared.underruns.fetch_add(1, Ordering::Relaxed);
                }
                self.primed |= got > 0;
            }
        }
        let filled = got * fb;
        let volume = self.shared.volume();
        match self.kind {
            RingKind::Device => {
                if got > 0 {
                    self.meter_bytes(filled, out, channels);
                }
                apply_volume(
                    &mut out[..filled],
                    &self.format,
                    self.last_volume,
                    volume,
                    &mut self.dither,
                );
            }
            RingKind::Float => {
                if got > 0 {
                    self.meter(channels);
                }
                self.render_floats(&mut out[..filled], got, channels, volume);
            }
        }
        out[filled..].fill(0);
        self.ring_frames += got as u64;
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

    /// Feeds the VU meter (skipped for this buffer while the engine reads it).
    fn meter(&self, channels: usize) {
        if let Ok(mut m) = self.shared.meter.try_lock() {
            if let Some(m) = m.as_mut() {
                m.add(&self.floats, channels);
            }
        }
    }

    fn meter_bytes(&mut self, filled: usize, out: &[u8], channels: usize) {
        let kind = self.format.kind;
        self.floats.clear();
        self.floats.extend(
            out[..filled]
                .chunks_exact(kind.bytes())
                .map(|b| read_sample(b, kind)),
        );
        self.meter(channels);
    }

    /// EQ (per song; a crossfade over one buffer when the curve changes), volume, dither, device format.
    fn render_floats(&mut self, out: &mut [u8], frames: usize, channels: usize, volume: f32) {
        let mut done = 0;
        let mut changed = self.last_volume != 1.0 || volume != 1.0;
        while done < frames {
            let frame = self.ring_frames + done as u64;
            let (plan, dither, next_start) = self.plans.at(frame);
            let until = next_start
                .map(|n| (n.saturating_sub(self.ring_frames) as usize).clamp(done + 1, frames))
                .unwrap_or(frames);
            changed |= dither || !plan.is_identity();
            let span = done * channels..until * channels;
            if *self.eq.plan() != plan {
                // Run the old and new curves side by side and fade from one to the other: no click.
                self.previous.clear();
                self.previous.extend_from_slice(&self.floats[span.clone()]);
                let mut old = self.eq;
                old.process(&mut self.previous, channels);
                self.eq.set_plan(plan);
                self.eq.process(&mut self.floats[span.clone()], channels);
                let n = (until - done).max(1) as f32;
                for (i, f) in self.floats[span].chunks_exact_mut(channels).enumerate() {
                    let t = (i + 1) as f32 / n;
                    for (c, s) in f.iter_mut().enumerate() {
                        *s = self.previous[i * channels + c] * (1.0 - t) + *s * t;
                    }
                }
            } else {
                self.eq.process(&mut self.floats[span], channels);
            }
            done = until;
        }
        let (from, to) = (self.last_volume, volume);
        let kind = self.format.kind;
        let sb = kind.bytes();
        for (i, (f, bytes)) in self
            .floats
            .chunks_exact(channels)
            .zip(out.chunks_exact_mut(channels * sb))
            .take(frames)
            .enumerate()
        {
            let g = if frames > 1 {
                from + (to - from) * (i as f32 / (frames - 1) as f32)
            } else {
                to
            };
            for (s, b) in f.iter().zip(bytes.chunks_exact_mut(sb)) {
                if changed {
                    write_dithered(*s * g, kind, &mut self.dither, b);
                } else {
                    write_float(*s, kind, b);
                }
            }
        }
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
    /// True when the engine owns the device (exclusive mode).
    fn exclusive(&self) -> bool {
        false
    }
}

/// How to open an output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputRequest {
    /// Shared or exclusive.
    pub mode: OutputMode,
    /// The exact format for exclusive mode (from [`crate::negotiate::choose`]); ignored in shared mode.
    pub format: Option<DeviceFormat>,
    /// Exclusive-mode buffer (ms).
    pub buffer_ms: u32,
}

/// Default exclusive buffer (plan §8.2: 50 ms, "Normal").
pub const DEFAULT_BUFFER_MS: u32 = 50;

/// Called when the device list changes.
pub type DeviceNotify = Arc<dyn Fn() + Send + Sync>;

/// Keeps a device watch alive; dropping it stops watching.
pub trait WatchGuard: Send {}

/// Opens outputs for the engine (the real device, or [`null::NullFactory`] in tests).
pub trait OutputFactory: Send {
    /// Opens `device_id` (or the default).
    fn open(
        &self,
        device_id: Option<&str>,
        request: &OutputRequest,
    ) -> Result<Box<dyn Output>, OutputError>;

    /// The exclusive formats the device takes; [`ErrorCode::ExclusiveNotAllowed`] when Windows forbids it.
    fn exclusive_caps(&self, device_id: Option<&str>) -> Result<DeviceCaps, OutputError>;

    /// Output devices now.
    fn devices(&self) -> Vec<Device> {
        Vec::new()
    }

    /// Starts watching for device changes.
    fn watch(&self, _notify: DeviceNotify) -> Option<Box<dyn WatchGuard>> {
        None
    }
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
        shared.set_source(1, c, RingKind::Device);
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
