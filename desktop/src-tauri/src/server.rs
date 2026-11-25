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
    let adapters = local_ipv4_addresses();
    let mut candidates = discovery_candidates(&adapters);
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

    let own: Vec<Ipv4Addr> = adapters.iter().map(|a| a.addr).collect();
    ServerDiscoveryResult {
        servers: fold_this_pc(servers, &own),
        scanned,
    }
}

/// One IPv4 address of this PC and whether its adapter is a virtual one (WSL, Hyper-V, VMware, VirtualBox, Docker,
/// loopback), whose subnet holds no other machines worth scanning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LocalAddress {
    addr: Ipv4Addr,
    virtual_adapter: bool,
}

/// Port 3001 on localhost, then on every host of each real adapter's private /24.
#[cfg_attr(not(feature = "health-probe"), allow(dead_code))]
fn discovery_candidates(adapters: &[LocalAddress]) -> Vec<String> {
    let mut urls = BTreeSet::new();
    urls.insert("http://localhost:3001".to_string());

    for adapter in adapters {
        if adapter.virtual_adapter || !is_private_lan(adapter.addr) {
            continue;
        }
        let octets = adapter.addr.octets();
        for host in 1..=254 {
            urls.insert(format!(
                "http://{}.{}.{}.{host}:3001",
                octets[0], octets[1], octets[2]
            ));
        }
    }

    urls.into_iter().collect()
}

fn url_host(url: &str) -> &str {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let authority = rest.split('/').next().unwrap_or(rest);
    authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host)
}

/// The server on this PC answers on localhost and on each of the PC's own addresses: list it once, as localhost
/// (or, if localhost didn't answer, as the first own address that did). Other machines are listed by address.
#[cfg_attr(not(feature = "health-probe"), allow(dead_code))]
fn fold_this_pc(servers: Vec<ServerProbeResult>, own: &[Ipv4Addr]) -> Vec<ServerProbeResult> {
    let is_own = |url: &str| {
        let host = url_host(url);
        host == "localhost"
            || host
                .parse::<Ipv4Addr>()
                .is_ok_and(|ip| ip.is_loopback() || own.contains(&ip))
    };
    let (mut this_pc, mut others): (Vec<_>, Vec<_>) =
        servers.into_iter().partition(|s| is_own(&s.url));
    this_pc.sort_by_key(|s| (url_host(&s.url) != "localhost", s.url.clone()));
    others.sort_by(|a, b| a.url.cmp(&b.url));
    this_pc.truncate(1);
    this_pc.extend(others);
    this_pc
}

fn is_private_lan(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 10
        || (octets[0] == 172 && (16..=31).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 168)
}

#[cfg(feature = "health-probe")]
fn local_ipv4_addresses() -> Vec<LocalAddress> {
    match Command::new("ipconfig").output() {
        Ok(output) => parse_ipconfig(&String::from_utf8_lossy(&output.stdout)),
        Err(_) => Vec::new(),
    }
}

/// Adapter names (not localized by Windows) whose subnets hold only virtual machines or this PC itself.
const VIRTUAL_ADAPTERS: [&str; 7] = [
    "vethernet",
    "hyper-v",
    "wsl",
    "vmware",
    "virtualbox",
    "docker",
    "loopback",
];

/// The PC's IPv4 addresses from `ipconfig`: values on lines labelled "IPv4" (the label is localized, but every
/// Windows language keeps "IPv4" in it; gateways and masks don't have it), each with its adapter's kind. An
/// adapter section starts at an unindented line ending in ':'.
#[cfg_attr(not(feature = "health-probe"), allow(dead_code))]
fn parse_ipconfig(text: &str) -> Vec<LocalAddress> {
    let mut virtual_adapter = false;
    let mut found = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if !line.starts_with(char::is_whitespace) && trimmed.ends_with(':') {
            let header = trimmed.to_lowercase();
            virtual_adapter = VIRTUAL_ADAPTERS.iter().any(|name| header.contains(name));
            continue;
        }
        let Some((label, value)) = line.split_once(':') else {
            continue;
        };
        if !label.contains("IPv4") {
            continue;
        }
        let value = value.trim().split('(').next().unwrap_or("").trim();
        if let Ok(addr) = value.parse::<Ipv4Addr>() {
            found.push(LocalAddress {
                addr,
                virtual_adapter,
            });
        }
    }
    found
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

#[cfg(test)]
mod tests {
    use super::*;

    const IPCONFIG: &str = "
Windows IP Configuration


Ethernet adapter Ethernet:

   Connection-specific DNS Suffix  . :
   IPv6 Address. . . . . . . . . . . : 2607:fea8:3ca5:1e00::6993
   IPv4 Address. . . . . . . . . . . : 10.0.0.227
   Subnet Mask . . . . . . . . . . . : 255.255.255.0
   Default Gateway . . . . . . . . . : fe80::1236:aaff:fec7:e177%21
                                       10.0.0.1

Wireless LAN adapter Wi-Fi:

   Media State . . . . . . . . . . . : Media disconnected

Ethernet adapter VMware Network Adapter VMnet1:

   IPv4 Address. . . . . . . . . . . : 192.168.185.1
   Subnet Mask . . . . . . . . . . . : 255.255.255.0

Ethernet adapter vEthernet (WSL (Hyper-V firewall)):

   IPv4-Adresse  . . . . . . . . . . : 172.28.192.1(Bevorzugt)
   Subnetzmaske  . . . . . . . . . . : 255.255.240.0
";

    fn found(url: &str) -> ServerProbeResult {
        ServerProbeResult {
            reachable: true,
            url: url.to_string(),
            version: Some("0.9.128".into()),
            app: Some("BoogieBox".into()),
            setup_required: Some(false),
            error: None,
        }
    }

    fn addr(text: &str) -> Ipv4Addr {
        text.parse().unwrap()
    }

    #[test]
    fn ipconfig_gives_each_address_and_whether_its_adapter_is_virtual() {
        assert_eq!(
            parse_ipconfig(IPCONFIG),
            vec![
                LocalAddress {
                    addr: addr("10.0.0.227"),
                    virtual_adapter: false
                },
                LocalAddress {
                    addr: addr("192.168.185.1"),
                    virtual_adapter: true
                },
                LocalAddress {
                    addr: addr("172.28.192.1"),
                    virtual_adapter: true
                },
            ]
        );
    }

    #[test]
    fn only_real_adapters_subnets_are_scanned() {
        let urls = discovery_candidates(&parse_ipconfig(IPCONFIG));
        assert_eq!(urls.len(), 1 + 254);
        assert!(urls.contains(&"http://localhost:3001".to_string()));
        assert!(urls.contains(&"http://10.0.0.143:3001".to_string()));
        assert!(!urls.iter().any(|u| u.starts_with("http://192.168.185.")));
        assert!(!urls.iter().any(|u| u.starts_with("http://172.28.")));
    }

    #[test]
    fn this_pc_is_listed_once_as_localhost_and_other_machines_by_address() {
        let own: Vec<Ipv4Addr> = parse_ipconfig(IPCONFIG).iter().map(|a| a.addr).collect();
        let servers = vec![
            found("http://10.0.0.227:3001"),
            found("http://10.0.0.143:3001"),
            found("http://localhost:3001"),
            found("http://127.0.0.1:3001"),
            found("http://192.168.185.1:3001"),
        ];
        let urls: Vec<String> = fold_this_pc(servers, &own)
            .into_iter()
            .map(|s| s.url)
            .collect();
        assert_eq!(urls, ["http://localhost:3001", "http://10.0.0.143:3001"]);
    }

    #[test]
    fn without_a_localhost_answer_one_own_address_stays() {
        let own = [addr("10.0.0.227")];
        let servers = vec![
            found("http://10.0.0.227:3001"),
            found("http://10.0.0.5:3001"),
        ];
        let urls: Vec<String> = fold_this_pc(servers, &own)
            .into_iter()
            .map(|s| s.url)
            .collect();
        assert_eq!(urls, ["http://10.0.0.227:3001", "http://10.0.0.5:3001"]);
    }
}
