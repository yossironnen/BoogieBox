//! Commands the host sends to the engine and events it gets back (`wip/native-hires-engine-plan.md` §4.4).

use serde::{Deserialize, Serialize};

use crate::dsp::EqBand;

/// One playable item: a library track or a BoogieMix, fetched from a native stream link.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayItem {
    /// Track id (or `boogiemix:{id}`), echoed back in events.
    pub track_id: String,
    /// Absolute `http://server/media/n/{token}` URL.
    pub url: String,
    /// File extension or format name (`flac`, `m4a`, `wav`…), a probe hint.
    #[serde(default)]
    pub format_hint: Option<String>,
    /// Duration from the server, used until the decoder knows better.
    #[serde(default)]
    pub duration_sec: Option<f64>,
    /// The server decoded the source (`decode: "server_wav"`: Opus, WMA).
    #[serde(default)]
    pub server_decoded: bool,
    /// A live stream (internet radio): read once from the start, no seeking, no length.
    #[serde(default)]
    pub live: bool,
    /// Volume levelling gain (dB) the web UI worked out for this item (`audio/loudness.ts`).
    #[serde(default)]
    pub gain_db: f64,
    /// This item's own EQ curve (saved for the song or album, or Auto EQ's); `None`: the user's EQ.
    #[serde(default)]
    pub eq: Option<Vec<EqBand>>,
}

/// How the engine may change the sound (the web player's settings; Pure mode turns all of it off).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoundSettings {
    /// Nothing changes the samples: EQ, levelling and crossfade are off.
    #[serde(default)]
    pub pure: bool,
    /// The user's own EQ curve (empty: off).
    #[serde(default)]
    pub eq: Vec<EqBand>,
    /// Items may bring their own curve (saved EQ, Auto EQ).
    #[serde(default)]
    pub item_eq: bool,
    /// Volume levelling is on (items bring their gain).
    #[serde(default)]
    pub levelling: bool,
    /// Crossfade length (s) between items; 0: gapless joins.
    #[serde(default)]
    pub crossfade_sec: f64,
}

/// How the engine shares the device with other apps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputMode {
    /// Windows mixes and resamples; works with every device and other apps keep sound.
    Shared,
    /// The engine owns the device (phase 3: bit-perfect).
    Exclusive,
}

/// Something for the engine to do. The host page sends these as JSON (`{"type": "load", ...}`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Command {
    /// Replace what is playing.
    Load {
        /// The item.
        item: PlayItem,
        /// Start position (s).
        start_sec: f64,
        /// Start playing once buffered (else stay paused at `start_sec`).
        autoplay: bool,
    },
    /// The item after the current one (gapless); `None` clears it.
    SetNext {
        /// The item, or none.
        item: Option<PlayItem>,
    },
    /// Resume.
    Play,
    /// Pause, keeping the buffers.
    Pause,
    /// Stop and forget the current item.
    Stop,
    /// Seek within the current item.
    Seek {
        /// Position (s).
        sec: f64,
    },
    /// Linear volume 0..=1.
    SetVolume {
        /// Volume.
        linear: f64,
    },
    /// Choose the output device and mode.
    SetOutput {
        /// Device id from [`crate::list_devices`], `None` for the system default.
        device_id: Option<String>,
        /// Shared or exclusive.
        mode: OutputMode,
        /// Exclusive-mode buffer (ms), 20..=200; plan §12.1 Low/Normal/Safe = 20/50/150.
        buffer_ms: u32,
        /// A file the device can't play at its own rate is resampled to the nearest rate it takes (D9);
        /// off: such files play in shared mode.
        #[serde(default = "default_true")]
        nearest_rate: bool,
    },
    /// EQ, levelling, crossfade and Pure mode.
    SetSound {
        /// The settings.
        sound: SoundSettings,
    },
    /// An item's levelling gain or own EQ curve changed (e.g. its loudness values arrived).
    SetItemSound {
        /// Item id.
        track_id: String,
        /// Levelling gain (dB).
        gain_db: f64,
        /// Its own curve, or `None` for the user's EQ.
        #[serde(default)]
        eq: Option<Vec<EqBand>>,
    },
    /// A fresh link for an item whose link expired ([`Event::TokenExpired`]).
    RefreshUrl {
        /// Item id.
        track_id: String,
        /// New URL.
        url: String,
    },
    /// End the engine's threads.
    Shutdown,
}

fn default_true() -> bool {
    true
}

/// Transport state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// Nothing loaded.
    Idle,
    /// Opening or buffering.
    Loading,
    /// Sound is coming out.
    Playing,
    /// Paused.
    Paused,
    /// Finished, stopped, or stopped by an error.
    Stopped,
}

/// A stream format (source or device).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Format {
    /// Sample rate (Hz).
    pub sample_rate: u32,
    /// Bit depth, when meaningful (lossless sources, integer devices).
    pub bits: Option<u16>,
    /// Channel count.
    pub channels: u16,
    /// Codec or sample type (`FLAC`, `ALAC`, `MP3`…, `f32`, `s24in32`…).
    pub codec: String,
}

/// What happens to the samples on their way to the device (the badge in the UI).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum PathKind {
    /// Samples reach the device unchanged.
    BitPerfect,
    /// Unchanged except for the volume.
    VolumeApplied,
    /// The samples are changed on the way (e.g. a device narrower than the source cuts the low bits).
    DspApplied,
    /// The EQ shapes the sound.
    EqApplied,
    /// Volume levelling changes the level.
    Levelled,
    /// Songs crossfade into each other.
    Crossfade,
    /// Resampled by the engine.
    Resampled {
        /// Output rate.
        to_hz: u32,
    },
    /// Windows' mixer takes it from here (shared mode).
    SharedMixer,
    /// More channels than the device takes, mixed down.
    Downmixed,
    /// The server decoded a format the engine can't (Opus, WMA).
    ServerConverted,
}

/// Why something failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The server could not be reached or stopped answering.
    Network,
    /// The file could not be decoded.
    Decode,
    /// Another app holds the device in exclusive mode.
    DeviceBusy,
    /// The device went away or failed.
    DeviceLost,
    /// The device or engine can't play this format.
    FormatUnsupported,
    /// The stream link expired and no new one came.
    TokenExpired,
    /// Windows' "Allow applications to take exclusive control" is off for the device.
    ExclusiveNotAllowed,
}

/// Something the host should know.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum Event {
    /// Transport state and position (~4 Hz while playing, and on every change).
    State {
        /// Status.
        status: Status,
        /// Item at the device now.
        track_id: Option<String>,
        /// Position (s).
        position_sec: f64,
        /// Duration (s), when known.
        duration_sec: Option<f64>,
    },
    /// The first sample of an item reached the device.
    TrackStarted {
        /// Item id.
        track_id: String,
        /// Source format.
        source: Format,
        /// Device format.
        output: Format,
        /// What happens on the way.
        path: Vec<PathKind>,
    },
    /// An item finished playing (fires when the next one starts, for gapless).
    TrackEnded {
        /// Item id.
        track_id: String,
    },
    /// The current item ends in under 15 s and no next item is set.
    NeedNext {
        /// Current item id.
        track_id: String,
    },
    /// What happens to the current item's samples changed (e.g. the volume left 100%).
    PathChanged {
        /// Item id.
        track_id: String,
        /// The new path.
        path: Vec<PathKind>,
    },
    /// Output devices were added, removed or changed.
    DeviceChanged {
        /// Devices now.
        devices: Vec<Device>,
        /// The engine's device (`None`: the system default).
        active: Option<String>,
    },
    /// An item's link expired: answer with [`Command::RefreshUrl`].
    TokenExpired {
        /// Item id.
        track_id: String,
    },
    /// What is playing now, for the VU meter (~20 Hz while playing).
    Levels {
        /// Highest sample level since the last event (0..1).
        peak: f32,
        /// RMS level since the last event (0..1).
        rms: f32,
        /// The latest ~21 ms of the sound (mono, ~24 kHz), scaled to -127..127.
        wave: Vec<i8>,
    },
    /// Something failed.
    Error {
        /// Item, when the error belongs to one.
        track_id: Option<String>,
        /// Kind.
        code: ErrorCode,
        /// Detail for logs.
        message: String,
    },
}

/// An output device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// Stable id.
    pub id: String,
    /// Name shown to the user.
    pub name: String,
    /// The system default output.
    pub is_default: bool,
    /// Highest exclusive-mode rate (Hz), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rate: Option<u32>,
    /// Highest exclusive-mode bit depth, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bits: Option<u16>,
    /// Exclusive mode takes one rate only (e.g. an HDMI monitor at 48 kHz).
    #[serde(default)]
    pub single_rate: bool,
}
