//! A device-less output: consumes the ring in fixed periods (optionally faster than real time) and can
//! record every byte, for tests and for running the engine without sound. The factory pretends to be a
//! device with configurable exclusive formats, a device list that can change, and a busy switch.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use super::{
    DeviceNotify, Output, OutputError, OutputFactory, OutputRequest, OutputShared, RenderCore,
    WatchGuard,
};
use crate::convert::DeviceFormat;
use crate::negotiate::DeviceCaps;
use crate::types::{Device, ErrorCode, OutputMode, PathKind};

/// Null output settings.
#[derive(Debug, Clone)]
pub struct NullConfig {
    /// The pretend device's shared-mode (mix) format.
    pub format: DeviceFormat,
    /// Frames per period, as a share of a second (`rate / period_div`).
    pub period_div: u32,
    /// Wall time per period (`period / speed`); zero runs flat out.
    pub sleep: Duration,
    /// Keeps every rendered byte.
    pub record: bool,
}

/// Bytes a [`NullOutput`] recorded.
pub type Recording = Arc<Mutex<Vec<u8>>>;

/// A running null output.
pub struct NullOutput {
    format: DeviceFormat,
    exclusive: bool,
    shared: Arc<OutputShared>,
    recorded: Recording,
    thread: Option<JoinHandle<()>>,
}

impl NullOutput {
    /// Starts the output thread for `format`.
    pub fn start(config: &NullConfig, format: DeviceFormat, exclusive: bool) -> Self {
        let shared = OutputShared::new();
        let recorded: Recording = Arc::default();
        let (s, r) = (shared.clone(), recorded.clone());
        let (sleep, record) = (config.sleep, config.record);
        let period = (format.sample_rate / config.period_div.max(1)).max(1) as usize;
        let thread = std::thread::Builder::new()
            .name("bb-audio-null".into())
            .spawn(move || {
                let mut core = RenderCore::new(s.clone(), format);
                let mut buf = vec![0u8; period * format.frame_bytes()];
                while !s.stopping() {
                    core.render(&mut buf, 0);
                    if record {
                        r.lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .extend_from_slice(&buf);
                    }
                    std::thread::sleep(sleep);
                }
            })
            .ok();
        Self {
            format,
            exclusive,
            shared,
            recorded,
            thread,
        }
    }

    /// The bytes rendered so far (when recording).
    pub fn recorded(&self) -> Recording {
        self.recorded.clone()
    }
}

impl Drop for NullOutput {
    fn drop(&mut self) {
        self.shared.request_stop();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Output for NullOutput {
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

struct NullWatch;
impl WatchGuard for NullWatch {}

/// Opens [`NullOutput`]s and keeps what they recorded.
#[derive(Clone)]
pub struct NullFactory {
    /// Settings for every output it opens.
    pub config: NullConfig,
    /// Exclusive formats; `None` behaves like Windows with exclusive control turned off.
    pub exclusive: Arc<Mutex<Option<DeviceCaps>>>,
    /// Exclusive opens fail as "in use by another app" while set.
    pub busy: Arc<AtomicBool>,
    /// Every output opened: its format, whether exclusive, and what it recorded.
    pub opened: Arc<Mutex<Vec<(DeviceFormat, bool, Recording)>>>,
    /// Shared state of the most recent output (tests inject device errors through it).
    pub last_shared: Arc<Mutex<Option<Arc<OutputShared>>>>,
    /// The pretend device list.
    pub device_list: Arc<Mutex<Vec<Device>>>,
    notify: Arc<Mutex<Option<DeviceNotify>>>,
}

impl NullFactory {
    /// A factory for `config` with one default device and no exclusive mode.
    pub fn new(config: NullConfig) -> Self {
        Self {
            config,
            exclusive: Arc::default(),
            busy: Arc::default(),
            opened: Arc::default(),
            last_shared: Arc::default(),
            device_list: Arc::new(Mutex::new(vec![Device {
                id: "null".into(),
                name: "Null output".into(),
                is_default: true,
            }])),
            notify: Arc::default(),
        }
    }

    /// The recording of the most recent output.
    pub fn last_recording(&self) -> Option<Recording> {
        let opened = self.opened.lock().unwrap_or_else(|p| p.into_inner());
        opened.last().map(|(_, _, r)| r.clone())
    }

    /// Replaces the device list and tells the engine (like a plug or unplug).
    pub fn set_devices(&self, devices: Vec<Device>) {
        *self.device_list.lock().unwrap_or_else(|p| p.into_inner()) = devices;
        let notify = self
            .notify
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(n) = notify {
            n();
        }
    }
}

impl OutputFactory for NullFactory {
    fn open(
        &self,
        _device_id: Option<&str>,
        request: &OutputRequest,
    ) -> Result<Box<dyn Output>, OutputError> {
        let (format, exclusive) = match (request.mode, request.format) {
            (OutputMode::Exclusive, Some(format)) => {
                if self.busy.load(Ordering::Relaxed) {
                    return Err(OutputError::new(
                        ErrorCode::DeviceBusy,
                        "the device is in use",
                    ));
                }
                (format, true)
            }
            _ => (self.config.format, false),
        };
        let out = NullOutput::start(&self.config, format, exclusive);
        self.opened.lock().unwrap_or_else(|p| p.into_inner()).push((
            format,
            exclusive,
            out.recorded(),
        ));
        *self.last_shared.lock().unwrap_or_else(|p| p.into_inner()) = Some(out.shared().clone());
        Ok(Box::new(out))
    }

    fn exclusive_caps(&self, _device_id: Option<&str>) -> Result<DeviceCaps, OutputError> {
        self.exclusive
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .ok_or_else(|| {
                OutputError::new(
                    ErrorCode::ExclusiveNotAllowed,
                    "exclusive control is turned off for this device",
                )
            })
    }

    fn devices(&self) -> Vec<Device> {
        self.device_list
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn watch(&self, notify: DeviceNotify) -> Option<Box<dyn WatchGuard>> {
        *self.notify.lock().unwrap_or_else(|p| p.into_inner()) = Some(notify);
        Some(Box::new(NullWatch))
    }
}
