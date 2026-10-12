//! Parsing of renderer device descriptions (`LOCATION` XML), AVTransport SCPDs and
//! `ConnectionManager.GetProtocolInfo` sink lists.

use std::collections::BTreeSet;
use std::net::IpAddr;

use reqwest::Url;
use serde::{Deserialize, Serialize};

use super::xml::{self, XmlNode};
use super::{
    UpnpError, AV_TRANSPORT_SERVICE, CONNECTION_MANAGER_SERVICE, MEDIA_RENDERER_DEVICE,
    RENDERING_CONTROL_SERVICE,
};

/// Control endpoints of one UPnP service, as absolute URLs on the device's own host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceEndpoint {
    /// Full service type URN as advertised (versions above :1 are accepted).
    pub service_type: String,
    /// SOAP control URL.
    pub control_url: String,
    /// GENA event subscription URL (unused until GENA support lands).
    pub event_sub_url: Option<String>,
    /// Service description (SCPD) URL.
    pub scpd_url: Option<String>,
}

/// A device icon advertised in `iconList`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceIcon {
    /// Image MIME type (`image/png`, `image/jpeg`).
    pub mime: String,
    /// Width in pixels (0 when not advertised).
    pub width: u32,
    /// Height in pixels (0 when not advertised).
    pub height: u32,
    /// Absolute icon URL on the device's host.
    pub url: String,
}

/// Known renderer families that need vendor-specific handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Vendor {
    /// WiiM / LinkPlay-based streamers.
    Wiim,
    /// Any other UPnP AV renderer.
    Generic,
}

impl Vendor {
    /// Stable string form stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Vendor::Wiim => "wiim",
            Vendor::Generic => "generic",
        }
    }
}

/// Everything Play On needs from a renderer's device description.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RendererDescription {
    /// Normalized UDN (lower-case, without the `uuid:` prefix); stable device identity.
    pub udn: String,
    /// User-facing name set on the device.
    pub friendly_name: String,
    /// Manufacturer string, if any.
    pub manufacturer: Option<String>,
    /// Model name, if any.
    pub model_name: Option<String>,
    /// Model number/firmware string, if any.
    pub model_number: Option<String>,
    /// Detected vendor family.
    pub vendor: Vendor,
    /// Advertised icons (absolute URLs).
    pub icons: Vec<DeviceIcon>,
    /// AVTransport endpoints (required).
    pub av_transport: ServiceEndpoint,
    /// RenderingControl endpoints (volume/mute), when present.
    pub rendering_control: Option<ServiceEndpoint>,
    /// ConnectionManager endpoints (format discovery), when present.
    pub connection_manager: Option<ServiceEndpoint>,
}

impl RendererDescription {
    /// Best icon for the picker: PNG preferred, largest not exceeding 128px, else the smallest larger one.
    pub fn best_icon(&self) -> Option<&DeviceIcon> {
        let score = |i: &DeviceIcon| {
            let png = i.mime.eq_ignore_ascii_case("image/png");
            let size = i.width.max(i.height);
            let fits = size <= 128;
            // Higher is better: PNG, then fitting icons by size, then oversized icons smallest-first.
            (png, fits, if fits { size as i64 } else { -(size as i64) })
        };
        self.icons.iter().max_by_key(|i| score(i))
    }
}

/// Normalizes a UDN for use as a key: trims, strips `uuid:` and lower-cases.
pub fn normalize_udn(raw: &str) -> String {
    let t = raw.trim();
    let t = t
        .get(..5)
        .filter(|p| p.eq_ignore_ascii_case("uuid:"))
        .map(|_| &t[5..])
        .unwrap_or(t);
    t.trim().to_ascii_lowercase()
}

/// True for addresses Play On is willing to talk to: private, link-local, loopback or unique-local.
pub fn is_lan_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_loopback(),
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Validates a device-supplied URL: http(s) only, host must be a LAN IP literal or equal to `expected_host`.
pub fn validate_device_url(url: &Url, expected_host: Option<&str>) -> Result<(), UpnpError> {
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(UpnpError::BadUrl(format!("scheme {}", url.scheme())));
    }
    let host = url
        .host_str()
        .ok_or_else(|| UpnpError::BadUrl("no host".into()))?;
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Some(expected) = expected_host {
        if !bare.eq_ignore_ascii_case(expected.trim_start_matches('[').trim_end_matches(']')) {
            return Err(UpnpError::BadUrl(
                "host differs from the device address".into(),
            ));
        }
    }
    match bare.parse::<IpAddr>() {
        Ok(ip) if is_lan_ip(ip) => Ok(()),
        Ok(_) => Err(UpnpError::BadUrl("not a LAN address".into())),
        Err(_) => Err(UpnpError::BadUrl("host is not an IP address".into())),
    }
}

fn resolve(base: &Url, raw: &str, host: &str) -> Result<String, UpnpError> {
    let joined = base
        .join(raw.trim())
        .map_err(|e| UpnpError::BadUrl(e.to_string()))?;
    validate_device_url(&joined, Some(host))?;
    Ok(joined.to_string())
}

fn find_renderer_device(node: &XmlNode) -> Option<&XmlNode> {
    if node.name.eq_ignore_ascii_case("device") {
        let is_renderer = node
            .child_text("deviceType")
            .map(|t| t.starts_with("urn:schemas-upnp-org:device:MediaRenderer:"))
            .unwrap_or(false);
        if is_renderer {
            return Some(node);
        }
    }
    // Recurse into the root device and any embedded `deviceList` (e.g. Sonos ZonePlayer → MediaRenderer).
    node.children.iter().find_map(find_renderer_device)
}

fn service_endpoint(
    svc: &XmlNode,
    base: &Url,
    host: &str,
) -> Result<Option<ServiceEndpoint>, UpnpError> {
    let (Some(service_type), Some(control)) =
        (svc.child_text("serviceType"), svc.child_text("controlURL"))
    else {
        return Ok(None);
    };
    Ok(Some(ServiceEndpoint {
        service_type,
        control_url: resolve(base, &control, host)?,
        event_sub_url: svc
            .child_text("eventSubURL")
            .and_then(|u| resolve(base, &u, host).ok()),
        scpd_url: svc
            .child_text("SCPDURL")
            .and_then(|u| resolve(base, &u, host).ok()),
    }))
}

fn service_matches(service_type: &str, wanted: &str) -> bool {
    // Accept higher versions of the same service (`...:AVTransport:2`).
    let stem = wanted.trim_end_matches(char::is_numeric);
    service_type.starts_with(stem)
}

fn detect_vendor(manufacturer: Option<&str>, model: Option<&str>) -> Vendor {
    let m = manufacturer.unwrap_or("").to_ascii_lowercase();
    let model = model.unwrap_or("").to_ascii_lowercase();
    if m.contains("linkplay") || m.contains("wiim") || model.starts_with("wiim") {
        Vendor::Wiim
    } else {
        Vendor::Generic
    }
}

/// Parses a device description fetched from `location`.
///
/// All service and icon URLs are resolved against `URLBase` (or `location`) and must stay on the same host
/// as `location`, which itself must be a LAN address.
pub fn parse_device_description(
    xml_text: &str,
    location: &str,
) -> Result<RendererDescription, UpnpError> {
    let location_url = Url::parse(location).map_err(|e| UpnpError::BadUrl(e.to_string()))?;
    validate_device_url(&location_url, None)?;
    let host = location_url
        .host_str()
        .ok_or_else(|| UpnpError::BadUrl("no host".into()))?
        .to_string();

    let root = xml::parse(xml_text)?;
    let base = match root.child_text("URLBase") {
        Some(b) => {
            let u = Url::parse(&b).map_err(|e| UpnpError::BadUrl(e.to_string()))?;
            validate_device_url(&u, Some(&host))?;
            u
        }
        None => location_url.clone(),
    };
    let device = find_renderer_device(&root).ok_or(UpnpError::Missing("MediaRenderer device"))?;

    let udn = device
        .child_text("UDN")
        .map(|u| normalize_udn(&u))
        .filter(|u| !u.is_empty())
        .ok_or(UpnpError::Missing("UDN"))?;
    let manufacturer = device.child_text("manufacturer");
    let model_name = device.child_text("modelName");
    let friendly_name = device
        .child_text("friendlyName")
        .or_else(|| model_name.clone())
        .unwrap_or_else(|| "Unnamed speaker".to_string());

    let mut av_transport = None;
    let mut rendering_control = None;
    let mut connection_manager = None;
    if let Some(list) = device.child("serviceList") {
        for svc in list.children_named("service") {
            let Some(endpoint) = service_endpoint(svc, &base, &host)? else {
                continue;
            };
            if service_matches(&endpoint.service_type, AV_TRANSPORT_SERVICE) {
                av_transport.get_or_insert(endpoint);
            } else if service_matches(&endpoint.service_type, RENDERING_CONTROL_SERVICE) {
                rendering_control.get_or_insert(endpoint);
            } else if service_matches(&endpoint.service_type, CONNECTION_MANAGER_SERVICE) {
                connection_manager.get_or_insert(endpoint);
            }
        }
    }

    let icons = device
        .child("iconList")
        .map(|list| {
            list.children_named("icon")
                .filter_map(|icon| {
                    let url = resolve(&base, &icon.child_text("url")?, &host).ok()?;
                    Some(DeviceIcon {
                        mime: icon.child_text("mimetype").unwrap_or_default(),
                        width: icon
                            .child_text("width")
                            .and_then(|w| w.parse().ok())
                            .unwrap_or(0),
                        height: icon
                            .child_text("height")
                            .and_then(|h| h.parse().ok())
                            .unwrap_or(0),
                        url,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(RendererDescription {
        vendor: detect_vendor(manufacturer.as_deref(), model_name.as_deref()),
        udn,
        friendly_name,
        manufacturer,
        model_name,
        model_number: device.child_text("modelNumber"),
        icons,
        av_transport: av_transport.ok_or(UpnpError::Missing("AVTransport service"))?,
        rendering_control,
        connection_manager,
    })
}

/// Action names declared in a service description (SCPD).
pub fn parse_scpd_actions(xml_text: &str) -> Result<BTreeSet<String>, UpnpError> {
    let root = xml::parse(xml_text)?;
    let list = root
        .child("actionList")
        .ok_or(UpnpError::Missing("actionList"))?;
    Ok(list
        .children_named("action")
        .filter_map(|a| a.child_text("name"))
        .collect())
}

/// Formats a renderer accepts over HTTP, from its `GetProtocolInfo` `Sink` value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SinkCaps {
    /// Lower-cased MIME types (`audio/flac`, ...). `*` or `audio/*` means anything.
    pub mimes: BTreeSet<String>,
}

const MIME_FAMILIES: &[&[&str]] = &[
    &["audio/flac", "audio/x-flac"],
    &["audio/mpeg", "audio/mp3", "audio/mpeg3", "audio/x-mpeg"],
    &["audio/mp4", "audio/m4a", "audio/x-m4a"],
    &["audio/wav", "audio/x-wav", "audio/wave", "audio/vnd.wave"],
    &["audio/aiff", "audio/x-aiff"],
    &["audio/x-ms-wma", "audio/wma"],
    &["audio/x-ape", "audio/ape"],
    &["audio/aac", "audio/vnd.dlna.adts", "audio/aacp"],
];

fn base_mime(mime: &str) -> String {
    mime.split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

impl SinkCaps {
    /// Parses a comma-separated `protocol:network:contentFormat:additionalInfo` list (http-get entries only).
    pub fn parse(sink: &str) -> Self {
        let mimes = sink
            .split(',')
            .filter_map(|entry| {
                let mut parts = entry.trim().splitn(4, ':');
                let protocol = parts.next()?;
                let _network = parts.next()?;
                let format = parts.next()?;
                (protocol.eq_ignore_ascii_case("http-get") && !format.is_empty())
                    .then(|| base_mime(format))
            })
            .filter(|m| !m.is_empty())
            .collect();
        SinkCaps { mimes }
    }

    /// True when the renderer declares `mime` (or an alias of it, or a wildcard).
    ///
    /// An empty list (device did not answer `GetProtocolInfo`) is treated as "unknown" and returns false so the
    /// caller falls back to a safe transcode.
    pub fn accepts(&self, mime: &str) -> bool {
        self.listed_alias(mime).is_some()
    }

    /// The exact MIME spelling the renderer lists for `mime`'s family (e.g. `audio/m4a` for `audio/mp4`), so
    /// `protocolInfo` matches what the device expects. Wildcards return `mime` unchanged.
    pub fn listed_alias(&self, mime: &str) -> Option<String> {
        let mime = base_mime(mime);
        if self.mimes.contains(&mime) {
            return Some(mime);
        }
        if let Some(alias) = MIME_FAMILIES
            .iter()
            .find(|family| family.contains(&mime.as_str()))
            .and_then(|family| family.iter().find(|alias| self.mimes.contains(**alias)))
        {
            return Some((*alias).to_string());
        }
        (self.mimes.contains("*") || self.mimes.contains("audio/*")).then_some(mime)
    }
}

/// True when `mime` (e.g. `MEDIA_RENDERER_DEVICE`) is the device type a search target should match.
pub fn is_renderer_search_target(st: &str) -> bool {
    st.trim().eq_ignore_ascii_case(MEDIA_RENDERER_DEVICE)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIIM_DEVICE: &str = include_str!("../../tests/fixtures/upnp/wiim-ultra/device.xml");
    const WIIM_SCPD: &str =
        include_str!("../../tests/fixtures/upnp/wiim-ultra/avtransport-scpd.xml");
    const WIIM_PROTOCOL_INFO: &str =
        include_str!("../../tests/fixtures/upnp/wiim-ultra/get-protocol-info.xml");
    const LOCATION: &str = "http://10.0.0.134:49152/description.xml";

    #[test]
    fn parses_wiim_ultra_fixture() {
        let d = parse_device_description(WIIM_DEVICE, LOCATION).unwrap();
        assert_eq!(d.friendly_name, "WiiM Ultra");
        assert_eq!(d.vendor, Vendor::Wiim);
        assert_eq!(d.model_name.as_deref(), Some("WiiM Ultra Receiver"));
        assert_eq!(d.udn, "00000000-0000-4000-8000-0000000000aa");
        assert_eq!(
            d.av_transport.control_url,
            "http://10.0.0.134:49152/upnp/control/rendertransport1"
        );
        assert_eq!(
            d.rendering_control.as_ref().unwrap().control_url,
            "http://10.0.0.134:49152/upnp/control/rendercontrol1"
        );
        assert!(d.connection_manager.is_some());
        assert_eq!(d.icons.len(), 4);
        let icon = d.best_icon().unwrap();
        assert_eq!((icon.mime.as_str(), icon.width), ("image/png", 120));
        // Multi-line modelNumber is trimmed.
        assert!(!d.model_number.unwrap().ends_with('\n'));
    }

    #[test]
    fn wiim_scpd_declares_set_next() {
        let actions = parse_scpd_actions(WIIM_SCPD).unwrap();
        assert!(actions.contains("SetNextAVTransportURI"));
        assert!(actions.contains("GetPositionInfo"));
        assert!(actions.contains("Seek"));
    }

    #[test]
    fn wiim_sink_caps_cover_common_formats() {
        let sink = xml::parse(WIIM_PROTOCOL_INFO)
            .unwrap()
            .find("Sink")
            .unwrap()
            .text
            .clone();
        let caps = SinkCaps::parse(&sink);
        assert!(caps.accepts("audio/flac"));
        assert!(caps.accepts("audio/mpeg"));
        assert!(caps.accepts("audio/mp4")); // via audio/m4a alias
        assert!(caps.accepts("audio/wav"));
        assert!(caps.accepts("audio/ogg; codecs=opus"));
        assert!(!caps.accepts("audio/aiff"));
    }

    #[test]
    fn embedded_renderer_and_url_base_are_resolved() {
        let xml = r#"<root><URLBase>http://192.168.1.9:1400/</URLBase><device><deviceType>urn:schemas-upnp-org:device:ZonePlayer:1</deviceType><UDN>uuid:RINCON_1</UDN><deviceList><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><friendlyName>Kitchen</friendlyName><manufacturer>Sonos, Inc.</manufacturer><UDN>uuid:RINCON_1_MR</UDN><serviceList><service><serviceType>urn:schemas-upnp-org:service:AVTransport:2</serviceType><controlURL>MediaRenderer/AVTransport/Control</controlURL></service></serviceList></device></deviceList></device></root>"#;
        let d = parse_device_description(xml, "http://192.168.1.9:1400/xml/device_description.xml")
            .unwrap();
        assert_eq!(d.udn, "rincon_1_mr");
        assert_eq!(d.friendly_name, "Kitchen");
        assert_eq!(d.vendor, Vendor::Generic);
        assert_eq!(
            d.av_transport.control_url,
            "http://192.168.1.9:1400/MediaRenderer/AVTransport/Control"
        );
        assert!(d.rendering_control.is_none());
        assert!(d.icons.is_empty());
        assert!(d.best_icon().is_none());
    }

    #[test]
    fn rejects_non_renderers_missing_services_and_foreign_hosts() {
        let server = r#"<root><device><deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType><UDN>uuid:x</UDN></device></root>"#;
        assert_eq!(
            parse_device_description(server, LOCATION),
            Err(UpnpError::Missing("MediaRenderer device"))
        );
        let no_avt = r#"<root><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><UDN>uuid:x</UDN></device></root>"#;
        assert_eq!(
            parse_device_description(no_avt, LOCATION),
            Err(UpnpError::Missing("AVTransport service"))
        );
        let no_udn = r#"<root><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType></device></root>"#;
        assert_eq!(
            parse_device_description(no_udn, LOCATION),
            Err(UpnpError::Missing("UDN"))
        );
        let foreign = r#"<root><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><UDN>uuid:x</UDN><serviceList><service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><controlURL>http://10.0.0.99/ctl</controlURL></service></serviceList></device></root>"#;
        assert!(matches!(
            parse_device_description(foreign, LOCATION),
            Err(UpnpError::BadUrl(_))
        ));
        let foreign_base = r#"<root><URLBase>http://10.0.0.99/</URLBase><device/></root>"#;
        assert!(matches!(
            parse_device_description(foreign_base, LOCATION),
            Err(UpnpError::BadUrl(_))
        ));
        assert!(parse_device_description(server, "http://8.8.8.8/d.xml").is_err());
        assert!(parse_device_description(server, "not a url").is_err());
    }

    #[test]
    fn friendly_name_falls_back_to_model_then_placeholder() {
        let tmpl = |extra: &str| {
            format!(
                r#"<root><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><UDN>uuid:A</UDN>{extra}<serviceList><service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><controlURL>/c</controlURL></service><service><serviceType>urn:other</serviceType></service></serviceList></device></root>"#
            )
        };
        let d = parse_device_description(&tmpl("<modelName>Box</modelName>"), LOCATION).unwrap();
        assert_eq!(d.friendly_name, "Box");
        let d = parse_device_description(&tmpl(""), LOCATION).unwrap();
        assert_eq!(d.friendly_name, "Unnamed speaker");
    }

    #[test]
    fn normalize_udn_strips_prefix_and_case() {
        assert_eq!(normalize_udn(" uuid:ABC-1 "), "abc-1");
        assert_eq!(normalize_udn("UUID:abc"), "abc");
        assert_eq!(normalize_udn("abc"), "abc");
        assert_eq!(normalize_udn("uu"), "uu");
    }

    #[test]
    fn lan_ip_and_url_validation() {
        assert!(is_lan_ip("192.168.0.2".parse().unwrap()));
        assert!(is_lan_ip("10.1.2.3".parse().unwrap()));
        assert!(is_lan_ip("169.254.1.1".parse().unwrap()));
        assert!(is_lan_ip("127.0.0.1".parse().unwrap()));
        assert!(is_lan_ip("fd00::1".parse().unwrap()));
        assert!(is_lan_ip("fe80::1".parse().unwrap()));
        assert!(is_lan_ip("::1".parse().unwrap()));
        assert!(!is_lan_ip("8.8.8.8".parse().unwrap()));
        assert!(!is_lan_ip("2001:db8::1".parse().unwrap()));

        let ok = Url::parse("http://10.0.0.5:80/x").unwrap();
        assert!(validate_device_url(&ok, Some("10.0.0.5")).is_ok());
        assert!(validate_device_url(&ok, Some("10.0.0.6")).is_err());
        let v6 = Url::parse("http://[fe80::1]:80/x").unwrap();
        assert!(validate_device_url(&v6, Some("[fe80::1]")).is_ok());
        assert!(validate_device_url(&Url::parse("ftp://10.0.0.5/x").unwrap(), None).is_err());
        assert!(validate_device_url(&Url::parse("http://example.com/x").unwrap(), None).is_err());
        assert!(validate_device_url(&Url::parse("http://8.8.8.8/x").unwrap(), None).is_err());
    }

    #[test]
    fn sink_caps_parsing_edge_cases() {
        let caps =
            SinkCaps::parse("rtsp-rtp-udp:*:audio/flac:*, http-get:*:*:*,bogus,http-get:*::*");
        assert!(caps.mimes.contains("*"));
        assert!(caps.accepts("audio/anything"));
        assert!(!SinkCaps::parse("").accepts("audio/flac"));
        let only_flac = SinkCaps::parse("http-get:*:audio/x-flac:*");
        assert!(only_flac.accepts("audio/flac"));
        assert!(!only_flac.accepts("audio/mpeg"));
        assert!(!only_flac.accepts("video/mp4"));
    }

    #[test]
    fn listed_alias_matches_device_spelling() {
        let wiim = SinkCaps::parse(
            "http-get:*:audio/m4a:*,http-get:*:audio/mp3:*,http-get:*:audio/flac:*",
        );
        assert_eq!(wiim.listed_alias("audio/mp4").as_deref(), Some("audio/m4a"));
        assert_eq!(
            wiim.listed_alias("audio/mpeg").as_deref(),
            Some("audio/mp3")
        );
        assert_eq!(
            wiim.listed_alias("audio/flac; x=1").as_deref(),
            Some("audio/flac")
        );
        assert_eq!(wiim.listed_alias("audio/aiff"), None);
        let any = SinkCaps::parse("http-get:*:audio/*:*");
        assert_eq!(
            any.listed_alias("audio/aiff").as_deref(),
            Some("audio/aiff")
        );
    }

    #[test]
    fn misc_helpers() {
        assert_eq!(Vendor::Wiim.as_str(), "wiim");
        assert_eq!(Vendor::Generic.as_str(), "generic");
        assert_eq!(detect_vendor(None, Some("WiiM Pro")), Vendor::Wiim);
        assert!(is_renderer_search_target(
            "urn:schemas-upnp-org:device:MediaRenderer:1"
        ));
        assert!(!is_renderer_search_target("upnp:rootdevice"));
        assert!(parse_scpd_actions("<scpd/>").is_err());
    }
}
