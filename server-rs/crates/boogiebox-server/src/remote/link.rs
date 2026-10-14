//! The transport a session uses to drive one renderer: real UPnP SOAP, or (tests) an in-process simulated
//! renderer that reproduces the WiiM behaviour measured in Phase 0.3.

use crate::upnp::device::ServiceEndpoint;
use crate::upnp::soap::{PositionInfo, SoapClient, TransportInfo};
use crate::upnp::UpnpError;

/// Control channel to a renderer.
#[derive(Clone, Debug)]
pub enum RendererLink {
    /// Real device over SOAP.
    Soap {
        /// LAN client.
        soap: SoapClient,
        /// AVTransport endpoints.
        avt: ServiceEndpoint,
        /// RenderingControl endpoints, when the device has volume control.
        rc: Option<ServiceEndpoint>,
    },
    /// Simulated renderer (tests).
    #[cfg(test)]
    Mock(std::sync::Arc<mock::MockRenderer>),
}

impl RendererLink {
    /// True when volume/mute can be controlled.
    pub fn has_volume(&self) -> bool {
        match self {
            RendererLink::Soap { rc, .. } => rc.is_some(),
            #[cfg(test)]
            RendererLink::Mock(_) => true,
        }
    }

    /// `SetAVTransportURI`.
    pub async fn set_uri(&self, uri: &str, meta: &str) -> Result<(), UpnpError> {
        match self {
            RendererLink::Soap { soap, avt, .. } => soap.set_av_transport_uri(avt, uri, meta).await,
            #[cfg(test)]
            RendererLink::Mock(m) => m.set_uri(uri, meta),
        }
    }

    /// `SetNextAVTransportURI` (empty `uri` clears).
    pub async fn set_next(&self, uri: &str, meta: &str) -> Result<(), UpnpError> {
        match self {
            RendererLink::Soap { soap, avt, .. } => {
                soap.set_next_av_transport_uri(avt, uri, meta).await
            }
            #[cfg(test)]
            RendererLink::Mock(m) => m.set_next(uri, meta),
        }
    }

    /// `Play`.
    pub async fn play(&self) -> Result<(), UpnpError> {
        match self {
            RendererLink::Soap { soap, avt, .. } => soap.play(avt).await,
            #[cfg(test)]
            RendererLink::Mock(m) => m.play(),
        }
    }

    /// `Pause`.
    pub async fn pause(&self) -> Result<(), UpnpError> {
        match self {
            RendererLink::Soap { soap, avt, .. } => soap.pause(avt).await,
            #[cfg(test)]
            RendererLink::Mock(m) => m.pause(),
        }
    }

    /// `Stop`.
    pub async fn stop(&self) -> Result<(), UpnpError> {
        match self {
            RendererLink::Soap { soap, avt, .. } => soap.stop(avt).await,
            #[cfg(test)]
            RendererLink::Mock(m) => m.stop(),
        }
    }

    /// `Seek` (`REL_TIME`).
    pub async fn seek(&self, seconds: f64) -> Result<(), UpnpError> {
        match self {
            RendererLink::Soap { soap, avt, .. } => soap.seek(avt, seconds).await,
            #[cfg(test)]
            RendererLink::Mock(m) => m.seek(seconds),
        }
    }

    /// `GetTransportInfo`.
    pub async fn transport_info(&self) -> Result<TransportInfo, UpnpError> {
        match self {
            RendererLink::Soap { soap, avt, .. } => soap.get_transport_info(avt).await,
            #[cfg(test)]
            RendererLink::Mock(m) => m.transport_info(),
        }
    }

    /// `GetPositionInfo`.
    pub async fn position_info(&self) -> Result<PositionInfo, UpnpError> {
        match self {
            RendererLink::Soap { soap, avt, .. } => soap.get_position_info(avt).await,
            #[cfg(test)]
            RendererLink::Mock(m) => m.position_info(),
        }
    }

    /// `GetVolume` + `GetMute`.
    pub async fn volume(&self) -> Result<(u8, bool), UpnpError> {
        match self {
            RendererLink::Soap { soap, rc, .. } => {
                let rc = rc.as_ref().ok_or(UpnpError::Missing("RenderingControl"))?;
                Ok((soap.get_volume(rc).await?, soap.get_mute(rc).await?))
            }
            #[cfg(test)]
            RendererLink::Mock(m) => m.volume(),
        }
    }

    /// `SetVolume`.
    pub async fn set_volume(&self, volume: u8) -> Result<(), UpnpError> {
        match self {
            RendererLink::Soap { soap, rc, .. } => {
                let rc = rc.as_ref().ok_or(UpnpError::Missing("RenderingControl"))?;
                soap.set_volume(rc, volume).await
            }
            #[cfg(test)]
            RendererLink::Mock(m) => m.set_volume(volume),
        }
    }

    /// `SetMute`.
    pub async fn set_mute(&self, mute: bool) -> Result<(), UpnpError> {
        match self {
            RendererLink::Soap { soap, rc, .. } => {
                let rc = rc.as_ref().ok_or(UpnpError::Missing("RenderingControl"))?;
                soap.set_mute(rc, mute).await
            }
            #[cfg(test)]
            RendererLink::Mock(m) => m.set_mute(mute),
        }
    }
}

#[cfg(test)]
pub(crate) mod mock {
    //! Simulated renderer on tokio time (use `start_paused = true` for deterministic tests).

    use std::sync::Mutex;
    use tokio::time::Instant;

    use crate::upnp::didl::parse_duration;
    use crate::upnp::soap::{PositionInfo, TransportInfo, TransportState};
    use crate::upnp::UpnpError;

    #[derive(Debug)]
    struct State {
        transport: TransportState,
        current: Option<String>,
        current_dur: f64,
        next: Option<String>,
        next_dur: f64,
        next_valid: bool,
        base: f64,
        started: Option<Instant>,
        volume: u8,
        fixed_volume: bool,
        muted: bool,
        offline: bool,
        fault_uri_contains: Option<String>,
        seek_drops_next: bool,
        carry_seek: Option<f64>,
        last_seek: Option<f64>,
        calls: Vec<String>,
    }

    /// Scriptable renderer.
    #[derive(Debug)]
    pub struct MockRenderer {
        s: Mutex<State>,
    }

    fn duration_from_meta(meta: &str) -> f64 {
        meta.split("duration=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .and_then(parse_duration)
            .unwrap_or(10.0)
    }

    impl MockRenderer {
        /// A WiiM-like renderer: seeking drops the prefetched next item.
        pub fn wiim() -> Self {
            Self {
                s: Mutex::new(State {
                    transport: TransportState::NoMedia,
                    current: None,
                    current_dur: 0.0,
                    next: None,
                    next_dur: 0.0,
                    next_valid: false,
                    base: 0.0,
                    started: None,
                    volume: 30,
                    fixed_volume: false,
                    muted: false,
                    offline: false,
                    fault_uri_contains: None,
                    seek_drops_next: true,
                    carry_seek: None,
                    last_seek: None,
                    calls: Vec::new(),
                }),
            }
        }

        fn lock(&self) -> std::sync::MutexGuard<'_, State> {
            self.s.lock().unwrap()
        }

        fn guard(&self, call: String) -> Result<std::sync::MutexGuard<'_, State>, UpnpError> {
            let mut s = self.lock();
            if s.offline {
                return Err(UpnpError::Http("timeout".into()));
            }
            s.calls.push(call);
            Ok(s)
        }

        /// Advances simulated playback to "now" (gapless transitions, natural end).
        fn settle(s: &mut State) {
            let Some(started) = s.started else { return };
            let now = Instant::now();
            let mut pos = s.base + now.duration_since(started).as_secs_f64();
            loop {
                if pos < s.current_dur {
                    break;
                }
                let overflow = pos - s.current_dur;
                if s.next.is_some() && s.next_valid {
                    s.current = s.next.take();
                    s.current_dur = s.next_dur;
                    s.next_valid = false;
                    pos = overflow;
                } else {
                    s.transport = TransportState::Stopped;
                    s.base = s.current_dur;
                    s.started = None;
                    return;
                }
            }
            s.base = pos;
            s.started = Some(now);
        }

        fn position(s: &State) -> f64 {
            match s.started {
                Some(t) => s.base + Instant::now().duration_since(t).as_secs_f64(),
                None => s.base,
            }
        }

        pub(crate) fn set_uri(&self, uri: &str, meta: &str) -> Result<(), UpnpError> {
            let mut s = self.guard(format!("SetAVTransportURI {uri}"))?;
            if s.fault_uri_contains
                .as_deref()
                .is_some_and(|f| uri.contains(f))
            {
                return Err(UpnpError::Fault {
                    code: 714,
                    description: "Illegal MIME-type".into(),
                });
            }
            s.current = Some(uri.to_string());
            s.current_dur = duration_from_meta(meta);
            s.next = None;
            s.next_valid = false;
            s.transport = TransportState::Stopped;
            // WiiM quirk: a fresh load starts at the previous seek target when one is pending.
            s.base = s.carry_seek.take().unwrap_or(0.0);
            s.started = None;
            Ok(())
        }

        pub(crate) fn set_next(&self, uri: &str, meta: &str) -> Result<(), UpnpError> {
            let mut s = self.guard(format!("SetNextAVTransportURI {uri}"))?;
            Self::settle(&mut s);
            if uri.is_empty() {
                s.next = None;
                s.next_valid = false;
            } else {
                s.next = Some(uri.to_string());
                s.next_dur = duration_from_meta(meta);
                s.next_valid = true;
            }
            Ok(())
        }

        pub(crate) fn play(&self) -> Result<(), UpnpError> {
            let mut s = self.guard("Play".into())?;
            if s.current.is_none() {
                return Err(UpnpError::Fault {
                    code: 701,
                    description: "Transition not allowed".into(),
                });
            }
            Self::settle(&mut s);
            if s.transport != TransportState::Playing {
                if s.transport == TransportState::Stopped && s.base >= s.current_dur {
                    s.base = 0.0;
                }
                s.transport = TransportState::Playing;
                s.started = Some(Instant::now());
            }
            Ok(())
        }

        pub(crate) fn pause(&self) -> Result<(), UpnpError> {
            let mut s = self.guard("Pause".into())?;
            Self::settle(&mut s);
            if s.transport != TransportState::Playing {
                return Err(UpnpError::Fault {
                    code: 701,
                    description: "Transition not allowed".into(),
                });
            }
            s.base = Self::position(&s);
            s.started = None;
            s.transport = TransportState::Paused;
            Ok(())
        }

        pub(crate) fn stop(&self) -> Result<(), UpnpError> {
            let mut s = self.guard("Stop".into())?;
            s.transport = TransportState::Stopped;
            s.started = None;
            s.base = 0.0;
            Ok(())
        }

        /// Device-side stop the way WiiM reports it: `STOPPED`, URI kept, `RelTime == TrackDuration`.
        pub fn stop_reporting_end(&self) {
            let mut s = self.lock();
            s.transport = TransportState::Stopped;
            s.started = None;
            s.base = s.current_dur;
        }

        pub(crate) fn seek(&self, seconds: f64) -> Result<(), UpnpError> {
            let mut s = self.guard(format!("Seek {seconds:.0}"))?;
            Self::settle(&mut s);
            s.base = seconds.clamp(0.0, s.current_dur);
            s.last_seek = Some(seconds);
            if s.started.is_some() {
                s.started = Some(Instant::now());
            }
            if s.seek_drops_next {
                s.next_valid = false;
            }
            Ok(())
        }

        pub(crate) fn transport_info(&self) -> Result<TransportInfo, UpnpError> {
            let mut s = self.guard("GetTransportInfo".into())?;
            Self::settle(&mut s);
            Ok(TransportInfo {
                state: s.transport.clone(),
                status: "OK".into(),
            })
        }

        pub(crate) fn position_info(&self) -> Result<PositionInfo, UpnpError> {
            let mut s = self.guard("GetPositionInfo".into())?;
            Self::settle(&mut s);
            Ok(PositionInfo {
                track_uri: s.current.clone(),
                rel_time: Some(Self::position(&s).floor()),
                duration: (s.current_dur > 0.0).then_some(s.current_dur),
            })
        }

        pub(crate) fn volume(&self) -> Result<(u8, bool), UpnpError> {
            let s = self.guard("GetVolume".into())?;
            Ok((s.volume, s.muted))
        }

        pub(crate) fn set_volume(&self, v: u8) -> Result<(), UpnpError> {
            let mut s = self.guard(format!("SetVolume {v}"))?;
            if !s.fixed_volume {
                s.volume = v.min(100);
            }
            Ok(())
        }

        /// Simulates a fixed-volume output: SetVolume succeeds but changes nothing.
        pub fn fix_volume(&self, at: u8) {
            let mut s = self.lock();
            s.fixed_volume = true;
            s.volume = at;
        }

        pub(crate) fn set_mute(&self, m: bool) -> Result<(), UpnpError> {
            self.guard(format!("SetMute {m}"))?.muted = m;
            Ok(())
        }

        // ── test controls ──

        /// Simulates the device dropping off the network (or coming back).
        pub fn set_offline(&self, offline: bool) {
            self.lock().offline = offline;
        }

        /// Makes `SetAVTransportURI` fail with fault 714 for URIs containing `needle`.
        pub fn fault_on(&self, needle: &str) {
            self.lock().fault_uri_contains = Some(needle.into());
        }

        /// Simulates another app taking the device over.
        pub fn foreign_play(&self, uri: &str) {
            let mut s = self.lock();
            s.current = Some(uri.into());
            s.current_dur = 300.0;
            s.next = None;
            s.transport = TransportState::Playing;
            s.base = 0.0;
            s.started = Some(Instant::now());
        }

        /// Simulates the user pausing from the device's own app.
        pub fn external_pause(&self) {
            let mut s = self.lock();
            Self::settle(&mut s);
            s.base = Self::position(&s);
            s.started = None;
            s.transport = TransportState::Paused;
        }

        /// Arms the WiiM "fresh load starts at the previous seek target" quirk.
        pub fn carry_seek(&self, seconds: f64) {
            self.lock().carry_seek = Some(seconds);
        }

        /// Sets the device volume as if changed from its own app.
        pub fn external_volume(&self, v: u8) {
            self.lock().volume = v;
        }

        /// Recorded calls (`"Play"`, `"Seek 9"`, `"SetNextAVTransportURI <uri>"`...).
        pub fn calls(&self) -> Vec<String> {
            self.lock().calls.clone()
        }

        /// Currently loaded URI.
        pub fn current(&self) -> Option<String> {
            self.lock().current.clone()
        }

        /// Current transport state (after settling).
        pub fn state(&self) -> TransportState {
            let mut s = self.lock();
            Self::settle(&mut s);
            s.transport.clone()
        }

        /// Current volume.
        pub fn current_volume(&self) -> (u8, bool) {
            let s = self.lock();
            (s.volume, s.muted)
        }
    }
}
