//! The engine (`wip/native-hires-engine-plan.md` §4, §7, §8).
//!
//! - The **control thread** owns the output device and the transport. It turns commands into sessions and
//!   the output's played-frames counter into positions and events (~50 Hz checks, `State` at 4 Hz). It opens
//!   the device when a decoder thread asks for it, in the format that thread needs.
//! - A **decoder thread** per session opens the item, asks for the device, decodes, converts to the device
//!   format and fills the session's ring buffer (~2 s). At the end of an item it carries straight on with
//!   the next one in the same ring (gapless). In exclusive mode an item in another format makes it wait until
//!   the device has played everything, then the device is reopened at the new format (D8).
//! - **Prefetch**: `SetNext` opens the next item early (first blocks + probe) so the join doesn't wait on
//!   the network.
//!
//! Exclusive mode chooses each item's device format with [`negotiate::choose`] (D9). If Windows or another
//! app refuses exclusive mode the engine says so and plays in shared mode (never silently).
//!
//! Positions come from frames that reached the device, so `TrackStarted`/`TrackEnded` fire when the sound
//! changes, not when decoding moves on. A lost device pauses playback and resumes it if the device comes back
//! within 30 s (unplugged DAC, sleep).
//!
//! **Sound processing** (phase 5, off in Pure mode): with EQ, levelling or crossfade on, a session's ring holds
//! floats. The decoder thread applies each item's levelling gain and mixes crossfades (equal-power); the output
//! thread applies each item's EQ from the [`PlanTable`] this thread keeps up to date, so EQ changes are heard
//! at once. Without processing the ring holds device bytes, bit for bit.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use crate::convert::{encode_f32, encode_i32_exact, i32_to_f32, map_channels, DeviceFormat};
use crate::decode::{DecodeError, StreamInfo, TrackDecoder};
use crate::dsp::{crossfade_gains, db_to_linear, EqBand, EqPlan};
use crate::negotiate::{self, DeviceCaps};
use crate::output::{
    Output, OutputError, OutputFactory, OutputRequest, OutputShared, PlanTable, RingKind,
    WatchGuard, DEFAULT_BUFFER_MS, MAX_PLANS,
};
use crate::resample::StreamResampler;
use crate::source::{HttpRangeSource, SourceOptions, UrlSlot};
use crate::types::{
    Command, ErrorCode, Event, Format, OutputMode, PathKind, PlayItem, SoundSettings, Status,
};

/// Ring buffer length.
pub const RING_SECONDS: f64 = 2.0;
/// `NeedNext` fires this long before the end when no next item is set.
pub const NEED_NEXT_SEC: f64 = 15.0;
/// How much of the next item is fetched ahead.
pub const PREFETCH_SECONDS: f64 = 8.0;
/// At the end of an item the decoder waits for a next one until only this much sound is left buffered.
pub const END_WAIT_SEC: f64 = 0.25;
/// A lost device that comes back within this long picks up where it stopped.
pub const RESUME_WITHIN: Duration = Duration::from_secs(30);
const RESUME_RETRY: Duration = Duration::from_secs(2);
const TICK: Duration = Duration::from_millis(20);
const STATE_EVERY: Duration = Duration::from_millis(250);
const LEVELS_EVERY: Duration = Duration::from_millis(50);
/// Waveform points per `Levels` event (~21 ms at the meter's ~24 kHz).
const LEVEL_POINTS: usize = 512;
/// A crossfade needs at least this much of the outgoing item left (s).
const MIN_FADE_SEC: f64 = 0.5;
/// Rate used to decide whether a curve changes anything before the device rate is known.
const NOMINAL_RATE: u32 = 48_000;

/// An item's levelling gain and own curve.
#[derive(Debug, Clone, Default, PartialEq)]
struct ItemSound {
    gain_db: f64,
    eq: Option<Vec<EqBand>>,
}

/// The sound settings and every known item's gain and curve, shared with decoder threads.
#[derive(Debug, Default)]
struct SoundTable {
    settings: SoundSettings,
    items: HashMap<String, ItemSound>,
}

impl SoundTable {
    /// Something may change the samples (EQ, levelling or crossfade on, Pure mode off).
    fn processing(&self) -> bool {
        let s = &self.settings;
        !s.pure
            && (s.levelling
                || s.crossfade_sec > 0.0
                || s.item_eq
                || !EqPlan::new(&s.eq, NOMINAL_RATE).is_identity())
    }

    fn gain_db(&self, track_id: &str) -> f64 {
        if self.settings.pure || !self.settings.levelling {
            return 0.0;
        }
        self.items.get(track_id).map_or(0.0, |i| i.gain_db)
    }

    fn eq(&self, track_id: &str) -> &[EqBand] {
        if self.settings.pure {
            return &[];
        }
        match self.items.get(track_id).and_then(|i| i.eq.as_deref()) {
            Some(own) if self.settings.item_eq => own,
            _ => &self.settings.eq,
        }
    }

    fn crossfade_sec(&self) -> f64 {
        if self.settings.pure {
            0.0
        } else {
            self.settings.crossfade_sec.clamp(0.0, 30.0)
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Engine settings.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// First network retry delay.
    pub backoff: Duration,
    /// How long a lost device may take to come back before playback stops.
    pub resume_within: Duration,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            backoff: Duration::from_millis(250),
            resume_within: RESUME_WITHIN,
        }
    }
}

/// What reaches the control thread.
enum Msg {
    Cmd(Command),
    /// A decoder thread needs the device.
    Output(OutputAsk),
    /// The device list changed.
    Devices,
}

/// A decoder thread's request for the device: exclusive at `format`, or shared when `None`.
struct OutputAsk {
    session: Arc<AtomicBool>,
    format: Option<DeviceFormat>,
    reply: Sender<Option<Grant>>,
}

/// The device a decoder thread may fill.
#[derive(Clone)]
struct Grant {
    generation: u64,
    format: DeviceFormat,
    shared: Arc<OutputShared>,
    path: Vec<PathKind>,
    exclusive: bool,
}

/// Sends commands to a running engine.
#[derive(Debug, Clone)]
pub struct EngineHandle {
    tx: Sender<Msg>,
}

impl std::fmt::Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Msg::Cmd(c) => write!(f, "Cmd({c:?})"),
            Msg::Output(_) => write!(f, "Output"),
            Msg::Devices => write!(f, "Devices"),
        }
    }
}

impl EngineHandle {
    /// Queues a command; false once the engine has shut down.
    pub fn send(&self, command: Command) -> bool {
        self.tx.send(Msg::Cmd(command)).is_ok()
    }
}

/// Starts the engine on `factory`'s outputs; events arrive on the returned receiver.
pub fn start(
    factory: Box<dyn OutputFactory>,
    config: EngineConfig,
) -> (EngineHandle, Receiver<Event>) {
    let (tx, rx) = mpsc::channel();
    let (etx, erx) = mpsc::channel();
    let shared = Arc::new(Shared {
        events: etx,
        urls: Mutex::default(),
        backoff: config.backoff,
    });
    let to_control = tx.clone();
    let resume_within = config.resume_within;
    let spawned = std::thread::Builder::new()
        .name("bb-audio-control".into())
        .spawn(move || Control::new(shared, factory, to_control, resume_within).run(rx));
    if let Err(e) = spawned {
        tracing::error!(target: "audio", "engine thread did not start: {e}");
    }
    (EngineHandle { tx }, erx)
}

/// State every engine thread can reach.
struct Shared {
    events: Sender<Event>,
    /// Live link slots by item id, so a fresh link reaches readers that are waiting for one.
    urls: Mutex<HashMap<String, Weak<UrlSlot>>>,
    backoff: Duration,
}

impl Shared {
    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    fn slot(&self, item: &PlayItem) -> Arc<UrlSlot> {
        let mut urls = lock(&self.urls);
        urls.retain(|_, w| w.strong_count() > 0);
        if let Some(slot) = urls.get(&item.track_id).and_then(Weak::upgrade) {
            return slot;
        }
        let slot = UrlSlot::new(&item.url);
        urls.insert(item.track_id.clone(), Arc::downgrade(&slot));
        slot
    }

    fn open_decoder(
        self: &Arc<Self>,
        item: &PlayItem,
        cancel: &Arc<AtomicBool>,
        prefetch: bool,
    ) -> Result<TrackDecoder, DecodeError> {
        let me = Arc::downgrade(self);
        let options = SourceOptions {
            track_id: item.track_id.clone(),
            url: self.slot(item),
            on_expired: Arc::new(move |id: &str| {
                if let Some(s) = me.upgrade() {
                    s.emit(Event::TokenExpired {
                        track_id: id.to_owned(),
                    });
                }
            }),
            cancel: cancel.clone(),
            backoff: self.backoff,
        };
        if item.live {
            let stream = crate::source::open_live(&options).map_err(DecodeError::Io)?;
            return TrackDecoder::open(Box::new(stream), item.format_hint.as_deref());
        }
        let mut source = HttpRangeSource::open(options).map_err(DecodeError::Io)?;
        if prefetch {
            let bytes = item
                .duration_sec
                .filter(|d| *d > 0.0)
                .map(|d| (source.len() as f64 * PREFETCH_SECONDS / d) as u64)
                .unwrap_or(4 << 20)
                .min(16 << 20);
            source.prefetch(bytes).map_err(DecodeError::Io)?;
        }
        TrackDecoder::open(Box::new(source), item.format_hint.as_deref())
    }
}

/// Where each item starts in a session's ring (device frames since the ring began).
#[derive(Debug, Clone)]
struct Segment {
    start: u64,
    track_id: String,
    start_sec: f64,
    duration: Option<f64>,
    source: Format,
    output: Format,
    path: Vec<PathKind>,
    /// Samples reach an exclusive device unchanged (before the volume and any processing).
    bit_exact: bool,
    /// The ring holds floats (sound processing on).
    float: bool,
    /// Nothing but processing touches the samples (no resampling, channel mapping or lost bits).
    passthrough: bool,
    /// The item fades in over the previous one.
    faded: bool,
    /// Where this item's EQ takes over (the middle of a crossfade).
    eq_start: u64,
}

#[derive(Debug, Default)]
struct Timeline {
    generation: u64,
    segments: Vec<Segment>,
    /// Frames in the ring once the decoder thread finished (no next item, or an error).
    end: Option<u64>,
    /// The decoder is waiting for the device to play out before reopening it at another format.
    switching: bool,
}

/// Decoded samples to device bytes: exact copy, or float → channel map → resample → encode.
struct Pipeline {
    device: DeviceFormat,
    rate: u32,
    channels: u16,
    bits: Option<u16>,
    exact: bool,
    resampler: Option<StreamResampler>,
    floats: Vec<f32>,
    mapped: Vec<f32>,
    resampled: Vec<f32>,
    /// Source frames fed since the pipeline was made.
    fed: u64,
    /// Float ring: the levelling gain now (linear).
    gain: f32,
}

impl Pipeline {
    fn new(info: &StreamInfo, device: DeviceFormat) -> Result<Self, String> {
        if info.sample_rate == 0 || info.channels == 0 {
            return Err("unknown sample rate or channel count".into());
        }
        let resampler = (info.sample_rate != device.sample_rate)
            .then(|| StreamResampler::new(info.sample_rate, device.sample_rate, device.channels))
            .transpose()?;
        Ok(Self {
            device,
            rate: info.sample_rate,
            channels: info.channels,
            bits: info.bits,
            exact: resampler.is_none() && info.channels == device.channels && device.kind.is_int(),
            resampler,
            floats: Vec::new(),
            mapped: Vec::new(),
            resampled: Vec::new(),
            fed: 0,
            gain: 1.0,
        })
    }

    /// In a float ring: no resampling or channel mapping, and floats keep every source bit (≤ 24).
    fn passthrough(&self) -> bool {
        self.resampler.is_none()
            && self.channels == self.device.channels
            && self.device.kind.is_int()
            && self.bits.is_none_or(|b| b <= 24)
    }

    /// Float ring: decoded samples to device-rate floats, the levelling gain ramped to `gain`.
    fn process_floats(
        &mut self,
        block: &[i32],
        gain: f32,
        out: &mut Vec<f32>,
    ) -> Result<(), String> {
        out.clear();
        self.fed += (block.len() / usize::from(self.channels)) as u64;
        self.floats.clear();
        self.floats.extend(block.iter().map(|s| i32_to_f32(*s)));
        map_channels(
            &self.floats,
            self.channels,
            self.device.channels,
            &mut self.mapped,
        );
        match self.resampler.as_mut() {
            Some(r) => r.push(&self.mapped, out)?,
            None => out.extend_from_slice(&self.mapped),
        }
        self.apply_gain(out, gain);
        Ok(())
    }

    fn flush_floats(&mut self, gain: f32, out: &mut Vec<f32>) -> Result<(), String> {
        out.clear();
        if let Some(r) = self.resampler.as_mut() {
            r.flush(out)?;
        }
        self.apply_gain(out, gain);
        Ok(())
    }

    fn apply_gain(&mut self, out: &mut [f32], gain: f32) {
        let (from, to) = (self.gain, gain);
        self.gain = gain;
        if from == 1.0 && to == 1.0 {
            return;
        }
        let ch = usize::from(self.device.channels.max(1));
        let frames = (out.len() / ch).max(1);
        for (i, f) in out.chunks_exact_mut(ch).enumerate() {
            let g = from + (to - from) * (i as f32 / frames as f32);
            for s in f {
                *s *= g;
            }
        }
    }

    fn compatible(&self, info: &StreamInfo) -> bool {
        info.sample_rate == self.rate && info.channels == self.channels
    }

    fn ratio(&self) -> f64 {
        f64::from(self.device.sample_rate) / f64::from(self.rate)
    }

    /// The device container keeps every source bit.
    fn keeps_all_bits(&self) -> bool {
        self.bits.is_none_or(|b| b <= self.device.kind.bits())
    }

    fn path(&self) -> Vec<PathKind> {
        let mut path = Vec::new();
        if self.resampler.is_some() {
            path.push(PathKind::Resampled {
                to_hz: self.device.sample_rate,
            });
        }
        if self.channels > self.device.channels {
            path.push(PathKind::Downmixed);
        }
        if self.exact && !self.keeps_all_bits() {
            // A device narrower than the source: the low bits are cut.
            path.push(PathKind::DspApplied);
        }
        path
    }

    fn process(&mut self, block: &[i32], out: &mut Vec<u8>) -> Result<(), String> {
        out.clear();
        self.fed += (block.len() / usize::from(self.channels)) as u64;
        if self.exact {
            encode_i32_exact(block, self.device.kind, out);
            return Ok(());
        }
        self.floats.clear();
        self.floats.extend(block.iter().map(|s| i32_to_f32(*s)));
        map_channels(
            &self.floats,
            self.channels,
            self.device.channels,
            &mut self.mapped,
        );
        match self.resampler.as_mut() {
            Some(r) => {
                self.resampled.clear();
                r.push(&self.mapped, &mut self.resampled)?;
                encode_f32(&self.resampled, self.device.kind, out);
            }
            None => encode_f32(&self.mapped, self.device.kind, out),
        }
        Ok(())
    }

    fn flush(&mut self, out: &mut Vec<u8>) -> Result<(), String> {
        out.clear();
        if let Some(r) = self.resampler.as_mut() {
            self.resampled.clear();
            r.flush(&mut self.resampled)?;
            encode_f32(&self.resampled, self.device.kind, out);
        }
        Ok(())
    }
}

type Prepared = Arc<Mutex<Option<(String, TrackDecoder)>>>;

/// Everything a decoder thread needs.
struct ProducerCtx {
    shared: Arc<Shared>,
    item: PlayItem,
    start_sec: f64,
    generation: u64,
    to_control: Sender<Msg>,
    /// Exclusive formats of the device (exclusive mode only).
    caps: Option<Arc<DeviceCaps>>,
    /// D9: resample to the nearest supported rate (else play such items in shared mode).
    nearest_rate: bool,
    /// The ring holds floats: levelling, crossfade and EQ may change the sound.
    float: bool,
    sounds: Arc<Mutex<SoundTable>>,
    cancel: Arc<AtomicBool>,
    next: Arc<Mutex<Option<PlayItem>>>,
    prepared: Prepared,
    timeline: Arc<Mutex<Timeline>>,
}

/// A ring and the device it feeds.
struct Ring {
    producer: rtrb::Producer<u8>,
    capacity: usize,
    grant: Grant,
    kind: RingKind,
    pushed: u64,
}

impl Ring {
    fn new(generation: u64, grant: Grant, kind: RingKind) -> Self {
        let fb = ring_frame_bytes(&grant.format, kind);
        let capacity =
            ((f64::from(grant.format.sample_rate) * RING_SECONDS) as usize).max(4096) * fb;
        let (producer, consumer) = rtrb::RingBuffer::<u8>::new(capacity);
        grant.shared.set_source(generation, consumer, kind);
        Self {
            producer,
            capacity,
            grant,
            kind,
            pushed: 0,
        }
    }

    fn fb(&self) -> usize {
        ring_frame_bytes(&self.grant.format, self.kind)
    }

    fn rate(&self) -> f64 {
        f64::from(self.grant.format.sample_rate)
    }

    fn channels(&self) -> usize {
        usize::from(self.grant.format.channels.max(1))
    }

    /// Pushes all of `bytes`, waiting while the ring is full; false when the session ended.
    fn push(&mut self, bytes: &[u8], cancel: &AtomicBool) -> bool {
        let mut rest = bytes;
        while !rest.is_empty() {
            if cancel.load(Ordering::Relaxed) || self.producer.is_abandoned() {
                return false;
            }
            let (_, left) = self.producer.push_partial_slice(rest);
            rest = left;
            if !rest.is_empty() {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        self.pushed += (bytes.len() / self.fb()) as u64;
        true
    }

    fn buffered(&self) -> usize {
        self.capacity - self.producer.slots()
    }
}

fn ring_frame_bytes(format: &DeviceFormat, kind: RingKind) -> usize {
    match kind {
        RingKind::Device => format.frame_bytes().max(1),
        RingKind::Float => 4 * usize::from(format.channels.max(1)),
    }
}

fn floats_to_bytes(floats: &[f32], out: &mut Vec<u8>) {
    out.clear();
    out.reserve(floats.len() * 4);
    for f in floats {
        out.extend_from_slice(&f.to_le_bytes());
    }
}

/// The next item fading in over the end of the current one.
struct Fade {
    item: PlayItem,
    dec: TrackDecoder,
    pipe: Pipeline,
    /// Its samples not mixed in yet.
    queue: VecDeque<f32>,
    /// Fade length and frames done (device frames).
    len: u64,
    pos: u64,
    /// Its device frames so far.
    produced: u64,
    /// Its decoder has nothing more (a very short item).
    ended: bool,
    scratch: Vec<f32>,
}

impl Fade {
    /// Mixes the incoming item into `out` (the outgoing item's frames, or silence once it ended).
    fn mix(&mut self, out: &mut [f32], channels: usize, gain: f32) -> Result<(), DecodeError> {
        let want = out.len();
        while self.queue.len() < want && !self.ended {
            match self.dec.next_block()? {
                Some(block) => {
                    self.pipe
                        .process_floats(block, gain, &mut self.scratch)
                        .map_err(DecodeError::Broken)?;
                    self.queue.extend(self.scratch.iter().copied());
                }
                None => {
                    self.pipe
                        .flush_floats(gain, &mut self.scratch)
                        .map_err(DecodeError::Broken)?;
                    self.queue.extend(self.scratch.iter().copied());
                    self.ended = true;
                }
            }
        }
        for frame in out.chunks_exact_mut(channels) {
            let p = if self.len == 0 {
                1.0
            } else {
                self.pos as f64 / self.len as f64
            };
            let (go, gi) = crossfade_gains(p);
            for s in frame.iter_mut() {
                let incoming = self.queue.pop_front().unwrap_or(0.0);
                *s = *s * go + incoming * gi;
            }
            self.pos += 1;
            self.produced += 1;
        }
        Ok(())
    }
}

impl ProducerCtx {
    fn take_prepared(&self, track_id: &str) -> Option<TrackDecoder> {
        let mut p = lock(&self.prepared);
        if p.as_ref().is_some_and(|(id, _)| id == track_id) {
            p.take().map(|(_, d)| d)
        } else {
            None
        }
    }

    fn open(&self, item: &PlayItem) -> Result<TrackDecoder, DecodeError> {
        match self.take_prepared(&item.track_id) {
            Some(d) => Ok(d),
            None => self.shared.open_decoder(item, &self.cancel, false),
        }
    }

    /// The exclusive format for a stream (`None`: shared mode, or nothing usable on the device).
    fn want(&self, info: &StreamInfo) -> Option<DeviceFormat> {
        self.caps
            .as_ref()
            .and_then(|c| negotiate::choose(c, info))
            .filter(|f| self.nearest_rate || f.sample_rate == info.sample_rate)
    }

    /// Asks the control thread for the device; `None` when the session ended or the device can't open.
    fn acquire(&self, format: Option<DeviceFormat>) -> Option<Grant> {
        let (reply, answer) = mpsc::channel();
        self.to_control
            .send(Msg::Output(OutputAsk {
                session: self.cancel.clone(),
                format,
                reply,
            }))
            .ok()?;
        loop {
            match answer.recv_timeout(Duration::from_millis(100)) {
                Ok(grant) => return grant,
                Err(RecvTimeoutError::Timeout) if !self.cancel.load(Ordering::Relaxed) => {}
                Err(_) => return None,
            }
        }
    }

    /// The item's levelling gain (linear) right now.
    fn gain(&self, item: &PlayItem) -> f32 {
        db_to_linear(lock(&self.sounds).gain_db(&item.track_id)) as f32
    }

    fn crossfade_sec(&self) -> f64 {
        lock(&self.sounds).crossfade_sec()
    }

    /// Starts a crossfade into the next item when the current one is about to end: same device format only
    /// (an exclusive rate change stays a clean switch), and a next item must be set.
    fn start_fade(&self, dec: &TrackDecoder, ring: &Ring, item_frames: u64) -> Option<Fade> {
        let fade_sec = self.crossfade_sec();
        let total = dec.info.duration_sec()? * ring.rate();
        let left = total - item_frames as f64;
        if fade_sec <= 0.0 || left > fade_sec * ring.rate() || left < MIN_FADE_SEC * ring.rate() {
            return None;
        }
        let next = lock(&self.next).take()?;
        let next_dec = match self.open(&next) {
            Ok(d) => d,
            Err(_) => {
                // Leave it for the normal join, which reports the failure.
                *lock(&self.next) = Some(next);
                return None;
            }
        };
        let fits = !ring.grant.exclusive || self.want(&next_dec.info) == Some(ring.grant.format);
        let pipe = Pipeline::new(&next_dec.info, ring.grant.format)
            .ok()
            .filter(|_| fits);
        let Some(pipe) = pipe else {
            // Another rate: no fade. Give the opened decoder to the normal join.
            *lock(&self.prepared) = Some((next.track_id.clone(), next_dec));
            *lock(&self.next) = Some(next);
            return None;
        };
        Some(Fade {
            item: next,
            dec: next_dec,
            pipe,
            queue: VecDeque::new(),
            len: left.max(0.0) as u64,
            pos: 0,
            produced: 0,
            ended: false,
            scratch: Vec::new(),
        })
    }

    fn timeline(&self) -> Option<MutexGuard<'_, Timeline>> {
        let tl = lock(&self.timeline);
        (tl.generation == self.generation).then_some(tl)
    }

    #[allow(clippy::too_many_arguments)]
    fn add_segment(
        &self,
        item: &PlayItem,
        info: &StreamInfo,
        pipe: &Pipeline,
        ring: &Ring,
        start: u64,
        start_sec: f64,
        fade_len: Option<u64>,
    ) {
        let mut path = ring.grant.path.clone();
        path.extend(pipe.path());
        if item.server_decoded {
            path.push(PathKind::ServerConverted);
        }
        let float = ring.kind == RingKind::Float;
        let passthrough = if float {
            pipe.passthrough()
        } else {
            pipe.exact && pipe.keeps_all_bits()
        };
        let bit_exact = ring.grant.exclusive && passthrough;
        if let Some(mut tl) = self.timeline() {
            tl.switching = false;
            tl.segments.push(Segment {
                start,
                track_id: item.track_id.clone(),
                start_sec,
                duration: info.duration_sec().or(item.duration_sec),
                source: Format {
                    sample_rate: info.sample_rate,
                    bits: info.bits,
                    channels: info.channels,
                    codec: info.codec.clone(),
                },
                output: ring.grant.format.to_format(),
                path,
                bit_exact,
                float,
                passthrough,
                faded: fade_len.is_some(),
                eq_start: start + fade_len.unwrap_or(0) / 2,
            });
        }
    }

    fn finish(&self, frames: u64) {
        if let Some(mut tl) = self.timeline() {
            tl.end = Some(frames);
        }
    }

    fn fail(&self, track_id: &str, code: ErrorCode, message: String, frames: u64) {
        if !self.cancel.load(Ordering::Relaxed) {
            tracing::warn!(target: "audio", track_id, ?code, "{message}");
            self.shared.emit(Event::Error {
                track_id: Some(track_id.to_owned()),
                code,
                message,
            });
        }
        self.finish(frames);
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

fn run_producer(mut ctx: ProducerCtx) {
    let mut item = ctx.item.clone();
    let mut dec = match ctx.open(&item) {
        Ok(d) => d,
        Err(e) if e.cancelled() => return,
        Err(e) => return ctx.fail(&item.track_id, e.code(), e.to_string(), 0),
    };
    let mut start_sec = 0.0;
    if ctx.start_sec > 0.0 && !item.live {
        match dec.seek(ctx.start_sec) {
            Ok(at) => start_sec = at,
            Err(e) => return ctx.fail(&item.track_id, e.code(), e.to_string(), 0),
        }
    }
    let Some(grant) = ctx.acquire(ctx.want(&dec.info)) else {
        return;
    };
    ctx.generation = grant.generation;
    let kind = if ctx.float {
        RingKind::Float
    } else {
        RingKind::Device
    };
    let mut ring = Ring::new(ctx.generation, grant, kind);
    let mut pipe = match Pipeline::new(&dec.info, ring.grant.format) {
        Ok(p) => p,
        Err(e) => return ctx.fail(&item.track_id, ErrorCode::FormatUnsupported, e, 0),
    };
    ctx.add_segment(&item, &dec.info, &pipe, &ring, 0, start_sec, None);
    let mut pipe_base = 0u64;
    let mut bytes = Vec::new();
    let mut floats = Vec::new();
    let channels = ring.channels();
    // Device frames of the current item from its start (the seek point counts), for the crossfade.
    let mut item_frames = (start_sec * ring.rate()) as u64;
    let mut fade: Option<Fade> = None;

    loop {
        if ctx.cancelled() {
            return;
        }
        if ctx.float && fade.is_none() && !item.live {
            if let Some(f) = ctx.start_fade(&dec, &ring, item_frames) {
                ctx.add_segment(
                    &f.item,
                    &f.dec.info,
                    &f.pipe,
                    &ring,
                    ring.pushed,
                    0.0,
                    Some(f.len),
                );
                fade = Some(f);
            }
        }
        match dec.next_block() {
            Ok(Some(block)) => {
                if ctx.float {
                    let gain = ctx.gain(&item);
                    if let Err(e) = pipe.process_floats(block, gain, &mut floats) {
                        return ctx.fail(&item.track_id, ErrorCode::Decode, e, ring.pushed);
                    }
                    item_frames += (floats.len() / channels) as u64;
                    if let Some(f) = fade.as_mut() {
                        let gain = ctx.gain(&f.item);
                        if let Err(e) = f.mix(&mut floats, channels, gain) {
                            if e.cancelled() {
                                return;
                            }
                            return ctx.fail(
                                &f.item.track_id,
                                e.code(),
                                e.to_string(),
                                ring.pushed,
                            );
                        }
                    }
                    floats_to_bytes(&floats, &mut bytes);
                } else if let Err(e) = pipe.process(block, &mut bytes) {
                    return ctx.fail(&item.track_id, ErrorCode::Decode, e, ring.pushed);
                }
                if !ring.push(&bytes, &ctx.cancel) {
                    return;
                }
            }
            Ok(None) => {
                // Get the resampler's last frames out first, so the item is whole even if no next one comes.
                let flushed = pipe.resampler.is_some();
                if flushed {
                    if ctx.float {
                        let gain = ctx.gain(&item);
                        let _ = pipe.flush_floats(gain, &mut floats);
                        if let Some(f) = fade.as_mut() {
                            let gain = ctx.gain(&f.item);
                            let _ = f.mix(&mut floats, channels, gain);
                        }
                        floats_to_bytes(&floats, &mut bytes);
                    } else {
                        let _ = pipe.flush(&mut bytes);
                    }
                    if !ring.push(&bytes, &ctx.cancel) {
                        return;
                    }
                }
                if let Some(mut f) = fade.take() {
                    // The outgoing item ended: finish the fade-in on its own, then carry on with it.
                    while f.pos < f.len {
                        let frames = (f.len - f.pos).min(4096) as usize;
                        floats.clear();
                        floats.resize(frames * channels, 0.0);
                        let gain = ctx.gain(&f.item);
                        if let Err(e) = f.mix(&mut floats, channels, gain) {
                            if e.cancelled() {
                                return;
                            }
                            return ctx.fail(
                                &f.item.track_id,
                                e.code(),
                                e.to_string(),
                                ring.pushed,
                            );
                        }
                        floats_to_bytes(&floats, &mut bytes);
                        if !ring.push(&bytes, &ctx.cancel) {
                            return;
                        }
                    }
                    // Whatever it decoded beyond the fade goes out as it is.
                    floats.clear();
                    floats.extend(f.queue.drain(..));
                    if !floats.is_empty() {
                        f.produced += (floats.len() / channels) as u64;
                        floats_to_bytes(&floats, &mut bytes);
                        if !ring.push(&bytes, &ctx.cancel) {
                            return;
                        }
                    }
                    let Fade {
                        item: next_item,
                        dec: next_dec,
                        pipe: next_pipe,
                        produced,
                        ..
                    } = f;
                    // Its segment and pipeline started where the fade began.
                    pipe_base = ring.pushed.saturating_sub(produced);
                    item_frames = produced;
                    pipe = next_pipe;
                    dec = next_dec;
                    item = next_item;
                    continue;
                }
                // A next item may still arrive while the ring has sound left in it.
                let low = (ring.rate() * END_WAIT_SEC) as usize * ring.fb();
                let next = loop {
                    if let Some(n) = lock(&ctx.next).take() {
                        break Some(n);
                    }
                    if ctx.cancelled() {
                        return;
                    }
                    if ring.buffered() < low || ring.producer.is_abandoned() {
                        break None;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                };
                let (next_item, next_dec) = match next.map(|n| {
                    let d = ctx.open(&n);
                    (n, d)
                }) {
                    Some((n, Ok(d))) => (n, d),
                    other => {
                        if let Some((n, Err(e))) = other {
                            if e.cancelled() {
                                return;
                            }
                            ctx.shared.emit(Event::Error {
                                track_id: Some(n.track_id.clone()),
                                code: e.code(),
                                message: e.to_string(),
                            });
                        }
                        return ctx.finish(ring.pushed);
                    }
                };
                let want = ctx.want(&next_dec.info);
                if ring.grant.exclusive && want != Some(ring.grant.format) {
                    // D8: another format. Let the device play out, then reopen it at the new one.
                    if let Some(mut tl) = ctx.timeline() {
                        tl.switching = true;
                    }
                    let generation = ctx.generation;
                    while ring.grant.shared.played(generation).unwrap_or(0) < ring.pushed {
                        if ctx.cancelled() {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    let Some(grant) = ctx.acquire(want) else {
                        return;
                    };
                    ctx.generation = grant.generation;
                    ring = Ring::new(ctx.generation, grant, kind);
                    pipe = match Pipeline::new(&next_dec.info, ring.grant.format) {
                        Ok(p) => p,
                        Err(e) => {
                            return ctx.fail(
                                &next_item.track_id,
                                ErrorCode::FormatUnsupported,
                                e,
                                0,
                            )
                        }
                    };
                    pipe_base = 0;
                } else if flushed || !pipe.compatible(&next_dec.info) {
                    // A drained resampler or a different rate/channel count: a fresh pipeline.
                    pipe = match Pipeline::new(&next_dec.info, ring.grant.format) {
                        Ok(p) => p,
                        Err(e) => {
                            return ctx.fail(
                                &next_item.track_id,
                                ErrorCode::FormatUnsupported,
                                e,
                                ring.pushed,
                            )
                        }
                    };
                    pipe_base = ring.pushed;
                }
                // Where the next item starts: after the frames already fed (resampled ones counted at the
                // output rate, the resampler's delay already trimmed).
                let start = pipe_base + (pipe.fed as f64 * pipe.ratio()).round() as u64;
                ctx.add_segment(&next_item, &next_dec.info, &pipe, &ring, start, 0.0, None);
                dec = next_dec;
                item = next_item;
                item_frames = 0;
            }
            Err(e) if e.cancelled() => return,
            Err(e) => {
                let _ = pipe.flush(&mut bytes);
                let _ = ring.push(&bytes, &ctx.cancel);
                return ctx.fail(&item.track_id, e.code(), e.to_string(), ring.pushed);
            }
        }
    }
}

/// Playback interrupted by a lost device, waiting for it to come back.
struct Lost {
    track_id: String,
    position: f64,
    was_playing: bool,
    since: Instant,
    last_try: Instant,
}

/// Keeps Windows from sleeping while music plays (plan §8.5).
fn keep_awake(on: bool) {
    #[cfg(windows)]
    {
        use windows::Win32::System::Power::{
            SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
        };
        let flags = if on {
            ES_CONTINUOUS | ES_SYSTEM_REQUIRED
        } else {
            ES_CONTINUOUS
        };
        // SAFETY: a plain Win32 call with valid flags; it only changes this thread's execution state.
        unsafe {
            SetThreadExecutionState(flags);
        }
    }
    #[cfg(not(windows))]
    let _ = on;
}

/// The control thread's state.
struct Control {
    shared: Arc<Shared>,
    factory: Box<dyn OutputFactory>,
    to_control: Sender<Msg>,
    _watch: Option<Box<dyn WatchGuard>>,
    output: Option<Box<dyn Output>>,
    device_id: Option<String>,
    mode: OutputMode,
    buffer_ms: u32,
    nearest_rate: bool,
    caps: Option<Result<Arc<DeviceCaps>, OutputError>>,
    generation: u64,
    session: Option<Arc<AtomicBool>>,
    next: Arc<Mutex<Option<PlayItem>>>,
    prepared: Prepared,
    timeline: Arc<Mutex<Timeline>>,
    items: HashMap<String, PlayItem>,
    status: Status,
    paused: bool,
    volume: f32,
    current: Option<String>,
    /// The segment playing now and the path last reported for it.
    current_seg: Option<Segment>,
    /// Ring generation and timeline index of the segment last announced with `TrackStarted`.
    announced: Option<(u64, usize)>,
    last_path: Option<Vec<PathKind>>,
    sounds: Arc<Mutex<SoundTable>>,
    /// The ring generation and segment count the output's EQ table was built from.
    published: (u64, usize),
    wave: Vec<f32>,
    last_levels: Instant,
    position: f64,
    duration: Option<f64>,
    need_next_sent: Option<String>,
    last_state: Instant,
    underruns: u64,
    lost: Option<Lost>,
    resume_within: Duration,
    awake: bool,
}

impl Control {
    fn new(
        shared: Arc<Shared>,
        factory: Box<dyn OutputFactory>,
        to_control: Sender<Msg>,
        resume_within: Duration,
    ) -> Self {
        let notify_tx = to_control.clone();
        let watch = factory.watch(Arc::new(move || {
            let _ = notify_tx.send(Msg::Devices);
        }));
        Self {
            shared,
            factory,
            to_control,
            _watch: watch,
            output: None,
            device_id: None,
            mode: OutputMode::Shared,
            buffer_ms: DEFAULT_BUFFER_MS,
            nearest_rate: true,
            caps: None,
            generation: 0,
            session: None,
            next: Arc::default(),
            prepared: Arc::default(),
            timeline: Arc::default(),
            items: HashMap::new(),
            status: Status::Idle,
            paused: false,
            volume: 1.0,
            current: None,
            current_seg: None,
            announced: None,
            last_path: None,
            sounds: Arc::default(),
            published: (u64::MAX, 0),
            wave: Vec::with_capacity(LEVEL_POINTS),
            last_levels: Instant::now(),
            position: 0.0,
            duration: None,
            need_next_sent: None,
            last_state: Instant::now(),
            underruns: 0,
            lost: None,
            resume_within,
            awake: false,
        }
    }

    fn run(mut self, rx: Receiver<Msg>) {
        loop {
            match rx.recv_timeout(TICK) {
                Ok(Msg::Cmd(cmd)) => {
                    if !self.handle(cmd) {
                        break;
                    }
                }
                Ok(Msg::Output(ask)) => self.grant(ask),
                Ok(Msg::Devices) => self.devices_changed(),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            self.tick();
            let awake = matches!(self.status, Status::Playing | Status::Loading);
            if awake != self.awake {
                self.awake = awake;
                keep_awake(awake);
            }
        }
        self.end_session();
        self.output = None;
        keep_awake(false);
    }

    fn emit(&self, event: Event) {
        self.shared.emit(event);
    }

    fn emit_state(&mut self) {
        self.last_state = Instant::now();
        self.emit(Event::State {
            status: self.status,
            track_id: self.current.clone(),
            position_sec: self.position,
            duration_sec: self.duration,
        });
    }

    fn set_status(&mut self, status: Status) {
        if self.status != status {
            self.status = status;
            self.emit_state();
        }
    }

    fn error(&self, code: ErrorCode, message: impl Into<String>) {
        self.emit(Event::Error {
            track_id: self.current.clone(),
            code,
            message: message.into(),
        });
    }

    fn end_session(&mut self) {
        if let Some(cancel) = self.session.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        *lock(&self.prepared) = None;
    }

    /// The device's exclusive formats, probed once per device (and again after a device change).
    fn exclusive_caps(&mut self) -> Option<Arc<DeviceCaps>> {
        if self.caps.is_none() {
            // Probing a device we hold in exclusive mode would fail: let go of it first.
            if self.output.as_ref().is_some_and(|o| o.exclusive()) {
                self.output = None;
            }
            self.caps = Some(
                self.factory
                    .exclusive_caps(self.device_id.as_deref())
                    .map(Arc::new),
            );
        }
        match self.caps.as_ref() {
            Some(Ok(c)) => Some(c.clone()),
            Some(Err(e)) => {
                self.error(
                    e.code,
                    format!("{}; playing in shared mode instead", e.message),
                );
                None
            }
            None => None,
        }
    }

    /// Starts a session at `start_sec` of `item`; the next slot is left as it is.
    fn begin(&mut self, item: PlayItem, start_sec: f64) {
        self.end_session();
        self.lost = None;
        // Fresh slots per session: a cancelled decoder thread still waiting at the end of its item must not
        // take the next item meant for this one.
        let carried = lock(&self.next).clone();
        self.next = Arc::new(Mutex::new(carried));
        self.prepared = Arc::default();
        let caps = match self.mode {
            OutputMode::Exclusive => self.exclusive_caps(),
            OutputMode::Shared => None,
        };
        self.generation += 1;
        *lock(&self.timeline) = Timeline {
            generation: self.generation,
            ..Timeline::default()
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let ctx = ProducerCtx {
            shared: self.shared.clone(),
            item: item.clone(),
            start_sec,
            generation: self.generation,
            to_control: self.to_control.clone(),
            caps,
            nearest_rate: self.nearest_rate,
            float: lock(&self.sounds).processing(),
            sounds: self.sounds.clone(),
            cancel: cancel.clone(),
            next: self.next.clone(),
            prepared: self.prepared.clone(),
            timeline: self.timeline.clone(),
        };
        let spawned = std::thread::Builder::new()
            .name("bb-audio-decode".into())
            .spawn(move || run_producer(ctx));
        if let Err(e) = spawned {
            self.error(
                ErrorCode::Decode,
                format!("decoder thread did not start: {e}"),
            );
            return;
        }
        self.session = Some(cancel);
        self.remember(&item);
        self.current = None;
        self.current_seg = None;
        self.last_path = None;
        self.published = (u64::MAX, 0);
        self.need_next_sent = None;
        self.position = start_sec;
        self.duration = None;
        self.status = Status::Loading;
        self.emit_state();
        self.prepare_next();
    }

    /// Opens (or reuses) the device for a decoder thread.
    fn grant(&mut self, ask: OutputAsk) {
        let live = self
            .session
            .as_ref()
            .is_some_and(|s| Arc::ptr_eq(s, &ask.session))
            && !ask.session.load(Ordering::Relaxed);
        if !live {
            let _ = ask.reply.send(None);
            return;
        }
        let exclusive = self.mode == OutputMode::Exclusive && ask.format.is_some();
        let reusable = self.output.as_ref().is_some_and(|o| {
            if exclusive {
                o.exclusive() && Some(o.format()) == ask.format
            } else {
                !o.exclusive()
            }
        });
        if !reusable {
            let mid_session = !lock(&self.timeline).segments.is_empty();
            // An exclusive stream holds the device: close it before opening another.
            self.output = None;
            let request = OutputRequest {
                mode: if exclusive {
                    OutputMode::Exclusive
                } else {
                    OutputMode::Shared
                },
                format: ask.format,
                buffer_ms: self.buffer_ms,
            };
            let opened = match self.factory.open(self.device_id.as_deref(), &request) {
                Ok(o) => Ok(o),
                Err(e) if exclusive => {
                    // Refused (in use, not allowed, format): say so and play shared, never silently.
                    self.error(
                        e.code,
                        format!("{}; playing in shared mode instead", e.message),
                    );
                    let shared = OutputRequest {
                        mode: OutputMode::Shared,
                        format: None,
                        buffer_ms: self.buffer_ms,
                    };
                    self.factory.open(self.device_id.as_deref(), &shared)
                }
                Err(e) => Err(e),
            };
            match opened {
                Ok(out) => {
                    out.shared().set_volume(self.volume);
                    self.underruns = 0;
                    self.output = Some(out);
                    if mid_session {
                        // A new ring on a new device stream: positions start again from its frames.
                        self.generation += 1;
                        let mut tl = lock(&self.timeline);
                        *tl = Timeline {
                            generation: self.generation,
                            ..Timeline::default()
                        };
                    }
                }
                Err(OutputError { code, message }) => {
                    self.error(code, message);
                    let _ = ask.reply.send(None);
                    self.end_session();
                    self.current = None;
                    self.set_status(Status::Stopped);
                    return;
                }
            }
        }
        let Some(out) = self.output.as_ref() else {
            let _ = ask.reply.send(None);
            return;
        };
        out.shared().set_paused(self.paused);
        let _ = ask.reply.send(Some(Grant {
            generation: self.generation,
            format: out.format(),
            shared: out.shared().clone(),
            path: out.path(),
            exclusive: out.exclusive(),
        }));
    }

    /// Opens the next item ahead of time (prefetch) for the current session.
    fn prepare_next(&self) {
        let Some(item) = lock(&self.next).clone() else {
            return;
        };
        let Some(cancel) = self.session.clone() else {
            return;
        };
        let (shared, prepared, next) = (
            self.shared.clone(),
            self.prepared.clone(),
            self.next.clone(),
        );
        let _ = std::thread::Builder::new()
            .name("bb-audio-prefetch".into())
            .spawn(move || {
                if let Ok(dec) = shared.open_decoder(&item, &cancel, true) {
                    let still_next = lock(&next)
                        .as_ref()
                        .is_some_and(|n| n.track_id == item.track_id);
                    if still_next && !cancel.load(Ordering::Relaxed) {
                        *lock(&prepared) = Some((item.track_id.clone(), dec));
                    }
                }
            });
    }

    /// Restarts the current item at `sec` (seek, output change, a changed next item after the decoder moved on).
    fn restart(&mut self, sec: f64, next: Option<Option<PlayItem>>) {
        let Some(id) = self.current.clone().or_else(|| self.first_track()) else {
            return;
        };
        let Some(item) = self.items.get(&id).cloned() else {
            return;
        };
        let next =
            next.unwrap_or_else(|| lock(&self.next).clone().or_else(|| self.committed_next()));
        *lock(&self.next) = next;
        self.begin(item, sec);
    }

    fn first_track(&self) -> Option<String> {
        let tl = lock(&self.timeline);
        tl.segments.first().map(|s| s.track_id.clone())
    }

    /// The item the decoder already moved on to, which hasn't reached the device yet.
    fn committed_next(&self) -> Option<PlayItem> {
        let tl = lock(&self.timeline);
        let current = self.current.as_deref()?;
        let idx = tl.segments.iter().rposition(|s| s.track_id == current)?;
        let next = tl.segments.get(idx + 1)?;
        self.items.get(&next.track_id).cloned()
    }

    /// Keeps an item and its levelling gain and own curve.
    fn remember(&mut self, item: &PlayItem) {
        self.items.insert(item.track_id.clone(), item.clone());
        lock(&self.sounds).items.insert(
            item.track_id.clone(),
            ItemSound {
                gain_db: item.gain_db,
                eq: item.eq.clone(),
            },
        );
    }

    /// The device rate now (the nominal rate before a device is open).
    fn rate(&self) -> u32 {
        self.output
            .as_ref()
            .map_or(NOMINAL_RATE, |o| o.format().sample_rate)
    }

    /// What happens to a segment's samples: the format path, then the sound processing in use, else
    /// bit-perfect (or the volume) when nothing changes them.
    fn full_path(&self, seg: &Segment) -> Vec<PathKind> {
        let mut changes = Vec::new();
        if seg.float {
            let sounds = lock(&self.sounds);
            if !EqPlan::new(sounds.eq(&seg.track_id), self.rate()).is_identity() {
                changes.push(PathKind::EqApplied);
            }
            if sounds.gain_db(&seg.track_id).abs() > 1e-6 {
                changes.push(PathKind::Levelled);
            }
            if sounds.crossfade_sec() > 0.0 {
                changes.push(PathKind::Crossfade);
            }
        }
        let mut path = seg.path.clone();
        let audible = changes
            .iter()
            .any(|c| matches!(c, PathKind::EqApplied | PathKind::Levelled));
        if seg.bit_exact && !audible {
            path.insert(
                0,
                if self.volume >= 1.0 {
                    PathKind::BitPerfect
                } else {
                    PathKind::VolumeApplied
                },
            );
        }
        path.extend(changes);
        path
    }

    /// Tells the host when the current item's path changed (volume, EQ, levelling).
    fn update_path(&mut self) {
        let (Some(id), Some(seg)) = (self.current.clone(), self.current_seg.clone()) else {
            return;
        };
        let path = self.full_path(&seg);
        if self.last_path.as_ref() != Some(&path) {
            self.last_path = Some(path.clone());
            self.emit(Event::PathChanged { track_id: id, path });
        }
    }

    /// Gives the output the EQ of the items in its ring (from the one playing on).
    fn publish_plans(&mut self, force: bool) {
        let Some(out) = self.output.as_ref() else {
            return;
        };
        let rate = out.format().sample_rate;
        let played = out.shared().played(self.generation).unwrap_or(0);
        let tl = lock(&self.timeline);
        if tl.generation != self.generation || tl.segments.is_empty() {
            return;
        }
        if !tl.segments.iter().any(|s| s.float) {
            return;
        }
        let key = (tl.generation, tl.segments.len());
        if !force && self.published == key {
            return;
        }
        let first = tl
            .segments
            .iter()
            .rposition(|s| s.start <= played)
            .unwrap_or(0);
        let sounds = lock(&self.sounds);
        let mut table = PlanTable::new(tl.generation);
        for seg in tl.segments.iter().skip(first).take(MAX_PLANS) {
            let plan = EqPlan::new(sounds.eq(&seg.track_id), rate);
            let changed = !seg.passthrough || seg.faded || sounds.gain_db(&seg.track_id) != 0.0;
            table.push(seg.eq_start, plan, changed);
        }
        out.shared().set_plans(table);
        drop(sounds);
        drop(tl);
        self.published = key;
    }

    /// Sends the VU meter's levels while playing.
    fn send_levels(&mut self) {
        if self.status != Status::Playing || self.last_levels.elapsed() < LEVELS_EVERY {
            return;
        }
        self.last_levels = Instant::now();
        let Some(out) = self.output.as_ref() else {
            return;
        };
        let Some((peak, rms)) = out.shared().take_levels(LEVEL_POINTS, &mut self.wave) else {
            return;
        };
        let wave = self
            .wave
            .iter()
            .map(|v| (v * 127.0).round().clamp(-127.0, 127.0) as i8)
            .collect();
        self.emit(Event::Levels { peak, rms, wave });
    }

    fn handle(&mut self, cmd: Command) -> bool {
        match cmd {
            Command::Load {
                item,
                start_sec,
                autoplay,
            } => {
                *lock(&self.next) = None;
                self.paused = !autoplay;
                self.begin(item, start_sec.max(0.0));
            }
            Command::SetNext { item } => self.set_next(item),
            Command::Play => {
                self.paused = false;
                if let Some(out) = self.output.as_ref() {
                    out.shared().set_paused(false);
                }
                if let Some(lost) = self.lost.as_mut() {
                    lost.was_playing = true;
                } else if self.session.is_some() {
                    let s = if self.current.is_some() {
                        Status::Playing
                    } else {
                        Status::Loading
                    };
                    self.set_status(s);
                }
            }
            Command::Pause => {
                self.paused = true;
                if let Some(out) = self.output.as_ref() {
                    out.shared().set_paused(true);
                }
                if let Some(lost) = self.lost.as_mut() {
                    lost.was_playing = false;
                } else if self.session.is_some() {
                    self.set_status(Status::Paused);
                }
            }
            Command::Stop => {
                self.end_session();
                self.lost = None;
                *lock(&self.next) = None;
                self.silence();
                self.current = None;
                self.set_status(Status::Stopped);
            }
            Command::Seek { sec } => {
                if self.session.is_some() {
                    self.restart(sec.max(0.0), None);
                } else if let Some(lost) = self.lost.as_mut() {
                    lost.position = sec.max(0.0);
                }
            }
            Command::SetVolume { linear } => {
                let was_full = self.volume >= 1.0;
                self.volume = linear.clamp(0.0, 1.0) as f32;
                if let Some(out) = self.output.as_ref() {
                    out.shared().set_volume(self.volume);
                }
                if was_full != (self.volume >= 1.0) {
                    self.update_path();
                }
            }
            Command::SetSound { sound } => {
                let was = lock(&self.sounds).processing();
                lock(&self.sounds).settings = sound;
                let now = lock(&self.sounds).processing();
                if was != now && self.session.is_some() {
                    // The ring must switch between device bytes and floats: pick up from here.
                    self.restart(self.position, None);
                } else {
                    self.publish_plans(true);
                    self.update_path();
                }
            }
            Command::SetItemSound {
                track_id,
                gain_db,
                eq,
            } => {
                if let Some(item) = self.items.get_mut(&track_id) {
                    item.gain_db = gain_db;
                    item.eq = eq.clone();
                }
                lock(&self.sounds)
                    .items
                    .insert(track_id, ItemSound { gain_db, eq });
                self.publish_plans(true);
                self.update_path();
            }
            Command::SetOutput {
                device_id,
                mode,
                buffer_ms,
                nearest_rate,
            } => {
                let buffer_ms = buffer_ms.clamp(20, 200);
                if device_id == self.device_id
                    && mode == self.mode
                    && buffer_ms == self.buffer_ms
                    && nearest_rate == self.nearest_rate
                {
                    return true;
                }
                self.nearest_rate = nearest_rate;
                let resume = self.session.is_some();
                let at = self.position;
                self.end_session();
                self.output = None;
                if device_id != self.device_id {
                    self.caps = None;
                }
                self.device_id = device_id;
                self.mode = mode;
                self.buffer_ms = buffer_ms;
                if resume {
                    self.restart(at, None);
                } else if mode == OutputMode::Exclusive {
                    // Report "exclusive not allowed" now rather than at the next play.
                    let _ = self.exclusive_caps();
                }
            }
            Command::RefreshUrl { track_id, url } => {
                if let Some(item) = self.items.get_mut(&track_id) {
                    item.url = url.clone();
                }
                if let Some(n) = lock(&self.next).as_mut().filter(|n| n.track_id == track_id) {
                    n.url = url.clone();
                }
                if let Some(slot) = lock(&self.shared.urls)
                    .get(&track_id)
                    .and_then(Weak::upgrade)
                {
                    slot.set(&url);
                }
            }
            Command::Shutdown => return false,
        }
        true
    }

    fn silence(&mut self) {
        if let Some(out) = self.output.as_ref() {
            self.generation += 1;
            let (_p, c) = rtrb::RingBuffer::<u8>::new(4);
            out.shared()
                .set_source(self.generation, c, RingKind::Device);
            let mut tl = lock(&self.timeline);
            *tl = Timeline {
                generation: self.generation,
                ..Timeline::default()
            };
        }
    }

    fn set_next(&mut self, item: Option<PlayItem>) {
        if let Some(i) = item.as_ref() {
            self.remember(i);
        }
        let (committed, ended, switching) = {
            let tl = lock(&self.timeline);
            let committed = self
                .current
                .as_deref()
                .and_then(|c| tl.segments.iter().rposition(|s| s.track_id == c))
                .and_then(|idx| tl.segments.get(idx + 1))
                .map(|s| s.track_id.clone());
            (committed, tl.end.is_some(), tl.switching)
        };
        let active = self.session.is_some() && self.current.is_some();
        match (&committed, &item) {
            (Some(c), Some(i)) if *c == i.track_id => {}
            // The decoder already moved on to another item, or finished: redo from here with the new next.
            (Some(_), _) if active => self.restart(self.position, Some(item)),
            (None, Some(_)) if (ended || switching) && active => {
                self.restart(self.position, Some(item))
            }
            _ => {
                *lock(&self.next) = item;
                *lock(&self.prepared) = None;
                self.need_next_sent = None;
                self.prepare_next();
            }
        }
    }

    /// Device list changed: tell the host, forget probed formats, and resume if the lost device is back.
    fn devices_changed(&mut self) {
        self.caps = None;
        self.emit(Event::DeviceChanged {
            devices: self.factory.devices(),
            active: self.device_id.clone(),
        });
        if let Some(lost) = self.lost.as_mut() {
            lost.last_try = Instant::now() - RESUME_RETRY;
        }
    }

    fn device_present(&self) -> bool {
        let devices = self.factory.devices();
        match self.device_id.as_deref() {
            Some(id) => devices.iter().any(|d| d.id == id),
            None => devices.iter().any(|d| d.is_default),
        }
    }

    fn check_lost(&mut self) {
        let Some(lost) = self.lost.as_ref() else {
            return;
        };
        if lost.since.elapsed() > self.resume_within {
            self.lost = None;
            self.current = None;
            self.set_status(Status::Stopped);
            return;
        }
        if lost.last_try.elapsed() < RESUME_RETRY {
            return;
        }
        if let Some(l) = self.lost.as_mut() {
            l.last_try = Instant::now();
        }
        if !self.device_present() {
            return;
        }
        let Some(lost) = self.lost.take() else {
            return;
        };
        let Some(item) = self.items.get(&lost.track_id).cloned() else {
            return;
        };
        tracing::info!(target: "audio", "the device is back: resuming");
        self.caps = None;
        self.paused = !lost.was_playing;
        self.begin(item, lost.position);
    }

    fn tick(&mut self) {
        self.check_lost();
        let failed_before_start = self.session.is_some() && {
            let tl = lock(&self.timeline);
            tl.generation == self.generation && tl.segments.is_empty() && tl.end.is_some()
        };
        if failed_before_start {
            // The first item never opened (its error was already sent); the device may not even be open.
            self.end_session();
            self.current = None;
            self.set_status(Status::Stopped);
            return;
        }
        let Some(out) = self.output.as_ref() else {
            return;
        };
        if let Some(OutputError { code, message }) = out.shared().take_error() {
            self.error(code, message);
            let interrupted = self.session.is_some() && self.current.is_some();
            self.end_session();
            self.output = None;
            if interrupted && code == ErrorCode::DeviceLost {
                if let Some(id) = self.current.clone() {
                    self.lost = Some(Lost {
                        track_id: id,
                        position: self.position,
                        was_playing: !self.paused,
                        since: Instant::now(),
                        last_try: Instant::now(),
                    });
                }
                self.set_status(Status::Paused);
            } else {
                self.current = None;
                self.set_status(Status::Stopped);
            }
            return;
        }
        let underruns = out.shared().underruns();
        if underruns > self.underruns {
            tracing::warn!(
                target: "audio",
                count = underruns - self.underruns,
                track_id = self.current.as_deref().unwrap_or(""),
                "the device ran out of sound (dropout)"
            );
            self.underruns = underruns;
        }
        if self.session.is_none() {
            return;
        }
        let rate = f64::from(out.format().sample_rate.max(1));
        let played = out.shared().played(self.generation);
        let (seg, idx, skipped, is_last, end, empty_end, switching) = {
            let tl = lock(&self.timeline);
            if tl.generation != self.generation {
                return;
            }
            if tl.segments.is_empty() {
                (
                    None,
                    0,
                    Vec::new(),
                    true,
                    tl.end,
                    tl.end.is_some(),
                    tl.switching,
                )
            } else {
                let Some(played) = played else { return };
                let idx = tl
                    .segments
                    .iter()
                    .rposition(|s| s.start <= played)
                    .unwrap_or(0);
                // Items that played out entirely between two ticks (a stalled engine thread, very short items).
                let from = match self.announced {
                    Some((g, i)) if g == self.generation => i + 1,
                    _ => 0,
                };
                (
                    tl.segments.get(idx).cloned(),
                    idx,
                    tl.segments
                        .get(from.min(idx)..idx)
                        .map(<[Segment]>::to_vec)
                        .unwrap_or_default(),
                    idx + 1 == tl.segments.len(),
                    tl.end,
                    false,
                    tl.switching,
                )
            }
        };
        if empty_end {
            // The first item never opened (its error was already sent).
            self.end_session();
            self.current = None;
            self.set_status(Status::Stopped);
            return;
        }
        let (Some(seg), Some(played)) = (seg, played) else {
            return;
        };
        let started = played > 0 || self.paused || seg.start > 0;
        if started && self.current.as_deref() != Some(seg.track_id.as_str()) {
            if let Some(prev) = self.current.take() {
                self.emit(Event::TrackEnded { track_id: prev });
            }
            for s in &skipped {
                let path = self.full_path(s);
                self.emit(Event::TrackStarted {
                    track_id: s.track_id.clone(),
                    source: s.source.clone(),
                    output: s.output.clone(),
                    path,
                });
                self.emit(Event::TrackEnded {
                    track_id: s.track_id.clone(),
                });
            }
            let path = self.full_path(&seg);
            self.last_path = Some(path.clone());
            self.emit(Event::TrackStarted {
                track_id: seg.track_id.clone(),
                source: seg.source.clone(),
                output: seg.output.clone(),
                path,
            });
            self.current = Some(seg.track_id.clone());
            self.current_seg = Some(seg.clone());
            self.announced = Some((self.generation, idx));
            self.need_next_sent = None;
        }
        if self.current.as_deref() != Some(seg.track_id.as_str()) {
            // This item hasn't reached the device yet (e.g. a reopened exclusive stream still empty).
            return;
        }
        let mut position = seg.start_sec + played.saturating_sub(seg.start) as f64 / rate;
        if let Some(d) = seg.duration {
            position = position.min(d);
        }
        self.position = position;
        self.duration = seg.duration;
        if self.status == Status::Loading {
            let s = if self.paused {
                Status::Paused
            } else {
                Status::Playing
            };
            self.set_status(s);
        }
        if let Some(end) = end {
            if played >= end {
                if let Some(id) = self.current.take() {
                    self.emit(Event::TrackEnded { track_id: id });
                }
                self.end_session();
                self.set_status(Status::Stopped);
                return;
            }
        }
        let next_set = lock(&self.next).is_some();
        if is_last
            && !switching
            && !next_set
            && seg.duration.is_some_and(|d| d - position < NEED_NEXT_SEC)
            && self.need_next_sent != self.current
        {
            self.need_next_sent = self.current.clone();
            self.emit(Event::NeedNext {
                track_id: seg.track_id.clone(),
            });
        }
        self.publish_plans(false);
        self.send_levels();
        if self.status == Status::Playing && self.last_state.elapsed() >= STATE_EVERY {
            self.emit_state();
        }
    }
}

/// Opens an item's link and decoder outside the engine (tests, tools).
pub fn probe(item: &PlayItem) -> io::Result<crate::decode::StreamInfo> {
    let (tx, _rx) = mpsc::channel();
    let shared = Arc::new(Shared {
        events: tx,
        urls: Mutex::default(),
        backoff: Duration::from_millis(250),
    });
    shared
        .open_decoder(item, &Arc::new(AtomicBool::new(false)), false)
        .map(|d| d.info)
        .map_err(|e| io::Error::other(e.to_string()))
}
