//! SOAP client for the UPnP AV actions Play On uses (AVTransport, RenderingControl, ConnectionManager).

use std::time::Duration;

use super::device::{ServiceEndpoint, SinkCaps};
use super::didl::{format_duration, parse_duration, xml_escape};
use super::xml::{self, XmlNode, MAX_XML_BYTES};
use super::UpnpError;

/// Per-action timeout. Renderers answer in tens of milliseconds; anything slower is treated as unreachable.
pub const SOAP_TIMEOUT: Duration = Duration::from_secs(3);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const SEND_RETRY_DELAY: Duration = Duration::from_millis(150);

/// Builds the SOAP envelope for `action` with already-unescaped `args` (escaped here).
pub fn build_envelope(service_type: &str, action: &str, args: &[(&str, &str)]) -> String {
    let mut body = String::new();
    for (name, value) in args {
        body.push_str(&format!("<{name}>{}</{name}>", xml_escape(value)));
    }
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action} xmlns:u="{}">{body}</u:{action}></s:Body></s:Envelope>"#,
        xml_escape(service_type)
    )
}

/// Extracts a SOAP fault (`UPnPError`) from a response body, if it contains one.
pub fn parse_fault(body: &str) -> Option<UpnpError> {
    let root = xml::parse(body).ok()?;
    let fault = root.find("Fault")?;
    let upnp = fault.find("UPnPError");
    let code = upnp
        .and_then(|u| u.child_text("errorCode"))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let description = upnp
        .and_then(|u| u.child_text("errorDescription"))
        .or_else(|| fault.child_text("faultstring"))
        .unwrap_or_default();
    Some(UpnpError::Fault { code, description })
}

/// Parses a successful response and returns the `<u:{action}Response>` element.
pub fn parse_response(body: &str, action: &str) -> Result<XmlNode, UpnpError> {
    let root = xml::parse(body)?;
    let wanted = format!("{action}Response");
    root.find(&wanted)
        .cloned()
        .ok_or(UpnpError::Missing("SOAP response element"))
}

/// AVTransport `CurrentTransportState`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportState {
    /// `PLAYING`
    Playing,
    /// `PAUSED_PLAYBACK`
    Paused,
    /// `STOPPED`
    Stopped,
    /// `TRANSITIONING` (buffering/loading)
    Transitioning,
    /// `NO_MEDIA_PRESENT`
    NoMedia,
    /// Vendor-specific or unknown value.
    Other(String),
}

impl TransportState {
    /// Parses the UPnP state string.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_uppercase().as_str() {
            "PLAYING" => Self::Playing,
            "PAUSED_PLAYBACK" | "PAUSED" => Self::Paused,
            "STOPPED" => Self::Stopped,
            "TRANSITIONING" => Self::Transitioning,
            "NO_MEDIA_PRESENT" => Self::NoMedia,
            other => Self::Other(other.to_string()),
        }
    }
}

/// Result of `GetTransportInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportInfo {
    /// Current transport state.
    pub state: TransportState,
    /// `CurrentTransportStatus` (`OK` or `ERROR_OCCURRED`).
    pub status: String,
}

/// Result of `GetPositionInfo`.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionInfo {
    /// URI of the item being played (empty strings become `None`).
    pub track_uri: Option<String>,
    /// Elapsed seconds in the current item.
    pub rel_time: Option<f64>,
    /// Item duration in seconds.
    pub duration: Option<f64>,
}

/// Result of `GetMediaInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaInfo {
    /// `CurrentURI`
    pub current_uri: Option<String>,
    /// `NextURI`
    pub next_uri: Option<String>,
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.is_empty() && v != "NOT_IMPLEMENTED")
}

fn parse_bool(s: &str) -> bool {
    matches!(s.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes")
}

/// HTTP client tuned for LAN renderers: short timeouts, no proxy (a system proxy must never see LAN traffic).
#[derive(Clone, Debug)]
pub struct SoapClient {
    http: reqwest::Client,
}

impl SoapClient {
    /// Builds the LAN client.
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .no_proxy()
            // Renderer HTTP stacks (WiiM/LinkPlay among them) drop idle keep-alive connections without
            // closing them cleanly; reusing one fails with "error sending request". One connection per call.
            .pool_max_idle_per_host(0)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(SOAP_TIMEOUT)
            .user_agent(concat!(
                "BoogieBox/",
                env!("CARGO_PKG_VERSION"),
                " UPnP/1.0"
            ))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { http }
    }

    /// Underlying HTTP client (also used for device-description and icon fetches).
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// GETs a device document (description/SCPD/icon) with the size cap applied.
    pub async fn get_bytes(&self, url: &str, max: usize) -> Result<Vec<u8>, UpnpError> {
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| UpnpError::Http(e.without_url().to_string()))?;
        if !resp.status().is_success() {
            return Err(UpnpError::Http(format!(
                "status {}",
                resp.status().as_u16()
            )));
        }
        read_capped(resp, max).await
    }

    /// GETs a device XML document as text.
    pub async fn get_text(&self, url: &str) -> Result<String, UpnpError> {
        let bytes = self.get_bytes(url, MAX_XML_BYTES).await?;
        String::from_utf8(bytes).map_err(|_| UpnpError::Xml("not UTF-8".into()))
    }

    /// Invokes `action` and returns the parsed `{action}Response` element.
    pub async fn call(
        &self,
        endpoint: &ServiceEndpoint,
        action: &str,
        args: &[(&str, &str)],
    ) -> Result<XmlNode, UpnpError> {
        let body = build_envelope(&endpoint.service_type, action, args);
        let send = || {
            self.http
                .post(&endpoint.control_url)
                .header("Content-Type", "text/xml; charset=\"utf-8\"")
                .header("Connection", "close")
                .header(
                    "SOAPACTION",
                    format!("\"{}#{}\"", endpoint.service_type, action),
                )
                .body(body.clone())
                .send()
        };
        // A send failure (connection refused/reset before any response) is retried once: every UPnP action
        // Play On uses is safe to repeat. Timeouts are not retried.
        let resp = match send().await {
            Ok(r) => Ok(r),
            Err(e) if !e.is_timeout() => {
                tokio::time::sleep(SEND_RETRY_DELAY).await;
                send().await
            }
            Err(e) => Err(e),
        }
        .map_err(|e| UpnpError::Http(e.without_url().to_string()))?;
        let status = resp.status();
        let bytes = read_capped(resp, MAX_XML_BYTES).await?;
        let text = String::from_utf8_lossy(&bytes);
        if let Some(fault) = parse_fault(&text) {
            return Err(fault);
        }
        if !status.is_success() {
            return Err(UpnpError::Http(format!("status {}", status.as_u16())));
        }
        parse_response(&text, action)
    }

    // ── AVTransport ──────────────────────────────────────────────────────────

    /// `SetAVTransportURI` — loads `uri` (with DIDL-Lite `metadata`) as the current item.
    pub async fn set_av_transport_uri(
        &self,
        avt: &ServiceEndpoint,
        uri: &str,
        metadata: &str,
    ) -> Result<(), UpnpError> {
        self.call(
            avt,
            "SetAVTransportURI",
            &[
                ("InstanceID", "0"),
                ("CurrentURI", uri),
                ("CurrentURIMetaData", metadata),
            ],
        )
        .await
        .map(|_| ())
    }

    /// `SetNextAVTransportURI` — queues `uri` for a gapless transition (empty `uri` clears it).
    pub async fn set_next_av_transport_uri(
        &self,
        avt: &ServiceEndpoint,
        uri: &str,
        metadata: &str,
    ) -> Result<(), UpnpError> {
        self.call(
            avt,
            "SetNextAVTransportURI",
            &[
                ("InstanceID", "0"),
                ("NextURI", uri),
                ("NextURIMetaData", metadata),
            ],
        )
        .await
        .map(|_| ())
    }

    /// `Play` at normal speed.
    pub async fn play(&self, avt: &ServiceEndpoint) -> Result<(), UpnpError> {
        self.call(avt, "Play", &[("InstanceID", "0"), ("Speed", "1")])
            .await
            .map(|_| ())
    }

    /// `Pause`.
    pub async fn pause(&self, avt: &ServiceEndpoint) -> Result<(), UpnpError> {
        self.call(avt, "Pause", &[("InstanceID", "0")])
            .await
            .map(|_| ())
    }

    /// `Stop`.
    pub async fn stop(&self, avt: &ServiceEndpoint) -> Result<(), UpnpError> {
        self.call(avt, "Stop", &[("InstanceID", "0")])
            .await
            .map(|_| ())
    }

    /// `Seek` to an absolute position in the current item (`REL_TIME`).
    pub async fn seek(&self, avt: &ServiceEndpoint, seconds: f64) -> Result<(), UpnpError> {
        let target = format_duration(seconds);
        self.call(
            avt,
            "Seek",
            &[
                ("InstanceID", "0"),
                ("Unit", "REL_TIME"),
                ("Target", &target),
            ],
        )
        .await
        .map(|_| ())
    }

    /// `GetTransportInfo`.
    pub async fn get_transport_info(
        &self,
        avt: &ServiceEndpoint,
    ) -> Result<TransportInfo, UpnpError> {
        let r = self
            .call(avt, "GetTransportInfo", &[("InstanceID", "0")])
            .await?;
        Ok(TransportInfo {
            state: TransportState::parse(
                &r.child_text("CurrentTransportState").unwrap_or_default(),
            ),
            status: r.child_text("CurrentTransportStatus").unwrap_or_default(),
        })
    }

    /// `GetPositionInfo`.
    pub async fn get_position_info(
        &self,
        avt: &ServiceEndpoint,
    ) -> Result<PositionInfo, UpnpError> {
        let r = self
            .call(avt, "GetPositionInfo", &[("InstanceID", "0")])
            .await?;
        Ok(PositionInfo {
            track_uri: non_empty(r.child_text("TrackURI")),
            rel_time: r.child_text("RelTime").as_deref().and_then(parse_duration),
            duration: r
                .child_text("TrackDuration")
                .as_deref()
                .and_then(parse_duration)
                .filter(|d| *d > 0.0),
        })
    }

    /// `GetMediaInfo`.
    pub async fn get_media_info(&self, avt: &ServiceEndpoint) -> Result<MediaInfo, UpnpError> {
        let r = self
            .call(avt, "GetMediaInfo", &[("InstanceID", "0")])
            .await?;
        Ok(MediaInfo {
            current_uri: non_empty(r.child_text("CurrentURI")),
            next_uri: non_empty(r.child_text("NextURI")),
        })
    }

    // ── RenderingControl ─────────────────────────────────────────────────────

    /// `GetVolume` (Master channel), 0–100.
    pub async fn get_volume(&self, rc: &ServiceEndpoint) -> Result<u8, UpnpError> {
        let r = self
            .call(
                rc,
                "GetVolume",
                &[("InstanceID", "0"), ("Channel", "Master")],
            )
            .await?;
        r.child_text("CurrentVolume")
            .and_then(|v| v.parse::<u32>().ok())
            .map(|v| v.min(100) as u8)
            .ok_or(UpnpError::Missing("CurrentVolume"))
    }

    /// `SetVolume` (Master channel), clamped to 0–100.
    pub async fn set_volume(&self, rc: &ServiceEndpoint, volume: u8) -> Result<(), UpnpError> {
        let v = volume.min(100).to_string();
        self.call(
            rc,
            "SetVolume",
            &[
                ("InstanceID", "0"),
                ("Channel", "Master"),
                ("DesiredVolume", &v),
            ],
        )
        .await
        .map(|_| ())
    }

    /// `GetMute` (Master channel).
    pub async fn get_mute(&self, rc: &ServiceEndpoint) -> Result<bool, UpnpError> {
        let r = self
            .call(rc, "GetMute", &[("InstanceID", "0"), ("Channel", "Master")])
            .await?;
        Ok(r.child_text("CurrentMute")
            .map(|v| parse_bool(&v))
            .unwrap_or(false))
    }

    /// `SetMute` (Master channel).
    pub async fn set_mute(&self, rc: &ServiceEndpoint, mute: bool) -> Result<(), UpnpError> {
        self.call(
            rc,
            "SetMute",
            &[
                ("InstanceID", "0"),
                ("Channel", "Master"),
                ("DesiredMute", if mute { "1" } else { "0" }),
            ],
        )
        .await
        .map(|_| ())
    }

    // ── ConnectionManager ────────────────────────────────────────────────────

    /// `GetProtocolInfo` → accepted sink formats.
    pub async fn get_protocol_info(&self, cm: &ServiceEndpoint) -> Result<SinkCaps, UpnpError> {
        let r = self.call(cm, "GetProtocolInfo", &[]).await?;
        Ok(SinkCaps::parse(&r.child_text("Sink").unwrap_or_default()))
    }
}

impl Default for SoapClient {
    fn default() -> Self {
        Self::new()
    }
}

async fn read_capped(mut resp: reqwest::Response, max: usize) -> Result<Vec<u8>, UpnpError> {
    if resp.content_length().is_some_and(|len| len as usize > max) {
        return Err(UpnpError::Http("response too large".into()));
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| UpnpError::Http(e.without_url().to_string()))?
    {
        if out.len() + chunk.len() > max {
            return Err(UpnpError::Http("response too large".into()));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use axum::{body::Bytes, http::HeaderMap, routing::post, Router};
    use std::sync::{Arc, Mutex};

    const POSITION_FIXTURE: &str = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body>
<u:GetPositionInfoResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
<Track>1</Track>
<TrackDuration>00:04:05</TrackDuration>
<TrackMetaData>&lt;DIDL-Lite&gt;&lt;item&gt;&lt;dc:title&gt;A &amp;amp; B&lt;/dc:title&gt;&lt;/item&gt;&lt;/DIDL-Lite&gt;</TrackMetaData>
<TrackURI>http://10.0.0.2:3001/media/r/abc</TrackURI>
<RelTime>00:01:02</RelTime>
<AbsTime>NOT_IMPLEMENTED</AbsTime>
</u:GetPositionInfoResponse>
</s:Body> </s:Envelope>"#;

    const FAULT_FIXTURE: &str = r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><s:Fault><faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail><UPnPError xmlns="urn:schemas-upnp-org:control-1-0"><errorCode>714</errorCode><errorDescription>Illegal MIME-type</errorDescription></UPnPError></detail></s:Fault></s:Body></s:Envelope>"#;

    fn ok_response(action: &str, inner: &str) -> String {
        format!(
            r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><u:{action}Response xmlns:u="urn:x">{inner}</u:{action}Response></s:Body></s:Envelope>"#
        )
    }

    /// Minimal scripted renderer: answers each SOAP action from a table and records the requests.
    pub(crate) async fn spawn_mock(
        responder: impl Fn(&str, &str) -> (u16, String) + Send + Sync + 'static,
    ) -> (String, Arc<Mutex<Vec<(String, String)>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let responder = Arc::new(responder);
        let app = Router::new().route(
            "/ctl",
            post({
                let log = log.clone();
                move |headers: HeaderMap, body: Bytes| {
                    let log = log.clone();
                    let responder = responder.clone();
                    async move {
                        let action = headers
                            .get("SOAPACTION")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("")
                            .trim_matches('"')
                            .rsplit('#')
                            .next()
                            .unwrap_or("")
                            .to_string();
                        let body = String::from_utf8_lossy(&body).to_string();
                        log.lock().unwrap().push((action.clone(), body.clone()));
                        let (status, text) = responder(&action, &body);
                        (axum::http::StatusCode::from_u16(status).unwrap(), text)
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{addr}/ctl"), log)
    }

    fn endpoint(url: &str) -> ServiceEndpoint {
        ServiceEndpoint {
            service_type: "urn:schemas-upnp-org:service:AVTransport:1".into(),
            control_url: url.into(),
            event_sub_url: None,
            scpd_url: None,
        }
    }

    #[test]
    fn envelope_escapes_arguments_including_double_escaped_didl() {
        let env = build_envelope(
            "urn:schemas-upnp-org:service:AVTransport:1",
            "SetAVTransportURI",
            &[
                ("CurrentURI", "http://h/a?x=1&y=2"),
                (
                    "CurrentURIMetaData",
                    "<DIDL-Lite><dc:title>Tom &amp; Jerry</dc:title></DIDL-Lite>",
                ),
            ],
        );
        assert!(env.contains(
            "<u:SetAVTransportURI xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\">"
        ));
        assert!(env.contains("<CurrentURI>http://h/a?x=1&amp;y=2</CurrentURI>"));
        assert!(env.contains("&lt;dc:title&gt;Tom &amp;amp; Jerry&lt;/dc:title&gt;"));
        // Round-trip: the renderer would see the original DIDL after one unescape.
        let parsed = xml::parse(&env).unwrap();
        assert_eq!(
            parsed.find("CurrentURIMetaData").unwrap().text,
            "<DIDL-Lite><dc:title>Tom &amp; Jerry</dc:title></DIDL-Lite>"
        );
    }

    #[test]
    fn fault_parsing() {
        assert_eq!(
            parse_fault(FAULT_FIXTURE),
            Some(UpnpError::Fault {
                code: 714,
                description: "Illegal MIME-type".into()
            })
        );
        let bare =
            r#"<Envelope><Body><Fault><faultstring>oops</faultstring></Fault></Body></Envelope>"#;
        assert_eq!(
            parse_fault(bare),
            Some(UpnpError::Fault {
                code: 0,
                description: "oops".into()
            })
        );
        assert_eq!(parse_fault(&ok_response("Play", "")), None);
        assert_eq!(parse_fault("not xml"), None);
    }

    #[test]
    fn response_parsing() {
        let r = parse_response(POSITION_FIXTURE, "GetPositionInfo").unwrap();
        assert_eq!(r.child_text("RelTime").as_deref(), Some("00:01:02"));
        assert_eq!(
            parse_response(POSITION_FIXTURE, "Play"),
            Err(UpnpError::Missing("SOAP response element"))
        );
    }

    #[test]
    fn transport_state_parsing() {
        assert_eq!(TransportState::parse("PLAYING"), TransportState::Playing);
        assert_eq!(
            TransportState::parse("paused_playback"),
            TransportState::Paused
        );
        assert_eq!(TransportState::parse("PAUSED"), TransportState::Paused);
        assert_eq!(TransportState::parse("STOPPED"), TransportState::Stopped);
        assert_eq!(
            TransportState::parse("TRANSITIONING"),
            TransportState::Transitioning
        );
        assert_eq!(
            TransportState::parse("NO_MEDIA_PRESENT"),
            TransportState::NoMedia
        );
        assert_eq!(
            TransportState::parse("x"),
            TransportState::Other("X".into())
        );
        assert!(parse_bool("1") && parse_bool("True") && !parse_bool("0"));
    }

    #[tokio::test]
    async fn typed_actions_round_trip_against_mock_renderer() {
        let (url, log) = spawn_mock(|action, _body| match action {
            "GetPositionInfo" => (200, POSITION_FIXTURE.to_string()),
            "GetTransportInfo" => (200, ok_response(action, "<CurrentTransportState>PLAYING</CurrentTransportState><CurrentTransportStatus>OK</CurrentTransportStatus>")),
            "GetMediaInfo" => (200, ok_response(action, "<CurrentURI>http://x/1</CurrentURI><NextURI></NextURI>")),
            "GetVolume" => (200, ok_response(action, "<CurrentVolume>130</CurrentVolume>")),
            "GetMute" => (200, ok_response(action, "<CurrentMute>1</CurrentMute>")),
            "GetProtocolInfo" => (200, ok_response(action, "<Source></Source><Sink>http-get:*:audio/flac:*</Sink>")),
            "SetAVTransportURI" => (500, FAULT_FIXTURE.to_string()),
            "Stop" => (500, "boom".to_string()),
            _ => (200, ok_response(action, "")),
        })
        .await;
        let ep = endpoint(&url);
        let c = SoapClient::new();

        let pos = c.get_position_info(&ep).await.unwrap();
        assert_eq!(
            pos.track_uri.as_deref(),
            Some("http://10.0.0.2:3001/media/r/abc")
        );
        assert_eq!(pos.rel_time, Some(62.0));
        assert_eq!(pos.duration, Some(245.0));
        assert_eq!(
            c.get_transport_info(&ep).await.unwrap().state,
            TransportState::Playing
        );
        let media = c.get_media_info(&ep).await.unwrap();
        assert_eq!(media.current_uri.as_deref(), Some("http://x/1"));
        assert_eq!(media.next_uri, None);
        assert_eq!(c.get_volume(&ep).await.unwrap(), 100);
        assert!(c.get_mute(&ep).await.unwrap());
        assert!(c
            .get_protocol_info(&ep)
            .await
            .unwrap()
            .accepts("audio/flac"));

        c.play(&ep).await.unwrap();
        c.pause(&ep).await.unwrap();
        c.seek(&ep, 75.4).await.unwrap();
        c.set_volume(&ep, 250).await.unwrap();
        c.set_mute(&ep, false).await.unwrap();
        c.set_next_av_transport_uri(&ep, "", "").await.unwrap();
        assert_eq!(
            c.set_av_transport_uri(&ep, "http://x", "")
                .await
                .unwrap_err()
                .fault_code(),
            Some(714)
        );
        assert!(c.stop(&ep).await.unwrap_err().is_transport_failure());

        let log = log.lock().unwrap();
        let body_of = |a: &str| {
            log.iter()
                .find(|(x, _)| x == a)
                .map(|(_, b)| b.clone())
                .unwrap()
        };
        assert!(body_of("Seek").contains("<Unit>REL_TIME</Unit><Target>0:01:15</Target>"));
        assert!(body_of("SetVolume").contains("<DesiredVolume>100</DesiredVolume>"));
        assert!(body_of("SetMute").contains("<DesiredMute>0</DesiredMute>"));
        assert!(body_of("Play").contains("<Speed>1</Speed>"));
    }

    /// Renderers drop connections abruptly (WiiM, Phase 0.3 live run): the first connection is closed without a
    /// response, the retry on a fresh connection succeeds.
    #[tokio::test]
    async fn send_failure_is_retried_once_on_a_new_connection() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let connections = Arc::new(Mutex::new(0u32));
        let counter = connections.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let n = {
                    let mut c = counter.lock().unwrap();
                    *c += 1;
                    *c
                };
                let mut buf = vec![0u8; 8192];
                let _ = sock.read(&mut buf).await;
                if n == 1 {
                    drop(sock); // reset without a response
                    continue;
                }
                let body = ok_response("Play", "");
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });
        let c = SoapClient::new();
        c.play(&endpoint(&format!("http://{addr}/ctl")))
            .await
            .unwrap();
        assert_eq!(*connections.lock().unwrap(), 2);
    }

    #[tokio::test]
    async fn missing_fields_and_unreachable_devices_are_errors() {
        let (url, _) = spawn_mock(|action, _| (200, ok_response(action, ""))).await;
        let ep = endpoint(&url);
        let c = SoapClient::default();
        assert_eq!(
            c.get_volume(&ep).await,
            Err(UpnpError::Missing("CurrentVolume"))
        );
        assert!(!c.get_mute(&ep).await.unwrap());
        let pos = c.get_position_info(&ep).await.unwrap();
        assert_eq!(
            (pos.track_uri, pos.rel_time, pos.duration),
            (None, None, None)
        );

        let dead = endpoint("http://127.0.0.1:9/ctl");
        assert!(c.play(&dead).await.unwrap_err().is_transport_failure());
        assert!(c.get_text("http://127.0.0.1:9/x").await.is_err());
    }

    #[tokio::test]
    async fn get_helpers_enforce_status_and_size() {
        let app = Router::new()
            .route("/ok", axum::routing::get(|| async { "<root/>" }))
            .route("/big", axum::routing::get(|| async { "x".repeat(2048) }))
            .route("/bin", axum::routing::get(|| async { vec![0xffu8, 0xfe] }))
            .route(
                "/missing",
                axum::routing::get(|| async { axum::http::StatusCode::NOT_FOUND }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let c = SoapClient::new();
        assert_eq!(
            c.get_text(&format!("http://{addr}/ok")).await.unwrap(),
            "<root/>"
        );
        assert!(c
            .get_bytes(&format!("http://{addr}/big"), 1024)
            .await
            .is_err());
        assert!(c.get_text(&format!("http://{addr}/bin")).await.is_err());
        assert!(c.get_text(&format!("http://{addr}/missing")).await.is_err());
        assert!(c
            .http()
            .get(format!("http://{addr}/ok"))
            .send()
            .await
            .is_ok());
    }
}
