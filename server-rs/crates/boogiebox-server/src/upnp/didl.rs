//! DIDL-Lite / XML text helpers shared by the DLNA MediaServer (`dlna.rs`) and the Play On control point.

/// Escapes the five XML special characters for use in element text or attribute values.
pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Formats whole seconds as UPnP `H:MM:SS` (hours are not zero-padded).
pub fn format_duration(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds as u64
    } else {
        0
    };
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    format!("{}:{:02}:{:02}", h, m, s)
}

/// Parses a UPnP time value (`H+:MM:SS[.F+]` or `H+:MM:SS[.F0/F1]`) into seconds.
///
/// Returns `None` for `NOT_IMPLEMENTED`, empty strings and anything malformed; renderers report all three.
pub fn parse_duration(value: &str) -> Option<f64> {
    let v = value.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("NOT_IMPLEMENTED") {
        return None;
    }
    let (v, negative) = match v.strip_prefix('-') {
        Some(rest) => (rest, true),
        None => (v.strip_prefix('+').unwrap_or(v), false),
    };
    let mut parts = v.split(':');
    let h: f64 = parts.next()?.trim().parse::<u64>().ok()? as f64;
    let m: f64 = parts.next()?.trim().parse::<u64>().ok()? as f64;
    let sec_part = parts.next()?.trim();
    if parts.next().is_some() || m >= 60.0 {
        return None;
    }
    let (whole, frac) = match sec_part.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (sec_part, None),
    };
    let s: f64 = whole.parse::<u64>().ok()? as f64;
    if s >= 60.0 {
        return None;
    }
    let frac_secs = match frac {
        // `F0/F1` fraction form: numerator/denominator.
        Some(f) if f.contains('/') => {
            let (n, d) = f.split_once('/')?;
            let n: f64 = n.parse().ok()?;
            let d: f64 = d.parse().ok()?;
            if d <= 0.0 {
                return None;
            }
            n / d
        }
        Some(f) if !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()) => {
            format!("0.{f}").parse::<f64>().ok()?
        }
        Some(_) => return None,
        None => 0.0,
    };
    let total = h * 3600.0 + m * 60.0 + s + frac_secs;
    Some(if negative { -total } else { total })
}

/// Wraps DIDL-Lite items in the standard root element with the dc/upnp/dlna namespaces.
pub fn wrap_didl(inner: &str) -> String {
    format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/" xmlns:dlna="urn:schemas-dlna-org:metadata-1-0/">{inner}</DIDL-Lite>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escape_replaces_all_special_chars() {
        assert_eq!(
            xml_escape(r#"a & <b> "c" 'd'"#),
            "a &amp; &lt;b&gt; &quot;c&quot; &apos;d&apos;"
        );
        assert_eq!(xml_escape("plain"), "plain");
    }

    #[test]
    fn format_duration_handles_bounds() {
        assert_eq!(format_duration(0.0), "0:00:00");
        assert_eq!(format_duration(59.9), "0:00:59");
        assert_eq!(format_duration(3661.0), "1:01:01");
        assert_eq!(format_duration(-5.0), "0:00:00");
        assert_eq!(format_duration(f64::NAN), "0:00:00");
    }

    #[test]
    fn parse_duration_accepts_renderer_formats() {
        assert_eq!(parse_duration("0:03:45"), Some(225.0));
        assert_eq!(parse_duration("01:18:21"), Some(4701.0));
        assert_eq!(parse_duration("0:00:01.500"), Some(1.5));
        assert_eq!(parse_duration("0:00:02.1/4"), Some(2.25));
        assert_eq!(parse_duration("+0:00:03"), Some(3.0));
        assert_eq!(parse_duration("-0:00:03"), Some(-3.0));
        assert_eq!(parse_duration(" 10:00:00 "), Some(36000.0));
    }

    #[test]
    fn parse_duration_rejects_unknown_and_malformed() {
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("NOT_IMPLEMENTED"), None);
        assert_eq!(parse_duration("1:2"), None);
        assert_eq!(parse_duration("0:60:00"), None);
        assert_eq!(parse_duration("0:00:60"), None);
        assert_eq!(parse_duration("0:00:01:00"), None);
        assert_eq!(parse_duration("a:b:c"), None);
        assert_eq!(parse_duration("0:00:01.x"), None);
        assert_eq!(parse_duration("0:00:01.1/0"), None);
    }

    #[test]
    fn wrap_didl_declares_namespaces() {
        let didl = wrap_didl("<item/>");
        assert!(didl.starts_with("<DIDL-Lite xmlns="));
        assert!(didl.contains("xmlns:upnp="));
        assert!(didl.ends_with("<item/></DIDL-Lite>"));
    }
}
