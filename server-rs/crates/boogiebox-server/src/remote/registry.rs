//! In-memory view of the renderers on the LAN: online state, last description and capabilities.
//! Persisted preferences (alias/hidden/format policy) live in `upnp_renderers`; this registry only tracks what
//! the network currently says.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::upnp::device::{
    parse_device_description, parse_scpd_actions, RendererDescription, SinkCaps,
};
use crate::upnp::soap::SoapClient;
use crate::upnp::UpnpError;

/// Re-fetch a device description at most this often while the device keeps announcing itself.
pub const DESCRIPTION_TTL: Duration = Duration::from_secs(30 * 60);
/// Debounce for `last_seen_at` writes to the database.
pub const DB_TOUCH_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// Grace period after `max-age` before a silent device is considered offline.
pub const EXPIRY_GRACE: Duration = Duration::from_secs(30);

/// What a renderer can do beyond the mandatory AVTransport basics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RendererCaps {
    /// Accepted HTTP formats (from `GetProtocolInfo`; empty when unknown).
    pub sink: SinkCaps,
    /// `SetNextAVTransportURI` is declared (gapless transitions).
    pub set_next: bool,
    /// `Seek` is declared.
    pub seek: bool,
}

/// JSON stored in `upnp_renderers.capabilities` so offline devices still render in the UI after a restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredCapabilities {
    /// Last parsed device description.
    pub description: RendererDescription,
    /// Last known capabilities.
    pub caps: RendererCaps,
}

/// One renderer as currently known.
#[derive(Debug, Clone, PartialEq)]
pub struct RendererEntry {
    /// Parsed device description.
    pub description: RendererDescription,
    /// Capabilities.
    pub caps: RendererCaps,
    /// Description URL the device last advertised.
    pub location: String,
    /// Reachable according to discovery.
    pub online: bool,
    /// When the description was last fetched (None = loaded from the database only).
    pub described_at: Option<Instant>,
    /// When the device must re-announce before being considered offline.
    pub expires_at: Instant,
    /// Last `last_seen_at` database write.
    pub db_touched_at: Option<Instant>,
}

/// Result of a cheap "seen again" update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SeenOutcome {
    /// The device was unknown, moved, or its description is stale: fetch it again.
    pub needs_description: bool,
    /// The device went from offline to online.
    pub came_online: bool,
    /// `last_seen_at` should be written to the database.
    pub persist_touch: bool,
}

/// Shared, cheaply cloneable registry.
#[derive(Debug, Clone, Default)]
pub struct RendererRegistry {
    inner: Arc<RwLock<HashMap<String, RendererEntry>>>,
}

impl RendererRegistry {
    fn read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<String, RendererEntry>> {
        self.inner.read().unwrap_or_else(|p| p.into_inner())
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<String, RendererEntry>> {
        self.inner.write().unwrap_or_else(|p| p.into_inner())
    }

    /// Seeds offline entries from persisted capabilities (unparsable rows are skipped).
    pub fn load_persisted(&self, rows: &[boogiebox_db::remote::RendererRow], now: Instant) {
        let mut map = self.write();
        for row in rows {
            let Ok(stored) = serde_json::from_str::<StoredCapabilities>(&row.capabilities) else {
                continue;
            };
            map.entry(row.udn.clone()).or_insert(RendererEntry {
                description: stored.description,
                caps: stored.caps,
                location: row.location_url.clone(),
                online: false,
                described_at: None,
                expires_at: now,
                db_touched_at: None,
            });
        }
    }

    /// Clone of one entry.
    pub fn get(&self, udn: &str) -> Option<RendererEntry> {
        self.read().get(udn).cloned()
    }

    /// Clone of all entries.
    pub fn snapshot(&self) -> Vec<RendererEntry> {
        self.read().values().cloned().collect()
    }

    /// True when the renderer is currently online.
    pub fn is_online(&self, udn: &str) -> bool {
        self.read().get(udn).is_some_and(|e| e.online)
    }

    /// Records an advertisement for `udn` at `location` valid for `max_age` seconds.
    pub fn mark_seen(&self, udn: &str, location: &str, max_age: u64, now: Instant) -> SeenOutcome {
        let mut map = self.write();
        let Some(entry) = map.get_mut(udn) else {
            return SeenOutcome {
                needs_description: true,
                ..SeenOutcome::default()
            };
        };
        let stale = entry
            .described_at
            .is_none_or(|t| now.saturating_duration_since(t) >= DESCRIPTION_TTL);
        let moved = entry.location != location;
        let came_online = !entry.online;
        entry.online = true;
        entry.expires_at = now + Duration::from_secs(max_age) + EXPIRY_GRACE;
        let persist_touch = entry
            .db_touched_at
            .is_none_or(|t| now.saturating_duration_since(t) >= DB_TOUCH_INTERVAL);
        if persist_touch {
            entry.db_touched_at = Some(now);
        }
        SeenOutcome {
            needs_description: stale || moved,
            came_online,
            persist_touch,
        }
    }

    /// Stores a freshly fetched description. Returns true when anything user-visible changed.
    pub fn insert_described(
        &self,
        description: RendererDescription,
        caps: RendererCaps,
        location: &str,
        max_age: u64,
        now: Instant,
    ) -> bool {
        let udn = description.udn.clone();
        let entry = RendererEntry {
            description,
            caps,
            location: location.to_string(),
            online: true,
            described_at: Some(now),
            expires_at: now + Duration::from_secs(max_age) + EXPIRY_GRACE,
            db_touched_at: Some(now),
        };
        let mut map = self.write();
        let changed = map.get(&udn).is_none_or(|old| {
            !old.online || old.description != entry.description || old.caps != entry.caps
        });
        map.insert(udn, entry);
        changed
    }

    /// Marks a renderer offline (`ssdp:byebye`). Returns true if it was online.
    pub fn mark_gone(&self, udn: &str) -> bool {
        match self.write().get_mut(udn) {
            Some(e) if e.online => {
                e.online = false;
                true
            }
            _ => false,
        }
    }

    /// Marks renderers whose advertisement expired as offline and returns their UDNs.
    pub fn expire(&self, now: Instant) -> Vec<String> {
        let mut gone = Vec::new();
        for (udn, e) in self.write().iter_mut() {
            if e.online && e.expires_at <= now {
                e.online = false;
                gone.push(udn.clone());
            }
        }
        gone
    }

    /// Removes every entry (used when Play On is disabled or the database changes).
    pub fn clear(&self) {
        self.write().clear();
    }
}

/// Fetches and parses a renderer's description, SCPD actions and sink formats.
///
/// Only the description itself is mandatory; a missing SCPD or `GetProtocolInfo` degrades to conservative
/// capabilities (no gapless, empty sink list → transcode).
pub async fn describe_renderer(
    soap: &SoapClient,
    location: &str,
) -> Result<(RendererDescription, RendererCaps), UpnpError> {
    let text = soap.get_text(location).await?;
    let description = parse_device_description(&text, location)?;
    let mut caps = RendererCaps {
        seek: true,
        ..RendererCaps::default()
    };
    if let Some(scpd_url) = &description.av_transport.scpd_url {
        match soap
            .get_text(scpd_url)
            .await
            .and_then(|t| parse_scpd_actions(&t))
        {
            Ok(actions) => {
                caps.set_next = actions.contains("SetNextAVTransportURI");
                caps.seek = actions.contains("Seek");
            }
            Err(e) => {
                tracing::debug!(target: "remote", udn = %description.udn, "AVTransport SCPD unavailable: {e}")
            }
        }
    }
    if let Some(cm) = &description.connection_manager {
        match soap.get_protocol_info(cm).await {
            Ok(sink) => caps.sink = sink,
            Err(e) => {
                tracing::debug!(target: "remote", udn = %description.udn, "GetProtocolInfo failed: {e}")
            }
        }
    }
    Ok((description, caps))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::upnp::device::{ServiceEndpoint, Vendor};

    pub(crate) fn description(udn: &str, host: &str) -> RendererDescription {
        RendererDescription {
            udn: udn.into(),
            friendly_name: format!("Speaker {udn}"),
            manufacturer: Some("Acme".into()),
            model_name: Some("Box".into()),
            model_number: None,
            vendor: Vendor::Generic,
            icons: vec![],
            av_transport: ServiceEndpoint {
                service_type: crate::upnp::AV_TRANSPORT_SERVICE.into(),
                control_url: format!("http://{host}/ctl"),
                event_sub_url: None,
                scpd_url: None,
            },
            rendering_control: None,
            connection_manager: None,
        }
    }

    #[test]
    fn seen_lifecycle_and_expiry() {
        let reg = RendererRegistry::default();
        let t0 = Instant::now();
        let first = reg.mark_seen("a", "http://10.0.0.5/d.xml", 1800, t0);
        assert!(first.needs_description);
        assert!(reg.insert_described(
            description("a", "10.0.0.5"),
            RendererCaps::default(),
            "http://10.0.0.5/d.xml",
            60,
            t0
        ));
        assert!(reg.is_online("a"));
        // Same description again: nothing changed.
        assert!(!reg.insert_described(
            description("a", "10.0.0.5"),
            RendererCaps::default(),
            "http://10.0.0.5/d.xml",
            60,
            t0
        ));

        let again = reg.mark_seen(
            "a",
            "http://10.0.0.5/d.xml",
            60,
            t0 + Duration::from_secs(5),
        );
        assert_eq!(
            again,
            SeenOutcome {
                needs_description: false,
                came_online: false,
                persist_touch: false
            }
        );
        let moved = reg.mark_seen(
            "a",
            "http://10.0.0.6/d.xml",
            60,
            t0 + Duration::from_secs(6),
        );
        assert!(moved.needs_description);
        let stale = reg.mark_seen("a", "http://10.0.0.5/d.xml", 60, t0 + DESCRIPTION_TTL);
        assert!(stale.needs_description && stale.persist_touch);

        let later = t0 + DESCRIPTION_TTL + Duration::from_secs(61) + EXPIRY_GRACE;
        assert_eq!(reg.expire(later), vec!["a".to_string()]);
        assert!(!reg.is_online("a"));
        assert!(reg.expire(later).is_empty());
        let back = reg.mark_seen("a", "http://10.0.0.5/d.xml", 60, later);
        assert!(back.came_online);
    }

    #[test]
    fn gone_clear_and_snapshot() {
        let reg = RendererRegistry::default();
        let now = Instant::now();
        reg.insert_described(
            description("a", "10.0.0.5"),
            RendererCaps::default(),
            "http://10.0.0.5/d",
            60,
            now,
        );
        assert!(reg.mark_gone("a"));
        assert!(!reg.mark_gone("a"));
        assert!(!reg.mark_gone("zzz"));
        assert_eq!(reg.snapshot().len(), 1);
        assert!(reg.get("a").is_some());
        reg.clear();
        assert!(reg.snapshot().is_empty());
        assert!(reg.get("a").is_none());
    }

    #[test]
    fn load_persisted_skips_bad_rows_and_keeps_live_entries() {
        let reg = RendererRegistry::default();
        let now = Instant::now();
        reg.insert_described(
            description("live", "10.0.0.7"),
            RendererCaps::default(),
            "http://10.0.0.7/d",
            60,
            now,
        );
        let stored = serde_json::to_string(&StoredCapabilities {
            description: description("old", "10.0.0.8"),
            caps: RendererCaps {
                set_next: true,
                ..Default::default()
            },
        })
        .unwrap();
        let row = |udn: &str, caps: &str| boogiebox_db::remote::RendererRow {
            udn: udn.into(),
            friendly_name: "x".into(),
            alias: None,
            manufacturer: None,
            model_name: None,
            vendor: "generic".into(),
            location_url: "http://10.0.0.8/d".into(),
            capabilities: caps.into(),
            format_policy: "auto".into(),
            hidden: false,
            first_seen_at: String::new(),
            last_seen_at: String::new(),
        };
        reg.load_persisted(
            &[row("old", &stored), row("bad", "{}"), row("live", &stored)],
            now,
        );
        assert!(!reg.is_online("old"));
        assert!(reg.get("old").unwrap().caps.set_next);
        assert!(reg.get("bad").is_none());
        assert!(reg.is_online("live"), "live entry is not overwritten");
    }

    #[tokio::test]
    async fn describe_renderer_reads_description_scpd_and_sink() {
        use axum::{
            routing::{get, post},
            Router,
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let device: &str = r#"<root><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><friendlyName>Den</friendlyName><UDN>uuid:DEN</UDN><serviceList>
<service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><controlURL>/avt</controlURL><SCPDURL>/scpd</SCPDURL></service>
<service><serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType><controlURL>/cm</controlURL></service>
</serviceList></device></root>"#;
        let app = Router::new()
            .route("/d.xml", get(move || async move { device }))
            .route("/scpd", get(|| async { "<scpd><actionList><action><name>Play</name></action><action><name>SetNextAVTransportURI</name></action></actionList></scpd>" }))
            .route("/cm", post(|| async { r#"<Envelope><Body><GetProtocolInfoResponse><Sink>http-get:*:audio/flac:*</Sink></GetProtocolInfoResponse></Body></Envelope>"# }));
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let soap = SoapClient::new();
        let (desc, caps) = describe_renderer(&soap, &format!("http://{addr}/d.xml"))
            .await
            .unwrap();
        assert_eq!(desc.udn, "den");
        assert!(caps.set_next);
        assert!(!caps.seek, "Seek not declared in this SCPD");
        assert!(caps.sink.accepts("audio/flac"));
        assert!(describe_renderer(&soap, &format!("http://{addr}/missing"))
            .await
            .is_err());
    }
}
