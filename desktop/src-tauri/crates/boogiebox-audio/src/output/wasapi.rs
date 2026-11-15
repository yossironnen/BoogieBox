//! WASAPI output (Windows). Phase 2: shared mode, event-driven, in the device's mix format; Windows' mixer
//! takes it from there. Exclusive mode (bit-perfect) is phase 3.
//!
//! WASAPI objects are COM objects that stay on the thread that made them, so the device is opened on the
//! output thread and its format is sent back before the engine starts producing.

use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode};

use super::{Output, OutputError, OutputFactory, OutputShared, RenderCore};
use crate::convert::{DeviceFormat, SampleKind};
use crate::types::{Device, ErrorCode, OutputMode, PathKind};

/// Output devices (render endpoints that are active).
pub fn list_devices() -> Result<Vec<Device>, OutputError> {
    let _ = wasapi::initialize_mta();
    let en = DeviceEnumerator::new().map_err(device_err)?;
    let default_id = en
        .get_default_device(Direction::Render)
        .ok()
        .and_then(|d| d.get_id().ok());
    let mut out = Vec::new();
    for dev in &en
        .get_device_collection(Direction::Render)
        .map_err(device_err)?
    {
        let Ok(dev) = dev else { continue };
        let (Ok(id), Ok(name)) = (dev.get_id(), dev.get_friendlyname()) else {
            continue;
        };
        out.push(Device {
            is_default: default_id.as_deref() == Some(id.as_str()),
            id,
            name,
        });
    }
    Ok(out)
}

fn device_err(e: wasapi::WasapiError) -> OutputError {
    OutputError::new(ErrorCode::DeviceLost, e.to_string())
}

fn kind_of(fmt: &wasapi::WaveFormat) -> Option<SampleKind> {
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

/// A shared-mode WASAPI stream.
pub struct WasapiOutput {
    format: DeviceFormat,
    shared: Arc<OutputShared>,
    thread: Option<JoinHandle<()>>,
}

impl WasapiOutput {
    /// Opens `device_id` (or the default render device) in shared mode.
    pub fn open_shared(device_id: Option<&str>) -> Result<Self, OutputError> {
        let shared = OutputShared::new();
        let (tx, rx) = mpsc::channel::<Result<DeviceFormat, OutputError>>();
        let s = shared.clone();
        let id = device_id.map(str::to_owned);
        let thread = std::thread::Builder::new()
            .name("bb-audio-wasapi".into())
            .spawn(move || run_shared(id, s, tx))
            .map_err(|e| OutputError::new(ErrorCode::DeviceLost, e.to_string()))?;
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(format)) => Ok(Self {
                format,
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

fn run_shared(
    device_id: Option<String>,
    shared: Arc<OutputShared>,
    ready: mpsc::Sender<Result<DeviceFormat, OutputError>>,
) {
    let opened = (|| -> Result<_, OutputError> {
        let _ = wasapi::initialize_mta();
        let en = DeviceEnumerator::new().map_err(device_err)?;
        let device = match device_id.as_deref() {
            Some(id) => en.get_device(id),
            None => en.get_default_device(Direction::Render),
        }
        .map_err(device_err)?;
        let mut client = device.get_iaudioclient().map_err(device_err)?;
        let mix = client.get_mixformat().map_err(device_err)?;
        let kind = kind_of(&mix).ok_or_else(|| {
            OutputError::new(
                ErrorCode::FormatUnsupported,
                "the device mix format is not PCM or float",
            )
        })?;
        let (default_period, _) = client.get_device_period().map_err(device_err)?;
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
            .map_err(|e| match &e {
                wasapi::WasapiError::Windows(w) if w.code().0 as u32 == 0x8889_000A => {
                    OutputError::new(ErrorCode::DeviceBusy, e.to_string())
                }
                _ => device_err(e),
            })?;
        let event = client.set_get_eventhandle().map_err(device_err)?;
        let render = client.get_audiorenderclient().map_err(device_err)?;
        let format = DeviceFormat {
            sample_rate: mix.get_samplespersec(),
            channels: mix.get_nchannels(),
            kind,
        };
        Ok((client, event, render, format))
    })();
    let (client, event, render, format) = match opened {
        Ok(v) => v,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok(format));
    pro_audio_priority();

    let mut core = RenderCore::new(shared.clone(), format);
    let buffer_frames = client.get_buffer_size().unwrap_or(0) as usize;
    let mut buf = vec![0u8; buffer_frames.max(1) * format.frame_bytes()];
    // Prime with silence, then start.
    if let Ok(n) = client.get_available_space_in_frames() {
        let n = n as usize;
        core.render(&mut buf[..n * format.frame_bytes()], 0);
        if let Err(e) = render.write_to_device(&buf[..n * format.frame_bytes()], None) {
            shared.fail(device_err(e));
            return;
        }
    }
    if let Err(e) = client.start_stream() {
        shared.fail(device_err(e));
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
                shared.fail(device_err(e));
                break;
            }
        };
        if n == 0 {
            continue;
        }
        let bytes = n * format.frame_bytes();
        if bytes > buf.len() {
            buf.resize(bytes, 0);
        }
        core.render(&mut buf[..bytes], queued);
        if let Err(e) = render.write_to_device(&buf[..bytes], None) {
            shared.fail(device_err(e));
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
        vec![PathKind::SharedMixer]
    }
}

/// Opens WASAPI outputs.
#[derive(Debug, Default, Clone, Copy)]
pub struct WasapiFactory;

impl OutputFactory for WasapiFactory {
    fn open(
        &self,
        device_id: Option<&str>,
        mode: OutputMode,
    ) -> Result<Box<dyn Output>, OutputError> {
        match mode {
            OutputMode::Shared => Ok(Box::new(WasapiOutput::open_shared(device_id)?)),
            OutputMode::Exclusive => Err(OutputError::new(
                ErrorCode::FormatUnsupported,
                "exclusive mode is not built yet (phase 3)",
            )),
        }
    }
}
