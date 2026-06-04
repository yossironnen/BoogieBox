//! Defines Tauri desktop shell logic for Server.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, net::Ipv4Addr, process::Command, time::Duration};
use tokio::task::JoinSet;

/// Public Server Probe Result data shape used by BoogieBox.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProbeResult {
    /// Documents the Reachable public API surface.
    pub reachable: bool,
    /// Documents the Url public API surface.
    pub url: String,
    /// Documents the Version public API surface.
    pub version: Option<String>,
    /// Documents the App public API surface.
    pub app: Option<String>,
    /// Documents the Setup Required public API surface.
    pub setup_required: Option<bool>,
    /// Documents the Error public API surface.
    pub error: Option<String>,
}

/// Public Server Discovery Result data shape used by BoogieBox.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerDiscoveryResult {
    /// Documents the Servers public API surface.
    pub servers: Vec<ServerProbeResult>,
    /// Documents the Scanned public API surface.
    pub scanned: usize,
}

/// Probe the BoogieBox server at the given URL.
/// Hits `/api/system/status` and returns reachability + version string.
#[cfg(feature = "health-probe")]
pub async fn probe(url: &str) -> ServerProbeResult {
    probe_with_timeout(url, Duration::from_secs(5)).await
}

#[cfg(feature = "health-probe")]
async fn probe_with_timeout(url: &str, timeout: Duration) -> ServerProbeResult {
    let status_url = format!("{}/api/system/status", url.trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .unwrap_or_default();

    match client.get(&status_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            let value = resp
                .json::<serde_json::Value>()
                .await
                .unwrap_or(serde_json::Value::Null);
            let is_boogiebox = value.get("server").and_then(|v| v.as_str()) == Some("boogiebox")
                || value.get("app").and_then(|v| v.as_str()) == Some("BoogieBox");
            if is_boogiebox {
                ServerProbeResult {
                    reachable: true,
                    url: url.to_string(),
                    version: value
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    app: value.get("app").and_then(|v| v.as_str()).map(String::from),
                    setup_required: value.get("setupRequired").and_then(|v| v.as_bool()),
                    error: None,
                }
            } else {
                ServerProbeResult {
                    reachable: false,
                    url: url.to_string(),
                    version: None,
                    app: None,
                    setup_required: None,
                    error: Some("Not a BoogieBox server".to_string()),
                }
            }
        }
        Ok(resp) => ServerProbeResult {
            reachable: false,
            url: url.to_string(),
            version: None,
            app: None,
            setup_required: None,
            error: Some(format!("HTTP {}", resp.status())),
        },
        Err(e) => ServerProbeResult {
            reachable: false,
            url: url.to_string(),
            version: None,
            app: None,
            setup_required: None,
            error: Some(e.to_string()),
        },
    }
}

#[cfg(feature = "health-probe")]
pub async fn discover() -> ServerDiscoveryResult {
    let mut candidates = discovery_candidates();
    let scanned = candidates.len();
    let mut tasks = JoinSet::new();

    for url in candidates.drain(..) {
        tasks.spawn(async move { probe_with_timeout(&url, Duration::from_millis(450)).await });
    }

    let mut servers = Vec::new();
    while let Some(result) = tasks.join_next().await {
        if let Ok(probe) = result {
            if probe.reachable {
                servers.push(probe);
            }
        }
    }

    servers.sort_by(|a, b| a.url.cmp(&b.url));
    ServerDiscoveryResult { servers, scanned }
}

#[cfg(feature = "health-probe")]
fn discovery_candidates() -> Vec<String> {
    let mut urls = BTreeSet::new();
    urls.insert("http://localhost:3001".to_string());
    urls.insert("http://127.0.0.1:3001".to_string());

    for ip in local_ipv4_addresses() {
        let octets = ip.octets();
        if !is_private_lan(ip) {
            continue;
        }
        for host in 1..=254 {
            urls.insert(format!(
                "http://{}.{}.{}.{host}:3001",
                octets[0], octets[1], octets[2]
            ));
        }
    }

    urls.into_iter().collect()
}

#[cfg(feature = "health-probe")]
fn is_private_lan(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 10
        || (octets[0] == 172 && (16..=31).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 168)
}

#[cfg(feature = "health-probe")]
fn local_ipv4_addresses() -> Vec<Ipv4Addr> {
    let output = Command::new("ipconfig").output();
    let text = match output {
        Ok(output) => String::from_utf8_lossy(&output.stdout).into_owned(),
        Err(_) => return Vec::new(),
    };

    text.lines()
        .filter_map(|line| line.split(':').nth(1))
        .filter_map(|value| value.trim().split('(').next())
        .filter_map(|value| value.trim().parse::<Ipv4Addr>().ok())
        .collect()
}

#[cfg(not(feature = "health-probe"))]
pub async fn probe(url: &str) -> ServerProbeResult {
    ServerProbeResult {
        reachable: false,
        url: url.to_string(),
        version: None,
        app: None,
        setup_required: None,
        error: Some("health-probe feature not enabled".to_string()),
    }
}

#[cfg(not(feature = "health-probe"))]
pub async fn discover() -> ServerDiscoveryResult {
    ServerDiscoveryResult {
        servers: Vec::new(),
        scanned: 0,
    }
}
