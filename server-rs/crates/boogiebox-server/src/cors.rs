//! Defines Rust server support logic for Cors.

use std::collections::HashSet;

/// Returns true when an origin is trusted for credentialed cross-origin calls.
pub fn is_allowed_origin(origin: &str) -> bool {
    is_allowed_origin_with_config(origin, &allowed_origins_from_env())
}

/// Returns true when an origin is loopback or explicitly allowlisted.
pub fn is_allowed_origin_with_config(origin: &str, configured_origins: &HashSet<String>) -> bool {
    let normalized = match normalize_origin(origin) {
        Some(value) => value,
        None => return false,
    };
    if configured_origins.contains(&normalized) {
        return true;
    }

    let host = match parse_origin_host(origin) {
        Some(h) => h,
        None => return false,
    };
    is_loopback_host(&host)
}

/// Parses `BOOGIEBOX_ALLOWED_ORIGINS` as a comma-separated exact-origin allowlist.
pub fn allowed_origins_from_env() -> HashSet<String> {
    std::env::var("BOOGIEBOX_ALLOWED_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .filter_map(normalize_origin)
        .collect()
}

fn normalize_origin(origin: &str) -> Option<String> {
    let trimmed = origin.trim().trim_end_matches('/');
    if parse_origin_host(trimmed).is_some() {
        Some(trimmed.to_lowercase())
    } else {
        None
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_loopback_localhost() {
        assert!(is_allowed_origin_with_config(
            "http://localhost:3000",
            &HashSet::new()
        ));
        assert!(is_allowed_origin_with_config(
            "http://localhost",
            &HashSet::new()
        ));
    }

    #[test]
    fn allows_loopback_ipv4() {
        assert!(is_allowed_origin_with_config(
            "http://127.0.0.1:3001",
            &HashSet::new()
        ));
    }

    #[test]
    fn allows_ipv6_loopback() {
        assert!(is_allowed_origin_with_config(
            "http://[::1]:3001",
            &HashSet::new()
        ));
    }

    #[test]
    fn rejects_private_lan_origin_by_default() {
        assert!(!is_allowed_origin_with_config(
            "http://192.168.1.100:3001",
            &HashSet::new()
        ));
        assert!(!is_allowed_origin_with_config(
            "http://10.0.0.5:3001",
            &HashSet::new()
        ));
        assert!(!is_allowed_origin_with_config(
            "http://mymachine.local:3001",
            &HashSet::new()
        ));
        assert!(!is_allowed_origin_with_config(
            "http://mymachine:3001",
            &HashSet::new()
        ));
    }

    #[test]
    fn allows_configured_lan_origin() {
        let configured = HashSet::from(["http://192.168.1.100:3000".to_string()]);
        assert!(is_allowed_origin_with_config(
            "http://192.168.1.100:3000",
            &configured
        ));
    }

    #[test]
    fn rejects_public_domain() {
        assert!(!is_allowed_origin_with_config(
            "https://example.com",
            &HashSet::new()
        ));
        assert!(!is_allowed_origin_with_config(
            "https://attacker.io",
            &HashSet::new()
        ));
    }

    #[test]
    fn rejects_non_http_scheme() {
        assert!(!is_allowed_origin_with_config(
            "ftp://localhost",
            &HashSet::new()
        ));
    }

    #[test]
    fn rejects_public_ip() {
        assert!(!is_allowed_origin_with_config(
            "http://8.8.8.8:3001",
            &HashSet::new()
        ));
    }
}
