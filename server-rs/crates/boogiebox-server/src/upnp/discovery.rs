//! SSDP control-point discovery of MediaRenderers: periodic/on-demand `M-SEARCH` plus a passive `NOTIFY`
//! listener. Emits [`DiscoveryEvent`]s; fetching and parsing device descriptions happens in the consumer.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::Url;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Notify};
use tokio_util::sync::CancellationToken;

use super::device::{is_lan_ip, normalize_udn};
use super::MEDIA_RENDERER_DEVICE;

const SSDP_ADDR: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const SSDP_PORT: u16 = 1900;
/// Interval between unsolicited searches.
pub const SEARCH_INTERVAL: Duration = Duration::from_secs(300);
/// Extra searches shortly after start: SSDP is lossy and a busy device can miss the first burst, which would
/// otherwise leave it (and any saved session on it) waiting for the next periodic search.
pub const EARLY_SEARCHES: [Duration; 2] = [Duration::from_secs(5), Duration::from_secs(20)];
/// Default lifetime when a device omits `CACHE-CONTROL: max-age`.
pub const DEFAULT_MAX_AGE_SECS: u64 = 1800;
const SEARCH_REPEATS: u32 = 3;
const SEARCH_SPACING: Duration = Duration::from_millis(300);

/// What discovery learned about a renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryEvent {
    /// A renderer answered a search or announced itself.
    Seen {
        /// Device description URL (validated: http, LAN IP, same host as the packet source).
        location: String,
        /// Normalized UDN from the `USN` header.
        udn: String,
        /// Advertisement lifetime in seconds.
        max_age: u64,
    },
    /// A renderer announced `ssdp:byebye`.
    Gone {
        /// Normalized UDN.
        udn: String,
    },
}

/// Kind of SSDP datagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SsdpKind {
    /// Unicast reply to our `M-SEARCH`.
    Response,
    /// `NOTIFY` with `NTS: ssdp:alive`.
    Alive,
    /// `NOTIFY` with `NTS: ssdp:byebye`.
    ByeBye,
}

/// Parsed SSDP datagram (only the headers Play On uses).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsdpMessage {
    /// Datagram kind.
    pub kind: SsdpKind,
    /// `LOCATION` header.
    pub location: Option<String>,
    /// `USN` header.
    pub usn: Option<String>,
    /// `ST` (responses) or `NT` (notifications).
    pub target: Option<String>,
    /// `CACHE-CONTROL: max-age` seconds.
    pub max_age: Option<u64>,
}

impl SsdpMessage {
    /// Normalized UDN extracted from `USN` (`uuid:XXXX::urn:...` → `xxxx`).
    pub fn udn(&self) -> Option<String> {
        let usn = self.usn.as_deref()?;
        let head = usn.split("::").next()?;
        let udn = normalize_udn(head);
        (!udn.is_empty()).then_some(udn)
    }

    /// True when the message concerns a MediaRenderer device.
    pub fn is_renderer(&self) -> bool {
        self.target
            .as_deref()
            .is_some_and(|t| t.trim().eq_ignore_ascii_case(MEDIA_RENDERER_DEVICE))
    }
}

/// Builds an `M-SEARCH` request for `st`.
pub fn build_msearch(st: &str) -> String {
    format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: {SSDP_ADDR}:{SSDP_PORT}\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: {st}\r\nUSER-AGENT: BoogieBox UPnP/1.0\r\n\r\n"
    )
}

/// Parses an SSDP response or NOTIFY; returns `None` for searches and anything unrecognized.
pub fn parse_ssdp(text: &str) -> Option<SsdpMessage> {
    let mut lines = text.split("\r\n").flat_map(|l| l.split('\n'));
    let start = lines.next()?.trim().to_ascii_uppercase();
    let is_response = start.starts_with("HTTP/1.1 200") || start.starts_with("HTTP/1.0 200");
    let is_notify = start.starts_with("NOTIFY ");
    if !is_response && !is_notify {
        return None;
    }
    let mut location = None;
    let mut usn = None;
    let mut target = None;
    let mut nts = None;
    let mut max_age = None;
    for line in lines {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let v = v.trim().to_string();
        match k.trim().to_ascii_uppercase().as_str() {
            "LOCATION" => location = Some(v),
            "USN" => usn = Some(v),
            "ST" if is_response => target = Some(v),
            "NT" if is_notify => target = Some(v),
            "NTS" => nts = Some(v.to_ascii_lowercase()),
            "CACHE-CONTROL" => {
                max_age = v.split(',').find_map(|part| {
                    let (k, n) = part.split_once('=')?;
                    k.trim()
                        .eq_ignore_ascii_case("max-age")
                        .then(|| n.trim().parse::<u64>().ok())?
                });
            }
            _ => {}
        }
    }
    let kind = if is_response {
        SsdpKind::Response
    } else {
        match nts.as_deref() {
            Some("ssdp:alive") => SsdpKind::Alive,
            Some("ssdp:byebye") => SsdpKind::ByeBye,
            _ => return None,
        }
    };
    Some(SsdpMessage {
        kind,
        location,
        usn,
        target,
        max_age,
    })
}

/// Turns a datagram from `from` into a discovery event, enforcing that `LOCATION` points at the sender.
pub fn event_from_message(msg: &SsdpMessage, from: IpAddr) -> Option<DiscoveryEvent> {
    if !msg.is_renderer() || !is_lan_ip(from) {
        return None;
    }
    let udn = msg.udn()?;
    match msg.kind {
        SsdpKind::ByeBye => Some(DiscoveryEvent::Gone { udn }),
        SsdpKind::Response | SsdpKind::Alive => {
            let location = msg.location.as_deref()?;
            let url = Url::parse(location).ok()?;
            if url.scheme() != "http" {
                return None;
            }
            let host: IpAddr = url
                .host_str()?
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse()
                .ok()?;
            // Anti-spoofing: a device may only advertise a description hosted on its own address.
            (host == from).then(|| DiscoveryEvent::Seen {
                location: url.to_string(),
                udn,
                max_age: msg
                    .max_age
                    .unwrap_or(DEFAULT_MAX_AGE_SECS)
                    .clamp(60, 86_400),
            })
        }
    }
}

fn route_ipv4(dest: Ipv4Addr) -> Option<Ipv4Addr> {
    // A wildcard bind from a fresh test binary triggers a Windows Firewall prompt on every build.
    if cfg!(test) {
        let _ = dest;
        return None;
    }
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // `connect` on UDP only selects a route; no packet is sent.
    socket.connect((dest, 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(v4) if !v4.is_unspecified() => Some(v4),
        _ => None,
    }
}

/// IPv4 interfaces to search on: the admin override, or else the default-gateway interface plus the
/// interface the OS prefers for multicast. They differ on hosts with Hyper-V/WSL/VMware/VPN adapters, where
/// the multicast route often points at a virtual switch. Only private addresses are kept.
pub fn candidate_ipv4_interfaces(override_ip: Option<Ipv4Addr>) -> Vec<Ipv4Addr> {
    if let Some(ip) = override_ip {
        return vec![ip];
    }
    // TEST-NET-1 (RFC 5737) resolves through the default route without contacting anything.
    let mut out: Vec<Ipv4Addr> = Vec::new();
    for ip in [
        route_ipv4(Ipv4Addr::new(192, 0, 2, 1)),
        route_ipv4(SSDP_ADDR),
    ]
    .into_iter()
    .flatten()
    {
        if ip.is_private() && !out.contains(&ip) {
            out.push(ip);
        }
    }
    out
}

/// Local address the OS would use to reach `peer` — the address to put in media URLs sent to that renderer.
pub fn local_ip_for(peer: IpAddr) -> Option<IpAddr> {
    // Loopback peers bind loopback: no wildcard socket, so no Windows Firewall prompt.
    let bind = match peer {
        IpAddr::V4(v4) if v4.is_loopback() => "127.0.0.1:0",
        IpAddr::V6(v6) if v6.is_loopback() => "[::1]:0",
        IpAddr::V4(_) => "0.0.0.0:0",
        IpAddr::V6(_) => "[::]:0",
    };
    let socket = std::net::UdpSocket::bind(bind).ok()?;
    socket.connect(SocketAddr::new(peer, 9)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}

fn search_socket(iface: Ipv4Addr) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_nonblocking(true)?;
    socket.set_multicast_ttl_v4(2)?;
    socket.set_multicast_if_v4(&iface)?;
    socket.bind(&SocketAddr::new(IpAddr::V4(iface), 0).into())?;
    UdpSocket::from_std(socket.into())
}

fn notify_socket(ifaces: &[Ipv4Addr]) -> std::io::Result<UdpSocket> {
    // Wildcard :1900 bind: skipped in tests so fresh test binaries don't trigger a Windows Firewall prompt.
    if cfg!(test) {
        return Err(std::io::Error::other("NOTIFY listener disabled in tests"));
    }
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), SSDP_PORT).into())?;
    let mut joined = false;
    for iface in ifaces {
        joined |= socket.join_multicast_v4(&SSDP_ADDR, iface).is_ok();
    }
    if !joined {
        socket.join_multicast_v4(&SSDP_ADDR, &Ipv4Addr::UNSPECIFIED)?;
    }
    UdpSocket::from_std(socket.into())
}

/// What discovery is doing, for the admin diagnostics view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiscoveryStatus {
    /// Interfaces searched on.
    pub interfaces: Vec<Ipv4Addr>,
    /// Last `M-SEARCH` burst (ms since epoch).
    pub last_search_ms: Option<u64>,
    /// Last interface/socket problem.
    pub last_error: Option<String>,
}

/// Handle to a running discovery task.
#[derive(Clone, Debug)]
pub struct DiscoveryHandle {
    refresh: Arc<Notify>,
    status: Arc<Mutex<DiscoveryStatus>>,
}

impl DiscoveryHandle {
    /// Current diagnostics.
    pub fn status(&self) -> DiscoveryStatus {
        self.status
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Requests an immediate `M-SEARCH` burst.
    pub fn refresh(&self) {
        self.refresh.notify_one();
    }
}

async fn send_search_burst(sock: &UdpSocket) {
    let msg = build_msearch(MEDIA_RENDERER_DEVICE);
    let dst = SocketAddr::new(IpAddr::V4(SSDP_ADDR), SSDP_PORT);
    for i in 0..SEARCH_REPEATS {
        if i > 0 {
            tokio::time::sleep(SEARCH_SPACING).await;
        }
        if let Err(e) = sock.send_to(msg.as_bytes(), dst).await {
            tracing::debug!(target: "remote", "M-SEARCH send failed: {e}");
        }
    }
}

/// Reads datagrams from `sock` and forwards renderer events until cancelled or the consumer goes away.
async fn receive_loop(
    sock: Arc<UdpSocket>,
    tx: mpsc::Sender<DiscoveryEvent>,
    cancel: CancellationToken,
) {
    let mut buf = vec![0u8; 4096];
    loop {
        let (n, from) = tokio::select! {
            _ = cancel.cancelled() => return,
            r = sock.recv_from(&mut buf) => match r {
                Ok(v) => v,
                // Windows surfaces ICMP port-unreachable as a recv error on UDP sockets; keep listening.
                Err(_) => continue,
            },
        };
        let text = String::from_utf8_lossy(&buf[..n]);
        if let Some(event) = parse_ssdp(&text).and_then(|m| event_from_message(&m, from.ip())) {
            if tx.send(event).await.is_err() {
                return;
            }
        }
    }
}

/// Starts discovery on the candidate LAN interfaces (or `override_ip`) and forwards events to `tx` until
/// `cancel` fires.
pub fn spawn_discovery(
    override_ip: Option<Ipv4Addr>,
    tx: mpsc::Sender<DiscoveryEvent>,
    cancel: CancellationToken,
) -> DiscoveryHandle {
    let refresh = Arc::new(Notify::new());
    let status = Arc::new(Mutex::new(DiscoveryStatus::default()));
    let handle = DiscoveryHandle {
        refresh: refresh.clone(),
        status: status.clone(),
    };
    let set_status = move |update: &dyn Fn(&mut DiscoveryStatus)| {
        update(&mut status.lock().unwrap_or_else(|p| p.into_inner()));
    };
    tokio::spawn(async move {
        let ifaces = candidate_ipv4_interfaces(override_ip);
        set_status(&|s| s.interfaces = ifaces.clone());
        let mut searchers = Vec::new();
        for iface in &ifaces {
            match search_socket(*iface) {
                Ok(s) => searchers.push(Arc::new(s)),
                Err(e) => {
                    tracing::warn!(target: "remote", %iface, "Play On: cannot search on interface: {e}");
                    let message = format!("Cannot search on {iface}: {e}");
                    set_status(&|s| s.last_error = Some(message.clone()));
                }
            }
        }
        if searchers.is_empty() {
            tracing::warn!(target: "remote", "Play On discovery disabled: no usable LAN interface");
            set_status(&|s| s.last_error = Some("No usable LAN interface".into()));
            return;
        }
        for s in &searchers {
            tokio::spawn(receive_loop(s.clone(), tx.clone(), cancel.clone()));
        }
        match notify_socket(&ifaces) {
            Ok(s) => {
                tokio::spawn(receive_loop(Arc::new(s), tx.clone(), cancel.clone()));
            }
            Err(e) => tracing::info!(
                target: "remote",
                "Play On: NOTIFY listener unavailable ({e}); relying on periodic search"
            ),
        }
        tracing::info!(target: "remote", interfaces = ?ifaces, "Play On discovery started");
        let mut ticker = tokio::time::interval(SEARCH_INTERVAL);
        let started = tokio::time::Instant::now();
        let mut early = EARLY_SEARCHES.iter();
        let mut next_early = early.next();
        loop {
            let early_at = next_early.map(|d| started + *d);
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = ticker.tick() => {}
                _ = refresh.notified() => {}
                _ = async { tokio::time::sleep_until(early_at.unwrap_or(started)).await }, if early_at.is_some() => {
                    next_early = early.next();
                }
            }
            for s in &searchers {
                send_search_burst(s).await;
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            set_status(&|s| s.last_search_ms = Some(now));
        }
        tracing::info!(target: "remote", "Play On discovery stopped");
    });
    handle
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIIM_RESPONSE: &str = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age = 1800\r\nEXT:\r\nLOCATION: http://10.0.0.134:49152/description.xml\r\nSERVER: Linux\r\nST: urn:schemas-upnp-org:device:MediaRenderer:1\r\nUSN: uuid:FF98-AA::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn parses_search_response() {
        let m = parse_ssdp(WIIM_RESPONSE).unwrap();
        assert_eq!(m.kind, SsdpKind::Response);
        assert_eq!(m.max_age, Some(1800));
        assert_eq!(m.udn().as_deref(), Some("ff98-aa"));
        assert!(m.is_renderer());
        assert_eq!(
            event_from_message(&m, ip("10.0.0.134")),
            Some(DiscoveryEvent::Seen {
                location: "http://10.0.0.134:49152/description.xml".into(),
                udn: "ff98-aa".into(),
                max_age: 1800
            })
        );
    }

    #[test]
    fn rejects_spoofed_foreign_or_irrelevant_messages() {
        let m = parse_ssdp(WIIM_RESPONSE).unwrap();
        assert_eq!(event_from_message(&m, ip("10.0.0.99")), None); // location host != sender
        assert_eq!(event_from_message(&m, ip("8.8.8.8")), None); // not LAN
        let server = WIIM_RESPONSE.replace("MediaRenderer:1\r\nUSN", "MediaServer:1\r\nUSN");
        assert_eq!(
            event_from_message(&parse_ssdp(&server).unwrap(), ip("10.0.0.134")),
            None
        );
        let https = WIIM_RESPONSE.replace("http://", "https://");
        assert_eq!(
            event_from_message(&parse_ssdp(&https).unwrap(), ip("10.0.0.134")),
            None
        );
        let hostname = WIIM_RESPONSE.replace("10.0.0.134", "wiim.local");
        assert_eq!(
            event_from_message(&parse_ssdp(&hostname).unwrap(), ip("10.0.0.134")),
            None
        );
        let no_usn = WIIM_RESPONSE.replace(
            "USN: uuid:FF98-AA::urn:schemas-upnp-org:device:MediaRenderer:1\r\n",
            "",
        );
        assert_eq!(
            event_from_message(&parse_ssdp(&no_usn).unwrap(), ip("10.0.0.134")),
            None
        );
        assert!(parse_ssdp(&build_msearch(MEDIA_RENDERER_DEVICE)).is_none());
        assert!(parse_ssdp("").is_none());
        assert!(parse_ssdp("HTTP/1.1 404 Not Found\r\n\r\n").is_none());
    }

    #[test]
    fn parses_notify_alive_and_byebye() {
        let alive = "NOTIFY * HTTP/1.1\nHOST: 239.255.255.250:1900\nNT: urn:schemas-upnp-org:device:MediaRenderer:1\nNTS: ssdp:alive\nLOCATION: http://192.168.1.20:8080/d.xml\nUSN: uuid:abc::urn:schemas-upnp-org:device:MediaRenderer:1\nCACHE-CONTROL: no-cache, max-age=5\n\n";
        let m = parse_ssdp(alive).unwrap();
        assert_eq!(m.kind, SsdpKind::Alive);
        match event_from_message(&m, ip("192.168.1.20")) {
            Some(DiscoveryEvent::Seen { max_age, .. }) => assert_eq!(max_age, 60), // clamped up
            other => panic!("unexpected {other:?}"),
        }
        let bye = alive.replace("ssdp:alive", "ssdp:byebye");
        assert_eq!(
            event_from_message(&parse_ssdp(&bye).unwrap(), ip("192.168.1.20")),
            Some(DiscoveryEvent::Gone { udn: "abc".into() })
        );
        let unknown = alive.replace("ssdp:alive", "ssdp:update");
        assert!(parse_ssdp(&unknown).is_none());
        let root = alive.replace(
            "NT: urn:schemas-upnp-org:device:MediaRenderer:1",
            "NT: upnp:rootdevice",
        );
        assert_eq!(
            event_from_message(&parse_ssdp(&root).unwrap(), ip("192.168.1.20")),
            None
        );
    }

    #[test]
    fn default_max_age_applies() {
        let m =
            parse_ssdp(&WIIM_RESPONSE.replace("CACHE-CONTROL: max-age = 1800\r\n", "")).unwrap();
        match event_from_message(&m, ip("10.0.0.134")) {
            Some(DiscoveryEvent::Seen { max_age, .. }) => assert_eq!(max_age, DEFAULT_MAX_AGE_SECS),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn msearch_has_required_headers() {
        let m = build_msearch(MEDIA_RENDERER_DEVICE);
        assert!(m.starts_with("M-SEARCH * HTTP/1.1\r\n"));
        assert!(m.contains("MAN: \"ssdp:discover\""));
        assert!(m.contains("ST: urn:schemas-upnp-org:device:MediaRenderer:1"));
        assert!(m.ends_with("\r\n\r\n"));
    }

    #[test]
    fn local_ip_helpers() {
        assert_eq!(local_ip_for(ip("127.0.0.1")), Some(ip("127.0.0.1")));
        assert_eq!(
            candidate_ipv4_interfaces(Some(Ipv4Addr::new(10, 1, 1, 1))),
            vec![Ipv4Addr::new(10, 1, 1, 1)]
        );
        // Machine-dependent (may be empty in sandboxes) but always private and de-duplicated.
        let auto = candidate_ipv4_interfaces(None);
        assert!(auto.iter().all(|i| i.is_private()));
        let mut dedup = auto.clone();
        dedup.dedup();
        assert_eq!(auto, dedup);
    }

    #[tokio::test]
    async fn discovery_task_starts_refreshes_and_stops() {
        let (tx, _rx) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let handle = spawn_discovery(Some(Ipv4Addr::LOCALHOST), tx, cancel.clone());
        handle.refresh();
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(search_socket(Ipv4Addr::LOCALHOST).is_ok());
        // Joining on no interface falls back to the default; must not panic either way.
        let _ = notify_socket(&[]);
    }
}
