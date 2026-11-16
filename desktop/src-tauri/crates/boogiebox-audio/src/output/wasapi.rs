//! WASAPI output (Windows): shared mode in the device's mix format (Windows mixes and resamples), or
//! exclusive mode at an exact format (bit-perfect), both event-driven with MMCSS "Pro Audio".
//!
//! WASAPI objects are COM objects that stay on the thread that made them, so a stream is opened on its own
//! output thread and its format is sent back before the engine starts producing. Phase 0 measured the
//! HRESULTs used here: `0x8889000A` device in use, `0x8889000E` exclusive mode not allowed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use wasapi::{DeviceEnumerator, Direction, SampleType, ShareMode, StreamMode, WaveFormat};

use super::{
    DeviceNotify, Output, OutputError, OutputFactory, OutputRequest, OutputShared, RenderCore,
    WatchGuard,
};
use crate::convert::{DeviceFormat, SampleKind};
use crate::negotiate::{DeviceCaps, KINDS, RATES};
use crate::types::{Device, ErrorCode, OutputMode, PathKind};

const E_DEVICE_IN_USE: u32 = 0x8889_000A;
const E_EXCLUSIVE_NOT_ALLOWED: u32 = 0x8889_000E;
const E_UNSUPPORTED_FORMAT: u32 = 0x8889_0008;
const E_BUFFER_SIZE_NOT_ALIGNED: u32 = 0x8889_0019;

fn hresult(e: &wasapi::WasapiError) -> Option<u32> {
    match e {
        wasapi::WasapiError::Windows(w) => Some(w.code().0 as u32),
        _ => None,
    }
}

fn map_err(e: wasapi::WasapiError) -> OutputError {
    let code = match hresult(&e) {
        Some(E_DEVICE_IN_USE) => ErrorCode::DeviceBusy,
        Some(E_EXCLUSIVE_NOT_ALLOWED) => ErrorCode::ExclusiveNotAllowed,
        Some(E_UNSUPPORTED_FORMAT) => ErrorCode::FormatUnsupported,
        _ => ErrorCode::DeviceLost,
    };
    OutputError::new(code, e.to_string())
}

fn open_device(
    en: &DeviceEnumerator,
    device_id: Option<&str>,
) -> Result<wasapi::Device, OutputError> {
    match device_id {
        Some(id) => en.get_device(id),
        None => en.get_default_device(Direction::Render),
    }
    .map_err(map_err)
}

/// Output devices (active render endpoints).
pub fn list_devices() -> Result<Vec<Device>, OutputError> {
    let _ = wasapi::initialize_mta();
    let en = DeviceEnumerator::new().map_err(map_err)?;
    let default_id = en
        .get_default_device(Direction::Render)
        .ok()
        .and_then(|d| d.get_id().ok());
    let mut out = Vec::new();
    for dev in &en
        .get_device_collection(Direction::Render)
        .map_err(map_err)?
    {
        let Ok(dev) = dev else { continue };
        let (Ok(id), Ok(name)) = (dev.get_id(), dev.get_friendlyname()) else {
            continue;
        };
        let summary = caps_summary(&id);
        out.push(Device {
            is_default: default_id.as_deref() == Some(id.as_str()),
            id,
            name,
            max_rate: summary.map(|s| s.0),
            max_bits: summary.map(|s| s.1),
            single_rate: summary.is_some_and(|s| s.2),
        });
    }
    Ok(out)
}

/// (highest rate, highest bits, one rate only) per device id, kept once probed: a device this engine holds in
/// exclusive mode can't be probed again, but its formats haven't changed.
/// Highest rate, highest bits, one rate only.
type CapsSummary = (u32, u16, bool);

fn caps_summary(id: &str) -> Option<CapsSummary> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, CapsSummary>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Mutex::default);
    if let Some(s) = cache.lock().unwrap_or_else(|p| p.into_inner()).get(id) {
        return Some(*s);
    }
    let caps = exclusive_caps(Some(id)).ok()?;
    let max_rate = caps.formats.iter().map(|f| f.sample_rate).max()?;
    let max_bits = caps.formats.iter().map(|f| f.kind.bits()).max()?;
    let single = caps.formats.iter().all(|f| f.sample_rate == max_rate);
    let summary = (max_rate, max_bits, single);
    cache
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(id.to_owned(), summary);
    Some(summary)
}

fn kind_of(fmt: &WaveFormat) -> Option<SampleKind> {
    match (
        fmt.get_subformat().ok()?,
        fmt.get_bitspersample(),
        fmt.get_validbitspersample(),
    ) {
        (SampleType::Float, 32, _) => Some(SampleKind::F32),
        (SampleType::Int, 16, _) => Some(SampleKind::I16),
        (SampleType::Int, 24, _) => Some(SampleKind::I24Packed),
        (SampleType::Int, 32, 24) => Some(SampleKind::I24In32),
        (SampleType::Int, 32, _) => Some(SampleKind::I32),
        _ => None,
    }
}

/// The `WAVEFORMATEXTENSIBLE` for an exact device format (valid bits set, so 24-in-32 is not 32-bit).
fn wave_format(f: &DeviceFormat) -> WaveFormat {
    let (store, valid, st) = match f.kind {
        SampleKind::I16 => (16, 16, SampleType::Int),
        SampleKind::I24Packed => (24, 24, SampleType::Int),
        SampleKind::I24In32 => (32, 24, SampleType::Int),
        SampleKind::I32 => (32, 32, SampleType::Int),
        SampleKind::F32 => (32, 32, SampleType::Float),
    };
    WaveFormat::new(
        store,
        valid,
        st,
        f.sample_rate as usize,
        usize::from(f.channels),
        None,
    )
}

/// The exclusive formats a device takes (stereo), probed with the driver-quirk retries.
pub fn exclusive_caps(device_id: Option<&str>) -> Result<DeviceCaps, OutputError> {
    let _ = wasapi::initialize_mta();
    let en = DeviceEnumerator::new().map_err(map_err)?;
    let client = open_device(&en, device_id)?
        .get_iaudioclient()
        .map_err(map_err)?;
    // A plain query first: with "Allow applications to take exclusive control" off every format fails
    // with 0x8889000E, which the quirk helper would hide.
    let probe = wave_format(&DeviceFormat {
        sample_rate: 48_000,
        channels: 2,
        kind: SampleKind::I16,
    });
    if let Err(e) = client.is_supported(&probe, ShareMode::Exclusive) {
        if hresult(&e) == Some(E_EXCLUSIVE_NOT_ALLOWED) {
            return Err(map_err(e));
        }
    }
    let mut caps = DeviceCaps::default();
    for rate in RATES {
        for kind in KINDS {
            let f = DeviceFormat {
                sample_rate: rate,
                channels: 2,
                kind,
            };
            if client
                .is_supported_exclusive_with_quirks(&wave_format(&f))
                .is_ok()
            {
                caps.formats.push(f);
            }
        }
    }
    Ok(caps)
}

/// Raises the calling thread to MMCSS "Pro Audio" (phase 0: the naive loop glitched at 192 kHz).
fn pro_audio_priority() {
    let mut task = 0u32;
    // SAFETY: a plain Win32 call with a valid, NUL-terminated wide string and a live out-pointer.
    let result = unsafe {
        windows::Win32::System::Threading::AvSetMmThreadCharacteristicsW(
            windows::core::w!("Pro Audio"),
            &mut task,
        )
    };
    if let Err(e) = result {
        tracing::warn!(target: "audio", "MMCSS Pro Audio not available: {e}");
    }
}

/// What to open.
#[derive(Debug, Clone)]
enum Spec {
    Shared,
    Exclusive {
        format: DeviceFormat,
        buffer_ms: u32,
    },
}

/// A WASAPI stream on its own thread.
pub struct WasapiOutput {
    format: DeviceFormat,
    exclusive: bool,
    shared: Arc<OutputShared>,
    thread: Option<JoinHandle<()>>,
}

impl WasapiOutput {
    fn open(device_id: Option<&str>, spec: Spec) -> Result<Self, OutputError> {
        let shared = OutputShared::new();
        let (tx, rx) = mpsc::channel::<Result<DeviceFormat, OutputError>>();
        let s = shared.clone();
        let id = device_id.map(str::to_owned);
        let exclusive = matches!(spec, Spec::Exclusive { .. });
        let thread = std::thread::Builder::new()
            .name("bb-audio-wasapi".into())
            .spawn(move || run(id, spec, s, tx))
            .map_err(|e| OutputError::new(ErrorCode::DeviceLost, e.to_string()))?;
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(format)) => Ok(Self {
                format,
                exclusive,
                shared,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                shared.request_stop();
                Err(OutputError::new(
                    ErrorCode::DeviceLost,
                    "the device did not open",
                ))
            }
        }
    }
}

type Opened = (
    wasapi::AudioClient,
    wasapi::Handle,
    wasapi::AudioRenderClient,
    DeviceFormat,
);

fn open_stream(device_id: Option<&str>, spec: &Spec) -> Result<Opened, OutputError> {
    let _ = wasapi::initialize_mta();
    let en = DeviceEnumerator::new().map_err(map_err)?;
    let device = open_device(&en, device_id)?;
    let mut client = device.get_iaudioclient().map_err(map_err)?;
    let (wave, format) = match spec {
        Spec::Shared => {
            let mix = client.get_mixformat().map_err(map_err)?;
            let kind = kind_of(&mix).ok_or_else(|| {
                OutputError::new(
                    ErrorCode::FormatUnsupported,
                    "the mix format is not PCM or float",
                )
            })?;
            let format = DeviceFormat {
                sample_rate: mix.get_samplespersec(),
                channels: mix.get_nchannels(),
                kind,
            };
            let (default_period, _) = client.get_device_period().map_err(map_err)?;
            client
                .initialize_client(
                    &mix,
                    Direction::Render,
                    StreamMode::EventsShared {
                        autoconvert: false,
                        buffer_duration_hns: default_period * 2,
                    },
                    None,
                )
                .map_err(map_err)?;
            (mix, format)
        }
        Spec::Exclusive { format, buffer_ms } => {
            let wanted = wave_format(format);
            let wave = client
                .is_supported_exclusive_with_quirks(&wanted)
                .map_err(map_err)?;
            let period = client
                .calculate_aligned_period_near(i64::from(*buffer_ms) * 10_000, Some(128), &wave)
                .map_err(map_err)?;
            if let Err(e) = client.initialize_client(
                &wave,
                Direction::Render,
                StreamMode::EventsExclusive { period_hns: period },
                None,
            ) {
                if hresult(&e) != Some(E_BUFFER_SIZE_NOT_ALIGNED) {
                    return Err(map_err(e));
                }
                // The documented retry: a fresh client with the period the driver asked for.
                let frames = client.get_buffer_size().map_err(map_err)?;
                let aligned = wasapi::calculate_period_100ns(
                    i64::from(frames),
                    i64::from(format.sample_rate),
                );
                client = device.get_iaudioclient().map_err(map_err)?;
                client
                    .initialize_client(
                        &wave,
                        Direction::Render,
                        StreamMode::EventsExclusive {
                            period_hns: aligned,
                        },
                        None,
                    )
                    .map_err(map_err)?;
            }
            (wave, *format)
        }
    };
    let _ = wave;
    let event = client.set_get_eventhandle().map_err(map_err)?;
    let render = client.get_audiorenderclient().map_err(map_err)?;
    Ok((client, event, render, format))
}

fn run(
    device_id: Option<String>,
    spec: Spec,
    shared: Arc<OutputShared>,
    ready: mpsc::Sender<Result<DeviceFormat, OutputError>>,
) {
    let (client, event, render, format) = match open_stream(device_id.as_deref(), &spec) {
        Ok(v) => v,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok(format));
    pro_audio_priority();
    let fb = format.frame_bytes();
    let mut core = RenderCore::new(shared.clone(), format);
    let buffer_frames = client.get_buffer_size().unwrap_or(0) as usize;
    let mut buf = vec![0u8; buffer_frames.max(1) * fb];
    // Prime the whole buffer, then start.
    if let Ok(n) = client.get_available_space_in_frames() {
        let bytes = n as usize * fb;
        core.render(&mut buf[..bytes], 0);
        if let Err(e) = render.write_to_device(&buf[..bytes], None) {
            shared.fail(map_err(e));
            return;
        }
    }
    if let Err(e) = client.start_stream() {
        shared.fail(map_err(e));
        return;
    }
    while !shared.stopping() {
        if event.wait_for_event(1000).is_err() {
            shared.fail(OutputError::new(
                ErrorCode::DeviceLost,
                "the device stopped asking for audio",
            ));
            break;
        }
        let (n, queued) = match (
            client.get_available_space_in_frames(),
            client.get_current_padding(),
        ) {
            (Ok(n), Ok(p)) => (n as usize, u64::from(p)),
            (Err(e), _) | (_, Err(e)) => {
                shared.fail(map_err(e));
                break;
            }
        };
        if n == 0 {
            continue;
        }
        let bytes = n * fb;
        if bytes > buf.len() {
            buf.resize(bytes, 0);
        }
        core.render(&mut buf[..bytes], queued);
        if let Err(e) = render.write_to_device(&buf[..bytes], None) {
            shared.fail(map_err(e));
            break;
        }
    }
    let _ = client.stop_stream();
}

impl Drop for WasapiOutput {
    fn drop(&mut self) {
        self.shared.request_stop();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Output for WasapiOutput {
    fn format(&self) -> DeviceFormat {
        self.format
    }

    fn shared(&self) -> &Arc<OutputShared> {
        &self.shared
    }

    fn path(&self) -> Vec<PathKind> {
        if self.exclusive {
            Vec::new()
        } else {
            vec![PathKind::SharedMixer]
        }
    }

    fn exclusive(&self) -> bool {
        self.exclusive
    }
}

/// Keeps a device-change registration alive on its own thread (COM objects stay where they were made).
struct Watch {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl WatchGuard for Watch {}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Opens WASAPI outputs.
#[derive(Debug, Default, Clone, Copy)]
pub struct WasapiFactory;

impl OutputFactory for WasapiFactory {
    fn open(
        &self,
        device_id: Option<&str>,
        request: &OutputRequest,
    ) -> Result<Box<dyn Output>, OutputError> {
        let spec = match (request.mode, request.format) {
            (OutputMode::Exclusive, Some(format)) => Spec::Exclusive {
                format,
                buffer_ms: request.buffer_ms.clamp(20, 200),
            },
            _ => Spec::Shared,
        };
        Ok(Box::new(WasapiOutput::open(device_id, spec)?))
    }

    fn exclusive_caps(&self, device_id: Option<&str>) -> Result<DeviceCaps, OutputError> {
        exclusive_caps(device_id)
    }

    fn devices(&self) -> Vec<Device> {
        list_devices().unwrap_or_default()
    }

    fn watch(&self, notify: DeviceNotify) -> Option<Box<dyn WatchGuard>> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("bb-audio-devices".into())
            .spawn(move || {
                let _ = wasapi::initialize_mta();
                let Ok(en) = DeviceEnumerator::new() else {
                    return;
                };
                let mut callbacks = wasapi::DeviceEventCallbacks::new();
                let n = notify.clone();
                callbacks.set_device_added_callback(move |_| n());
                let n = notify.clone();
                callbacks.set_device_removed_callback(move |_| n());
                let n = notify.clone();
                callbacks.set_device_state_callback(move |_, _| n());
                let n = notify;
                callbacks.set_default_device_callback(move |_, _, _| n());
                let Ok(_registration) = en.register_notification_callback(callbacks) else {
                    return;
                };
                while !flag.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(200));
                }
            })
            .ok()?;
        Some(Box::new(Watch {
            stop,
            thread: Some(thread),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opens the real default device in shared and exclusive mode with silence. Needs audio hardware:
    /// `cargo test -p boogiebox-audio -- --ignored wasapi`.
    #[test]
    #[ignore = "needs a real audio device"]
    fn wasapi_opens_the_default_device_shared_and_exclusive() {
        let devices = list_devices().unwrap();
        assert!(devices.iter().any(|d| d.is_default), "{devices:?}");
        let factory = WasapiFactory;
        let shared = factory
            .open(
                None,
                &OutputRequest {
                    mode: OutputMode::Shared,
                    format: None,
                    buffer_ms: 50,
                },
            )
            .unwrap();
        assert!(!shared.exclusive());
        assert_eq!(shared.path(), [PathKind::SharedMixer]);
        std::thread::sleep(Duration::from_millis(300));
        assert!(shared.shared().take_error().is_none());
        assert!(shared.shared().played(0).is_some());
        drop(shared);

        let caps = match factory.exclusive_caps(None) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("exclusive mode not available here: {e}");
                return;
            }
        };
        eprintln!("exclusive formats: {}", caps.formats.len());
        let Some(format) = caps.formats.first().copied() else {
            return;
        };
        for buffer_ms in [20, 50, 150] {
            let out = factory
                .open(
                    None,
                    &OutputRequest {
                        mode: OutputMode::Exclusive,
                        format: Some(format),
                        buffer_ms,
                    },
                )
                .unwrap();
            assert!(out.exclusive());
            assert_eq!(out.format(), format);
            assert!(out.path().is_empty());
            std::thread::sleep(Duration::from_millis(200));
            assert!(out.shared().take_error().is_none(), "{buffer_ms} ms");
        }
        let guard = factory.watch(Arc::new(|| {}));
        assert!(guard.is_some());
    }
}
