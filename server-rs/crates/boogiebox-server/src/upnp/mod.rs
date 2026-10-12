//! UPnP AV control point used by Play On: discovering MediaRenderers on the LAN and driving them over SOAP.
//!
//! The MediaServer side (`dlna.rs`) shares only the DIDL/XML text helpers in [`didl`]; the control point never
//! depends on the DLNA server being enabled.

pub mod device;
pub mod didl;
pub mod discovery;
pub mod soap;
pub mod xml;

use thiserror::Error;

/// Device type every Play On target must advertise.
pub const MEDIA_RENDERER_DEVICE: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";
/// AVTransport service type (transport control).
pub const AV_TRANSPORT_SERVICE: &str = "urn:schemas-upnp-org:service:AVTransport:1";
/// RenderingControl service type (volume/mute).
pub const RENDERING_CONTROL_SERVICE: &str = "urn:schemas-upnp-org:service:RenderingControl:1";
/// ConnectionManager service type (supported formats).
pub const CONNECTION_MANAGER_SERVICE: &str = "urn:schemas-upnp-org:service:ConnectionManager:1";

/// Errors raised while talking to renderers.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum UpnpError {
    /// The device sent XML we could not parse.
    #[error("invalid XML: {0}")]
    Xml(String),
    /// A required element/service is missing from a device document.
    #[error("missing {0}")]
    Missing(&'static str),
    /// HTTP-level failure (connect, timeout, non-success status without a SOAP fault).
    #[error("http: {0}")]
    Http(String),
    /// The renderer answered with a UPnP SOAP fault.
    #[error("UPnP error {code}: {description}")]
    Fault {
        /// UPnP `errorCode` (e.g. 701 transition not available, 714 illegal MIME type, 716 resource not found).
        code: u32,
        /// UPnP `errorDescription` (may be empty).
        description: String,
    },
    /// A URL from the device is unusable (not http, unparsable, or outside the LAN).
    #[error("bad url: {0}")]
    BadUrl(String),
}

impl UpnpError {
    /// UPnP fault code when this is a SOAP fault.
    pub fn fault_code(&self) -> Option<u32> {
        match self {
            UpnpError::Fault { code, .. } => Some(*code),
            _ => None,
        }
    }

    /// True for network-level failures (device unreachable / timed out), as opposed to a device refusal.
    pub fn is_transport_failure(&self) -> bool {
        matches!(self, UpnpError::Http(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fault_helpers_classify_errors() {
        let fault = UpnpError::Fault {
            code: 714,
            description: "Illegal MIME-type".into(),
        };
        assert_eq!(fault.fault_code(), Some(714));
        assert!(!fault.is_transport_failure());
        assert!(fault.to_string().contains("714"));

        let http = UpnpError::Http("timeout".into());
        assert_eq!(http.fault_code(), None);
        assert!(http.is_transport_failure());
        assert!(!UpnpError::Missing("x").is_transport_failure());
    }
}
