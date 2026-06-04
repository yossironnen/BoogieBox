//! Defines Rust server support logic for Cors.

/// Returns true if the given origin header value is from localhost,
/// a private LAN IP address, a `.local` hostname, or a single-label
/// hostname — matching the Node backend's CORS allowlist behavior.
/// Documents the Is Allowed Origin public API surface.
pub fn is_allowed_origin(origin: &str) -> bool {
    let host = match parse_origin_host(origin) {
        Some(h) => h,
        None => return false,
    };
    is_loopback_host(&host) || is_private_ip_host(&host) || is_local_hostname(&host)
}

fn parse_origin_host(origin: &str) -> Option<String> {
    let without_scheme = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))?;
    let authority = without_scheme.split('/').next()?;
    let host = if authority.starts_with('[') {
        // IPv6 literal: [::1]:port or [::1]
        authority
            .trim_start_matches('[')
            .split(']')
            .next()?
            .to_lowercase()
    } else {
        // hostname or IPv4, possibly with :port
        authority.split(':').next()?.to_lowercase()
    };
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

fn is_loopback_host(host: &str) -> bool {
    host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "::ffff:127.0.0.1"
}

fn is_private_ip_host(host: &str) -> bool {
    let ip = match host.parse::<std::net::Ipv4Addr>() {
        Ok(ip) => ip,
        Err(_) => return false,
    };
    let [a, b, ..] = ip.octets();
    a == 10
        || (a == 192 && b == 168)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 169 && b == 254)
}

fn is_local_hostname(host: &str) -> bool {
    // .local mDNS suffix or single-label (no dot → not a public domain)
    host.ends_with(".local") || !host.contains('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_localhost() {
        assert!(is_allowed_origin("http://localhost:3000"));
        assert!(is_allowed_origin("http://localhost"));
    }

    #[test]
    fn allows_loopback_ipv4() {
        assert!(is_allowed_origin("http://127.0.0.1:3001"));
    }

    #[test]
    fn allows_ipv6_loopback() {
        assert!(is_allowed_origin("http://[::1]:3001"));
    }

    #[test]
    fn allows_private_192_168() {
        assert!(is_allowed_origin("http://192.168.1.100:3001"));
    }

    #[test]
    fn allows_private_10_x() {
        assert!(is_allowed_origin("http://10.0.0.5:3001"));
    }

    #[test]
    fn allows_private_172_16() {
        assert!(is_allowed_origin("http://172.16.0.1:3001"));
    }

    #[test]
    fn allows_link_local() {
        assert!(is_allowed_origin("http://169.254.1.1:3001"));
    }

    #[test]
    fn allows_local_mdns() {
        assert!(is_allowed_origin("http://mymachine.local:3001"));
    }

    #[test]
    fn allows_single_label_hostname() {
        assert!(is_allowed_origin("http://mymachine:3001"));
    }

    #[test]
    fn rejects_public_domain() {
        assert!(!is_allowed_origin("https://example.com"));
        assert!(!is_allowed_origin("https://attacker.io"));
    }

    #[test]
    fn rejects_non_http_scheme() {
        assert!(!is_allowed_origin("ftp://localhost"));
    }

    #[test]
    fn rejects_public_ip() {
        assert!(!is_allowed_origin("http://8.8.8.8:3001"));
    }
}
