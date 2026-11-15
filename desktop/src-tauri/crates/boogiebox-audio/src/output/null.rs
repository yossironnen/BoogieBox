//! A device-less output: consumes the ring in fixed periods (optionally faster than real time) and can
//! record every byte, for tests and for running the engine without sound.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use super::{Output, OutputError, OutputFactory, OutputShared, RenderCore};
use crate::convert::DeviceFormat;
use crate::types::{ErrorCode, OutputMode, PathKind};

/// Null output settings.
#[derive(Debug, Clone)]
pub struct NullConfig {
    /// The pretend device format.
    pub format: DeviceFormat,
    /// Frames per period.
    pub period_frames: usize,
    /// Wall time per period (`period / speed`); zero runs flat out.
    pub sleep: Duration,
    /// Keeps every rendered byte.
    pub record: bool,
}

/// A running null output.
pub struct NullOutput {
    config: NullConfig,
    shared: Arc<OutputShared>,
    recorded: Recording,
    thread: Option<JoinHandle<()>>,
}

impl NullOutput {
    /// Starts the output thread.
    pub fn start(config: NullConfig) -> Self {
        let shared = OutputShared::new();
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let (s, r, c) = (shared.clone(), recorded.clone(), config.clone());
        let thread = std::thread::Builder::new()
            .name("bb-audio-null".into())
            .spawn(move || {
                let mut core = RenderCore::new(s.clone(), c.format);
                let mut buf = vec![0u8; c.period_frames * c.format.frame_bytes()];
                while !s.stopping() {
                    core.render(&mut buf, 0);
                    if c.record {
                        r.lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .extend_from_slice(&buf);
                    }
                    std::thread::sleep(c.sleep);
                }
            })
            .ok();
        Self {
            config,
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
        self.config.format
    }

    fn shared(&self) -> &Arc<OutputShared> {
        &self.shared
    }

    fn path(&self) -> Vec<PathKind> {
        Vec::new()
    }
}

/// Bytes a [`NullOutput`] recorded.
pub type Recording = Arc<Mutex<Vec<u8>>>;

/// Opens [`NullOutput`]s; keeps the recording of the last one for tests.
#[derive(Clone)]
pub struct NullFactory {
    /// Settings for every output it opens.
    pub config: NullConfig,
    /// Recording of the most recent output.
    pub last_recording: Arc<Mutex<Option<Recording>>>,
    /// Shared state of the most recent output (tests inject device errors through it).
    pub last_shared: Arc<Mutex<Option<Arc<OutputShared>>>>,
    /// Refuse exclusive mode like a device that doesn't allow it.
    pub refuse_exclusive: bool,
}

impl NullFactory {
    /// A factory for `config`.
    pub fn new(config: NullConfig) -> Self {
        Self {
            config,
            last_recording: Arc::default(),
            last_shared: Arc::default(),
            refuse_exclusive: true,
        }
    }
}

impl OutputFactory for NullFactory {
    fn open(
        &self,
        _device_id: Option<&str>,
        mode: OutputMode,
    ) -> Result<Box<dyn Output>, OutputError> {
        if mode == OutputMode::Exclusive && self.refuse_exclusive {
            return Err(OutputError::new(
                ErrorCode::FormatUnsupported,
                "exclusive mode is not available on this output",
            ));
        }
        let out = NullOutput::start(self.config.clone());
        *self
            .last_recording
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(out.recorded());
        *self.last_shared.lock().unwrap_or_else(|p| p.into_inner()) = Some(out.shared().clone());
        Ok(Box::new(out))
    }
}
