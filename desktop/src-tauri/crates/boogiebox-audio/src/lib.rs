//! Native Hi-Res playback engine for the BoogieBox desktop app (`wip/native-hires-engine-plan.md`).
//!
//! The web UI keeps the queue (D1) and gives the engine the current item and the next one. The engine reads
//! each item from a native stream link over HTTP `Range` ([`source`]), decodes it with symphonia
//! ([`decode`]), converts it for the device ([`convert`], [`resample`]) and plays it through an output
//! ([`output`]: WASAPI on Windows, a null output in tests). [`engine`] ties it together.
//!
//! The audio thread must never panic (`panic = "abort"` in release builds): `unwrap`, `expect` and `panic!`
//! are denied for this crate outside tests.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod convert;
pub mod decode;
pub mod dsp;
pub mod engine;
mod gapless;
pub mod negotiate;
pub mod output;
pub mod resample;
pub mod source;
mod types;

#[cfg(test)]
#[allow(clippy::chunks_exact_to_as_chunks)]
mod engine_tests;
#[cfg(test)]
mod test_media;
#[cfg(test)]
mod test_server;

pub use engine::{start, EngineConfig, EngineHandle};
pub use output::{Output, OutputError, OutputFactory};
pub use types::*;

/// Output devices on this machine (empty where the platform has no backend yet).
pub fn list_devices() -> Vec<Device> {
    #[cfg(windows)]
    {
        output::wasapi::list_devices().unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// The platform's real output backend.
#[cfg(windows)]
pub fn default_factory() -> Box<dyn OutputFactory> {
    Box::new(output::wasapi::WasapiFactory)
}
