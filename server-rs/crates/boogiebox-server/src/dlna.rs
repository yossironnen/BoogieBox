//! Defines Rust server support logic for Dlna.

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Router,
};
use rusqlite::Connection;
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::DbPool;

// -- Constants -----------------------------------------------------------------

const SSDP_MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const SSDP_PORT: u16 = 1900;
const SSDP_INTERVAL_SECS: u64 = 10;
const DEFAULT_DLNA_PORT: u16 = 8200;
const BROWSE_DEFAULT_LIMIT: i64 = 1000;
const NT_ROOT_DEVICE: &str = "upnp:rootdevice";
const NT_MEDIA_SERVER: &str = "urn:schemas-upnp-org:device:MediaServer:1";
const NT_CONTENT_DIR: &str = "urn:schemas-upnp-org:service:ContentDirectory:1";
const NT_CONN_MGR: &str = "urn:schemas-upnp-org:service:ConnectionManager:1";

// -- Settings ------------------------------------------------------------------

/// Documents the DLNA Settings public API surface.
pub struct DlnaSettings {
    /// Documents the Enabled public API surface.
    pub enabled: bool,
    /// Documents the Port public API surface.
    pub port: u16,
    /// Documents the Friendly Name public API surface.
    pub friendly_name: String,
    /// Documents the Udn public API surface.
    pub udn: String,
}

/// Documents the Read DLNA Settings public API surface.
pub fn read_dlna_settings(conn: &Connection) -> DlnaSettings {
    let enabled = conn
        .query_row(
            "SELECT value FROM settings WHERE key='dlnaEnabled'",
            [],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_default()
        == "true";

    let port = conn
        .query_row("SELECT value FROM settings WHERE key='dlnaPort'", [], |r| {
            r.get::<_, String>(0)
        })
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(DEFAULT_DLNA_PORT);

    let friendly_name = conn
        .query_row(
            "SELECT value FROM settings WHERE key='dlnaFriendlyName'",
            [],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "BoogieBox".to_string());

    let udn = match conn.query_row("SELECT value FROM settings WHERE key='dlnaUdn'", [], |r| {
        r.get::<_, String>(0)
    }) {
        Ok(v) if !v.is_empty() => v,
        _ => {
            let new_udn = Uuid::now_v7().to_string();
            let _ = boogiebox_db::upsert_setting(conn, "dlnaUdn", &new_udn);
            new_udn
        }
    };

    DlnaSettings {
        enabled,
        port,
        friendly_name,
        udn,
    }
}

// -- Manager -------------------------------------------------------------------

/// Public DLNA Manager Inner data shape used by BoogieBox.
#[derive(Debug)]
pub struct DlnaManagerInner {
    /// Documents the Running public API surface.
    pub running: bool,
    /// Documents the Port public API surface.
    pub port: Option<u16>,
    /// Documents the Friendly Name public API surface.
    pub friendly_name: Option<String>,
    handles: Vec<tokio::task::JoinHandle<()>>,
}

/// Documents the DLNA Manager public API surface.
pub type DlnaManager = Arc<Mutex<DlnaManagerInner>>;

/// Documents the New DLNA Manager public API surface.
pub fn new_dlna_manager() -> DlnaManager {
    Arc::new(Mutex::new(DlnaManagerInner {
        running: false,
        port: None,
        friendly_name: None,
        handles: Vec::new(),
    }))
}

// -- Public API ----------------------------------------------------------------

/// Documents the Start DLNA public API surface.
pub fn start_dlna(manager: DlnaManager, db: DbPool, settings: DlnaSettings) {
    {
        let mut mgr = manager.lock().expect("dlna lock");
        for h in mgr.handles.drain(..) {
            h.abort();
        }
        mgr.running = false;
        mgr.port = None;
        mgr.friendly_name = None;
    }

    if !settings.enabled {
        tracing::info!("DLNA disabled; not starting");
        return;
    }

    let local_ip = match get_local_ip() {
        Some(ip) => ip,
        None => {
            tracing::warn!("DLNA: could not determine local IP; using 0.0.0.0");
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        }
    };

    let udn = settings.udn.clone();
    let friendly_name = settings.friendly_name.clone();
    let port = settings.port;

    let h1 = tokio::spawn(run_ssdp(udn.clone(), friendly_name.clone(), port, local_ip));
    let h2 = tokio::spawn(run_dlna_http_server(
        db,
        udn.clone(),
        friendly_name.clone(),
        port,
        local_ip,
    ));

    let mut mgr = manager.lock().expect("dlna lock");
    mgr.handles.push(h1);
    mgr.handles.push(h2);
    mgr.running = true;
    mgr.port = Some(port);
    mgr.friendly_name = Some(friendly_name);

    tracing::info!(
        "DLNA started: friendly_name={} port={} ip={}",
        &settings.friendly_name,
        port,
        local_ip
    );
}

/// Documents the Restart DLNA public API surface.
pub async fn restart_dlna(manager: DlnaManager, db: DbPool) {
    let settings = tokio::task::spawn_blocking({
        let db = db.clone();
        move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            read_dlna_settings(&conn)
        }
    })
    .await
    .expect("dlna settings read");
    start_dlna(manager, db, settings);
}

/// Documents the Start DLNA If Enabled public API surface.
pub async fn start_dlna_if_enabled(manager: DlnaManager, db: DbPool) {
    let settings = tokio::task::spawn_blocking({
        let db = db.clone();
        move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            read_dlna_settings(&conn)
        }
    })
    .await
    .expect("dlna settings read");
    if settings.enabled {
        start_dlna(manager, db, settings);
    }
}

fn get_local_ip() -> Option<IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket
        .connect(format!("{}:{}", SSDP_MULTICAST_ADDR, SSDP_PORT))
        .ok()?;
    Some(socket.local_addr().ok()?.ip())
}

// -- SSDP ----------------------------------------------------------------------

fn create_ssdp_socket() -> std::io::Result<tokio::net::UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), SSDP_PORT).into())?;
    socket.join_multicast_v4(&SSDP_MULTICAST_ADDR, &Ipv4Addr::UNSPECIFIED)?;
    let std_socket: std::net::UdpSocket = socket.into();
    tokio::net::UdpSocket::from_std(std_socket)
}

async fn send_notify_all(
    sock: &tokio::net::UdpSocket,
    udn: &str,
    friendly_name: &str,
    port: u16,
    local_ip: IpAddr,
) {
    let dst = SocketAddr::new(IpAddr::V4(SSDP_MULTICAST_ADDR), SSDP_PORT);
    let base_url = format!("http://{}:{}", local_ip, port);
    let types = [
        (NT_ROOT_DEVICE, format!("uuid:{}::{}", udn, NT_ROOT_DEVICE)),
        (&format!("uuid:{}", udn), format!("uuid:{}", udn)),
        (
            NT_MEDIA_SERVER,
            format!("uuid:{}::{}", udn, NT_MEDIA_SERVER),
        ),
        (NT_CONTENT_DIR, format!("uuid:{}::{}", udn, NT_CONTENT_DIR)),
        (NT_CONN_MGR, format!("uuid:{}::{}", udn, NT_CONN_MGR)),
    ];
    for (nt, usn) in &types {
        let msg = format!(
            "NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nCACHE-CONTROL: max-age=1800\r\nLOCATION: {base_url}/dlna/device.xml\r\nNT: {nt}\r\nNTS: ssdp:alive\r\nSERVER: Windows/10 UPnP/1.0 BoogieBox/1.0\r\nUSN: {usn}\r\nfriendlyName.dlna.org: {friendly_name}\r\n\r\n"
        );
        let _ = sock.send_to(msg.as_bytes(), dst).await;
    }
}

async fn respond_msearch(
    sock: &tokio::net::UdpSocket,
    src: SocketAddr,
    buf: &[u8],
    udn: &str,
    port: u16,
    local_ip: IpAddr,
) {
    let text = match std::str::from_utf8(buf) {
        Ok(t) => t,
        Err(_) => return,
    };
    let st_line = text
        .lines()
        .find(|l| l.to_ascii_uppercase().starts_with("ST:"))
        .map(|l| l[3..].trim())
        .unwrap_or("");

    let base_url = format!("http://{}:{}", local_ip, port);
    let respond = |nt: &str, usn: &str| {
        format!(
            "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nDATE: Thu, 01 Jan 1970 00:00:00 GMT\r\nEXT:\r\nLOCATION: {base_url}/dlna/device.xml\r\nSERVER: Windows/10 UPnP/1.0 BoogieBox/1.0\r\nST: {nt}\r\nUSN: {usn}\r\n\r\n"
        )
    };

    let matches: Vec<(String, String)> = if st_line == "ssdp:all" {
        vec![
            (
                NT_ROOT_DEVICE.to_string(),
                format!("uuid:{}::{}", udn, NT_ROOT_DEVICE),
            ),
            (format!("uuid:{}", udn), format!("uuid:{}", udn)),
            (
                NT_MEDIA_SERVER.to_string(),
                format!("uuid:{}::{}", udn, NT_MEDIA_SERVER),
            ),
            (
                NT_CONTENT_DIR.to_string(),
                format!("uuid:{}::{}", udn, NT_CONTENT_DIR),
            ),
            (
                NT_CONN_MGR.to_string(),
                format!("uuid:{}::{}", udn, NT_CONN_MGR),
            ),
        ]
    } else {
        let usn = if st_line == format!("uuid:{}", udn) {
            format!("uuid:{}", udn)
        } else {
            format!("uuid:{}::{}", udn, st_line)
        };
        if st_line == NT_ROOT_DEVICE
            || st_line == NT_MEDIA_SERVER
            || st_line == NT_CONTENT_DIR
            || st_line == NT_CONN_MGR
            || st_line == format!("uuid:{}", udn)
        {
            vec![(st_line.to_string(), usn)]
        } else {
            return;
        }
    };

    for (nt, usn) in &matches {
        let msg = respond(nt, usn);
        let _ = sock.send_to(msg.as_bytes(), src).await;
    }
}

async fn run_ssdp(udn: String, friendly_name: String, dlna_port: u16, local_ip: IpAddr) {
    let sock = match create_ssdp_socket() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                "DLNA SSDP: failed to bind port 1900: {e} - SSDP disabled, HTTP still active"
            );
            return;
        }
    };

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(SSDP_INTERVAL_SECS));
    let mut buf = vec![0u8; 2048];

    // Send initial alive immediately
    send_notify_all(&sock, &udn, &friendly_name, dlna_port, local_ip).await;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                send_notify_all(&sock, &udn, &friendly_name, dlna_port, local_ip).await;
            }
            result = sock.recv_from(&mut buf) => {
                match result {
                    Ok((n, src)) => {
                        let data = &buf[..n];
                        if data.windows(8).any(|w| w.eq_ignore_ascii_case(b"M-SEARCH")) {
                            respond_msearch(&sock, src, data, &udn, dlna_port, local_ip).await;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("DLNA SSDP recv error: {e}");
                    }
                }
            }
        }
    }
}

// -- DLNA HTTP server ----------------------------------------------------------

#[derive(Clone)]
struct DlnaHttpState {
    db: DbPool,
    friendly_name: String,
    udn: String,
    local_ip: IpAddr,
    port: u16,
}

async fn run_dlna_http_server(
    db: DbPool,
    udn: String,
    friendly_name: String,
    port: u16,
    local_ip: IpAddr,
) {
    let state = DlnaHttpState {
        db,
        friendly_name,
        udn,
        local_ip,
        port,
    };

    let app = Router::new()
        .route("/dlna/device.xml", get(device_xml_handler))
        .route(
            "/dlna/ContentDirectory.xml",
            get(content_directory_scpd_handler),
        )
        .route(
            "/dlna/ConnectionManager.xml",
            get(connection_manager_scpd_handler),
        )
        .route(
            "/dlna/control/ContentDirectory",
            post(content_directory_control_handler),
        )
        .route(
            "/dlna/control/ConnectionManager",
            post(connection_manager_control_handler),
        )
        .route("/dlna/media/track/{id}", get(audio_stream_handler))
        .route("/dlna/event/{service}", any(event_stub_handler))
        .with_state(state);

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("DLNA HTTP: failed to bind {}:{} - {e}", addr.ip(), port);
            return;
        }
    };
    tracing::info!("DLNA HTTP server listening on http://{}:{}", local_ip, port);
    if let Err(e) = axum::serve(listener, app).await {
        tracing::error!("DLNA HTTP server error: {e}");
    }
}

// -- Handlers ------------------------------------------------------------------

async fn device_xml_handler(State(state): State<DlnaHttpState>) -> impl IntoResponse {
    let xml = build_device_xml(&state.friendly_name, &state.udn, state.local_ip, state.port);
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
        xml,
    )
}

async fn content_directory_scpd_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
        CONTENT_DIRECTORY_SCPD,
    )
}

async fn connection_manager_scpd_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
        CONNECTION_MANAGER_SCPD,
    )
}

async fn content_directory_control_handler(
    State(state): State<DlnaHttpState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let soap_action = headers
        .get("SOAPAction")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| extract_soap_action(s, ""))
        .unwrap_or_default();

    let body_str = String::from_utf8_lossy(&body).into_owned();

    match soap_action.as_str() {
        "Browse" => {
            let obj_id = extract_xml_element(&body_str, "ObjectID")
                .unwrap_or("0")
                .to_string();
            let browse_flag = extract_xml_element(&body_str, "BrowseFlag")
                .unwrap_or("BrowseDirectChildren")
                .to_string();
            let start: i64 = extract_xml_element(&body_str, "StartingIndex")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let requested_count: i64 = extract_xml_element(&body_str, "RequestedCount")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let db = state.db.clone();
            let local_ip = state.local_ip;
            let port = state.port;
            let result = tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                handle_browse(
                    &conn,
                    &obj_id,
                    &browse_flag,
                    start,
                    requested_count,
                    &local_ip,
                    port,
                )
            })
            .await;
            match result {
                Ok(xml) => (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
                    xml,
                )
                    .into_response(),
                Err(_) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    soap_error(501, "Action Failed"),
                )
                    .into_response(),
            }
        }
        "GetSystemUpdateID" => {
            let xml = r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:GetSystemUpdateIDResponse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1"><Id>1</Id></u:GetSystemUpdateIDResponse></s:Body></s:Envelope>"#;
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
                xml,
            )
                .into_response()
        }
        "GetSearchCapabilities" => {
            let xml = r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:GetSearchCapabilitiesResponse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1"><SearchCaps></SearchCaps></u:GetSearchCapabilitiesResponse></s:Body></s:Envelope>"#;
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
                xml,
            )
                .into_response()
        }
        "GetSortCapabilities" => {
            let xml = r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:GetSortCapabilitiesResponse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1"><SortCaps></SortCaps></u:GetSortCapabilitiesResponse></s:Body></s:Envelope>"#;
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
                xml,
            )
                .into_response()
        }
        _ => (StatusCode::BAD_REQUEST, soap_error(401, "Invalid Action")).into_response(),
    }
}

async fn connection_manager_control_handler(
    headers: HeaderMap,
    _body: axum::body::Bytes,
) -> impl IntoResponse {
    let soap_action = headers
        .get("SOAPAction")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| extract_soap_action(s, ""))
        .unwrap_or_default();

    match soap_action.as_str() {
        "GetProtocolInfo" => {
            let source = [
                "http-get:*:audio/mpeg:*",
                "http-get:*:audio/mp4:*",
                "http-get:*:audio/x-m4a:*",
                "http-get:*:audio/flac:*",
                "http-get:*:audio/x-flac:*",
                "http-get:*:audio/ogg:*",
                "http-get:*:audio/wav:*",
                "http-get:*:audio/x-wav:*",
                "http-get:*:audio/aac:*",
                "http-get:*:audio/x-aac:*",
                "http-get:*:audio/wma:*",
                "http-get:*:audio/x-ms-wma:*",
            ]
            .join(",");
            let xml = format!(
                r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:GetProtocolInfoResponse xmlns:u="urn:schemas-upnp-org:service:ConnectionManager:1"><Source>{source}</Source><Sink></Sink></u:GetProtocolInfoResponse></s:Body></s:Envelope>"#
            );
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
                xml,
            )
                .into_response()
        }
        _ => (StatusCode::BAD_REQUEST, soap_error(401, "Invalid Action")).into_response(),
    }
}

async fn audio_stream_handler(
    State(state): State<DlnaHttpState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let db = state.db.clone();
    let track_id = id.clone();
    let file_path = match tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        conn.query_row(
            "SELECT file_path FROM tracks WHERE id=?1",
            rusqlite::params![track_id],
            |r| r.get::<_, String>(0),
        )
        .ok()
    })
    .await
    {
        Ok(Some(p)) => p,
        _ => {
            return (StatusCode::NOT_FOUND, "Track not found").into_response();
        }
    };

    let file_path = std::path::PathBuf::from(&file_path);
    if !file_path.exists() {
        return (StatusCode::NOT_FOUND, "File not found on disk").into_response();
    }

    let file_size = match tokio::fs::metadata(&file_path).await {
        Ok(m) => m.len(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Stat failed").into_response(),
    };

    let mime = crate::ffmpeg::audio_mime_type(&file_path);
    let range_header = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    if let Some(ref rh) = range_header {
        match crate::ffmpeg::parse_byte_range(rh, file_size) {
            None => {
                return Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{file_size}"))
                    .header(header::ACCEPT_RANGES, "bytes")
                    .body(Body::empty())
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
            }
            Some((start, end)) => {
                let mut file = match tokio::fs::File::open(&file_path).await {
                    Ok(f) => f,
                    Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                };
                use std::io::SeekFrom;
                if file.seek(SeekFrom::Start(start)).await.is_err() {
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
                let len = end - start + 1;
                let reader = file.take(len);
                return Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(
                        header::CONTENT_RANGE,
                        format!("bytes {start}-{end}/{file_size}"),
                    )
                    .header(header::ACCEPT_RANGES, "bytes")
                    .header(header::CONTENT_LENGTH, len.to_string())
                    .header(header::CONTENT_TYPE, mime)
                    .body(Body::from_stream(ReaderStream::new(reader)))
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
            }
        }
    }

    let file = match tokio::fs::File::open(&file_path).await {
        Ok(f) => f,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_LENGTH, file_size.to_string())
        .header(header::CONTENT_TYPE, mime)
        .header(header::ACCEPT_RANGES, "bytes")
        .body(Body::from_stream(ReaderStream::new(file)))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn event_stub_handler(_headers: HeaderMap) -> impl IntoResponse {
    let sid = format!("uuid:{}", Uuid::now_v7());
    Response::builder()
        .status(StatusCode::OK)
        .header("SID", sid)
        .header("TIMEOUT", "Second-1800")
        .body(Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

// -- Browse --------------------------------------------------------------------

struct TrackRow {
    id: String,
    title: String,
    file_path: String,
    file_size: Option<i64>,
    duration: Option<f64>,
    genre: Option<String>,
    track_number: Option<i64>,
    artist_name: Option<String>,
    album_title: Option<String>,
}

fn handle_browse(
    conn: &Connection,
    obj_id: &str,
    browse_flag: &str,
    start: i64,
    requested_count: i64,
    local_ip: &IpAddr,
    port: u16,
) -> String {
    let limit = if requested_count <= 0 {
        BROWSE_DEFAULT_LIMIT
    } else {
        requested_count
    };

    if browse_flag == "BrowseMetadata" {
        return handle_browse_metadata(conn, obj_id, local_ip, port);
    }

    // BrowseDirectChildren
    match obj_id {
        "0" => {
            let count_row =
                conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get::<_, i64>(0));
            let track_count = count_row.unwrap_or(0);
            let inner = container_xml("music", "0", "Music", "object.container", Some(track_count));
            wrap_browse_response(&[inner], 1, 1)
        }
        "music" => {
            let total = conn
                .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get::<_, i64>(0))
                .unwrap_or(0);
            let artist_count = conn
                .query_row("SELECT COUNT(*) FROM artists", [], |r| r.get::<_, i64>(0))
                .unwrap_or(0);
            let album_count = conn
                .query_row("SELECT COUNT(*) FROM albums", [], |r| r.get::<_, i64>(0))
                .unwrap_or(0);
            let genre_count: i64 = conn
                .query_row(
                    "SELECT COUNT(DISTINCT genre) FROM tracks WHERE genre IS NOT NULL AND genre!=''",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            let children = vec![
                container_xml(
                    "all",
                    "music",
                    "All Tracks",
                    "object.container.playlistContainer",
                    Some(total),
                ),
                container_xml(
                    "artists",
                    "music",
                    "Artists",
                    "object.container.person.musicArtist",
                    Some(artist_count),
                ),
                container_xml(
                    "albums",
                    "music",
                    "Albums",
                    "object.container.album.musicAlbum",
                    Some(album_count),
                ),
                container_xml(
                    "genres",
                    "music",
                    "Genres",
                    "object.container.genre.musicGenre",
                    Some(genre_count),
                ),
            ];
            wrap_browse_response(&children, children.len(), 4)
        }
        "all" => {
            let total = count_tracks_total(conn);
            let tracks = query_all_tracks(conn, start, limit);
            let items: Vec<String> = tracks
                .iter()
                .map(|t| track_to_didl(t, "all", local_ip, port))
                .collect();
            wrap_browse_response(&items, items.len(), total)
        }
        "artists" => {
            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM artists", [], |r| r.get(0))
                .unwrap_or(0);
            let mut stmt = conn
                .prepare("SELECT id, name FROM artists ORDER BY name LIMIT ?1 OFFSET ?2")
                .unwrap();
            let rows: Vec<(String, String)> = stmt
                .query_map(rusqlite::params![limit, start], |r| {
                    Ok((
                        entity_id_to_string(r.get::<_, rusqlite::types::Value>(0)?),
                        r.get::<_, String>(1).unwrap_or_default(),
                    ))
                })
                .unwrap()
                .filter_map(|r| r.ok())
                .collect();
            let album_counts: std::collections::HashMap<String, i64> = rows
                .iter()
                .map(|(id, _)| {
                    let c: i64 = conn
                        .query_row(
                            "SELECT COUNT(*) FROM albums WHERE artist_id=?1",
                            rusqlite::params![id],
                            |r| r.get(0),
                        )
                        .unwrap_or(0);
                    (id.clone(), c)
                })
                .collect();
            let items: Vec<String> = rows
                .iter()
                .map(|(id, name)| {
                    let c = album_counts.get(id).copied().unwrap_or(0);
                    container_xml(
                        &format!("artist:{id}"),
                        "artists",
                        name,
                        "object.container.person.musicArtist",
                        Some(c),
                    )
                })
                .collect();
            wrap_browse_response(&items, items.len(), total)
        }
        "albums" => {
            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM albums", [], |r| r.get(0))
                .unwrap_or(0);
            let mut stmt = conn
                .prepare(
                    "SELECT al.id, al.title, COUNT(t.id) FROM albums al LEFT JOIN tracks t ON t.album_id=al.id GROUP BY al.id ORDER BY al.title LIMIT ?1 OFFSET ?2",
                )
                .unwrap();
            let rows: Vec<(String, String, i64)> = stmt
                .query_map(rusqlite::params![limit, start], |r| {
                    Ok((
                        entity_id_to_string(r.get::<_, rusqlite::types::Value>(0)?),
                        r.get::<_, String>(1).unwrap_or_default(),
                        r.get::<_, i64>(2).unwrap_or(0),
                    ))
                })
                .unwrap()
                .filter_map(|r| r.ok())
                .collect();
            let items: Vec<String> = rows
                .iter()
                .map(|(id, title, count)| {
                    container_xml(
                        &format!("album:{id}"),
                        "albums",
                        title,
                        "object.container.album.musicAlbum",
                        Some(*count),
                    )
                })
                .collect();
            wrap_browse_response(&items, items.len(), total)
        }
        "genres" => {
            let total: i64 = conn
                .query_row(
                    "SELECT COUNT(DISTINCT genre) FROM tracks WHERE genre IS NOT NULL AND genre!=''",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            let mut stmt = conn
                .prepare(
                    "SELECT genre, COUNT(*) FROM tracks WHERE genre IS NOT NULL AND genre!='' GROUP BY genre ORDER BY genre LIMIT ?1 OFFSET ?2",
                )
                .unwrap();
            let rows: Vec<(String, i64)> = stmt
                .query_map(rusqlite::params![limit, start], |r| {
                    Ok((
                        r.get::<_, String>(0).unwrap_or_default(),
                        r.get::<_, i64>(1).unwrap_or(0),
                    ))
                })
                .unwrap()
                .filter_map(|r| r.ok())
                .collect();
            let items: Vec<String> = rows
                .iter()
                .map(|(genre, count)| {
                    container_xml(
                        &format!("genre:{}", xml_escape(genre)),
                        "genres",
                        genre,
                        "object.container.genre.musicGenre",
                        Some(*count),
                    )
                })
                .collect();
            wrap_browse_response(&items, items.len(), total)
        }
        id if id.starts_with("artist:") => {
            let artist_id = &id["artist:".len()..];
            let total: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM albums WHERE artist_id=?1",
                    rusqlite::params![artist_id],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            let mut stmt = conn
                .prepare(
                    "SELECT al.id, al.title, COUNT(t.id) FROM albums al LEFT JOIN tracks t ON t.album_id=al.id WHERE al.artist_id=?1 GROUP BY al.id ORDER BY al.title LIMIT ?2 OFFSET ?3",
                )
                .unwrap();
            let rows: Vec<(String, String, i64)> = stmt
                .query_map(rusqlite::params![artist_id, limit, start], |r| {
                    Ok((
                        entity_id_to_string(r.get::<_, rusqlite::types::Value>(0)?),
                        r.get::<_, String>(1).unwrap_or_default(),
                        r.get::<_, i64>(2).unwrap_or(0),
                    ))
                })
                .unwrap()
                .filter_map(|r| r.ok())
                .collect();
            let items: Vec<String> = rows
                .iter()
                .map(|(alid, title, count)| {
                    container_xml(
                        &format!("album:{alid}"),
                        id,
                        title,
                        "object.container.album.musicAlbum",
                        Some(*count),
                    )
                })
                .collect();
            wrap_browse_response(&items, items.len(), total)
        }
        id if id.starts_with("album:") => {
            let album_id = &id["album:".len()..];
            let total = count_album_tracks(conn, album_id);
            let tracks = query_album_tracks(conn, album_id, start, limit);
            let items: Vec<String> = tracks
                .iter()
                .map(|t| track_to_didl(t, id, local_ip, port))
                .collect();
            wrap_browse_response(&items, items.len(), total)
        }
        id if id.starts_with("genre:") => {
            let genre = &id["genre:".len()..];
            let total = count_genre_tracks(conn, genre);
            let tracks = query_genre_tracks(conn, genre, start, limit);
            let items: Vec<String> = tracks
                .iter()
                .map(|t| track_to_didl(t, id, local_ip, port))
                .collect();
            wrap_browse_response(&items, items.len(), total)
        }
        _ => soap_error(701, "No Such Object"),
    }
}

fn handle_browse_metadata(conn: &Connection, obj_id: &str, local_ip: &IpAddr, port: u16) -> String {
    match obj_id {
        "0" => {
            let inner = container_xml("0", "-1", "Root", "object.container", None);
            wrap_browse_response(&[inner], 1, 1)
        }
        "music" => {
            let total = conn
                .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get::<_, i64>(0))
                .unwrap_or(0);
            let inner = container_xml("music", "0", "Music", "object.container", Some(total));
            wrap_browse_response(&[inner], 1, 1)
        }
        id if id.starts_with("track:") => {
            let track_id = &id["track:".len()..];
            let tracks = query_track_by_id(conn, track_id);
            if tracks.is_empty() {
                return soap_error(701, "No Such Object");
            }
            let item = track_to_didl(&tracks[0], "all", local_ip, port);
            wrap_browse_response(&[item], 1, 1)
        }
        _ => soap_error(701, "No Such Object"),
    }
}

// -- SQL helpers ---------------------------------------------------------------

fn query_tracks_sql(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Vec<TrackRow> {
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map(params, |r| {
        Ok(TrackRow {
            id: entity_id_to_string(r.get::<_, rusqlite::types::Value>(0)?),
            title: r.get::<_, String>(1).unwrap_or_default(),
            file_path: r.get::<_, String>(2).unwrap_or_default(),
            file_size: r.get::<_, Option<i64>>(3)?,
            duration: r.get::<_, Option<f64>>(4)?,
            genre: r.get::<_, Option<String>>(5)?,
            track_number: r.get::<_, Option<i64>>(6)?,
            artist_name: r.get::<_, Option<String>>(7)?,
            album_title: r.get::<_, Option<String>>(8)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

const TRACK_SELECT: &str = "SELECT t.id, t.title, t.file_path, t.file_size, t.duration, t.genre, t.track_number, a.name, al.title FROM tracks t LEFT JOIN artists a ON a.id=t.artist_id LEFT JOIN albums al ON al.id=t.album_id";

fn query_all_tracks(conn: &Connection, start: i64, limit: i64) -> Vec<TrackRow> {
    let sql = format!("{TRACK_SELECT} ORDER BY t.title LIMIT ?1 OFFSET ?2");
    query_tracks_sql(conn, &sql, &[&limit, &start])
}

fn query_album_tracks(conn: &Connection, album_id: &str, start: i64, limit: i64) -> Vec<TrackRow> {
    let sql = format!(
        "{TRACK_SELECT} WHERE t.album_id=?1 ORDER BY t.track_number, t.title LIMIT ?2 OFFSET ?3"
    );
    query_tracks_sql(conn, &sql, &[&album_id, &limit, &start])
}

fn query_genre_tracks(conn: &Connection, genre: &str, start: i64, limit: i64) -> Vec<TrackRow> {
    let sql = format!("{TRACK_SELECT} WHERE t.genre=?1 ORDER BY t.title LIMIT ?2 OFFSET ?3");
    query_tracks_sql(conn, &sql, &[&genre, &limit, &start])
}

fn query_track_by_id(conn: &Connection, track_id: &str) -> Vec<TrackRow> {
    let sql = format!("{TRACK_SELECT} WHERE t.id=?1");
    query_tracks_sql(conn, &sql, &[&track_id])
}

fn count_tracks_total(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .unwrap_or(0)
}

fn count_album_tracks(conn: &Connection, album_id: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM tracks WHERE album_id=?1",
        rusqlite::params![album_id],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

fn count_genre_tracks(conn: &Connection, genre: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM tracks WHERE genre=?1",
        rusqlite::params![genre],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

// -- XML builders --------------------------------------------------------------

fn build_device_xml(friendly_name: &str, udn: &str, local_ip: IpAddr, port: u16) -> String {
    let base_url = format!("http://{}:{}", local_ip, port);
    format!(
        r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0" xmlns:dlna="urn:schemas-dlna-org:device-1-0">
  <specVersion><major>1</major><minor>0</minor></specVersion>
  <URLBase>{base_url}</URLBase>
  <device>
    <deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType>
    <friendlyName>{friendly_name}</friendlyName>
    <manufacturer>BoogieBox</manufacturer>
    <manufacturerURL>https://github.com/boogiebox</manufacturerURL>
    <modelDescription>BoogieBox DLNA Audio Server</modelDescription>
    <modelName>BoogieBox</modelName>
    <modelNumber>1</modelNumber>
    <UDN>uuid:{udn}</UDN>
    <dlna:X_DLNADOC xmlns:dlna="urn:schemas-dlna-org:device-1-0">DMS-1.50</dlna:X_DLNADOC>
    <serviceList>
      <service>
        <serviceType>urn:schemas-upnp-org:service:ContentDirectory:1</serviceType>
        <serviceId>urn:upnp-org:serviceId:ContentDirectory</serviceId>
        <SCPDURL>/dlna/ContentDirectory.xml</SCPDURL>
        <controlURL>/dlna/control/ContentDirectory</controlURL>
        <eventSubURL>/dlna/event/ContentDirectory</eventSubURL>
      </service>
      <service>
        <serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType>
        <serviceId>urn:upnp-org:serviceId:ConnectionManager</serviceId>
        <SCPDURL>/dlna/ConnectionManager.xml</SCPDURL>
        <controlURL>/dlna/control/ConnectionManager</controlURL>
        <eventSubURL>/dlna/event/ConnectionManager</eventSubURL>
      </service>
    </serviceList>
  </device>
</root>"#
    )
}

fn container_xml(
    id: &str,
    parent_id: &str,
    title: &str,
    upnp_class: &str,
    child_count: Option<i64>,
) -> String {
    let count_attr = match child_count {
        Some(n) => format!(r#" childCount="{n}""#),
        None => String::new(),
    };
    format!(
        r#"<container id="{}" parentID="{}" restricted="1" searchable="1"{count_attr}><dc:title>{}</dc:title><upnp:class>{}</upnp:class></container>"#,
        xml_escape(id),
        xml_escape(parent_id),
        xml_escape(title),
        xml_escape(upnp_class)
    )
}

fn track_to_didl(track: &TrackRow, parent_id: &str, local_ip: &IpAddr, port: u16) -> String {
    let url = format!("http://{}:{}/dlna/media/track/{}", local_ip, port, track.id);
    let title = xml_escape(&track.title);
    let artist = xml_escape(track.artist_name.as_deref().unwrap_or("Unknown Artist"));
    let album = xml_escape(track.album_title.as_deref().unwrap_or("Unknown Album"));
    let genre = xml_escape(track.genre.as_deref().unwrap_or(""));
    let track_num = track.track_number.unwrap_or(0);
    let duration_str = track
        .duration
        .map(format_duration)
        .unwrap_or_else(|| "0:00:00".to_string());
    let size_attr = track
        .file_size
        .map(|s| format!(r#" size="{s}""#))
        .unwrap_or_default();
    let mime = crate::ffmpeg::audio_mime_type(std::path::Path::new(&track.file_path));

    format!(
        r#"<item id="track:{}" parentID="{}" restricted="1"><dc:title>{title}</dc:title><upnp:class>object.item.audioItem.musicTrack</upnp:class><upnp:artist>{artist}</upnp:artist><upnp:album>{album}</upnp:album><upnp:genre>{genre}</upnp:genre><upnp:originalTrackNumber>{track_num}</upnp:originalTrackNumber><res protocolInfo="http-get:*:{mime}:*" duration="{duration_str}"{size_attr}>{}</res></item>"#,
        xml_escape(&track.id),
        xml_escape(parent_id),
        xml_escape(&url),
    )
}

fn wrap_didl(inner: &str) -> String {
    format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/" xmlns:dlna="urn:schemas-dlna-org:metadata-1-0/">{inner}</DIDL-Lite>"#
    )
}

fn wrap_browse_response(items: &[String], number_returned: usize, total: i64) -> String {
    let inner = items.join("");
    let didl = wrap_didl(&inner);
    let escaped_didl = xml_escape(&didl);
    format!(
        r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:BrowseResponse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1"><Result>{escaped_didl}</Result><NumberReturned>{number_returned}</NumberReturned><TotalMatches>{total}</TotalMatches><UpdateID>1</UpdateID></u:BrowseResponse></s:Body></s:Envelope>"#
    )
}

fn soap_error(code: u32, desc: &str) -> String {
    format!(
        r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><s:Fault><faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail><UPnPError xmlns="urn:schemas-upnp-org:control-1-0"><errorCode>{code}</errorCode><errorDescription>{desc}</errorDescription></UPnPError></detail></s:Fault></s:Body></s:Envelope>"#
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn entity_id_to_string(v: rusqlite::types::Value) -> String {
    match v {
        rusqlite::types::Value::Integer(i) => i.to_string(),
        rusqlite::types::Value::Text(s) => s,
        _ => String::new(),
    }
}

fn format_duration(seconds: f64) -> String {
    let total = seconds as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    format!("{}:{:02}:{:02}", h, m, s)
}

// -- SOAP parsing --------------------------------------------------------------

fn extract_soap_action(soap_action_header: &str, _body: &str) -> Option<String> {
    // Header format: "urn:schemas-upnp-org:service:ContentDirectory:1#Browse"
    // or: "\"urn:...#Browse\""
    let s = soap_action_header.trim().trim_matches('"');
    let pos = s.rfind('#')?;
    let action = s[pos + 1..].trim().trim_matches('"');
    if action.is_empty() {
        None
    } else {
        Some(action.to_string())
    }
}

fn extract_xml_element<'a>(xml: &'a str, element: &str) -> Option<&'a str> {
    let open = format!("<{element}>");
    let close = format!("</{element}>");
    let start = xml.find(&open)?;
    let content_start = start + open.len();
    let end = xml[content_start..].find(&close)?;
    Some(&xml[content_start..content_start + end])
}

// -- Static SCPD XML -----------------------------------------------------------

static CONTENT_DIRECTORY_SCPD: &str = r#"<?xml version="1.0"?>
<scpd xmlns="urn:schemas-upnp-org:service-1-0">
  <specVersion><major>1</major><minor>0</minor></specVersion>
  <actionList>
    <action>
      <name>Browse</name>
      <argumentList>
        <argument><name>ObjectID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_ObjectID</relatedStateVariable></argument>
        <argument><name>BrowseFlag</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_BrowseFlag</relatedStateVariable></argument>
        <argument><name>Filter</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_Filter</relatedStateVariable></argument>
        <argument><name>StartingIndex</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_Index</relatedStateVariable></argument>
        <argument><name>RequestedCount</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_Count</relatedStateVariable></argument>
        <argument><name>SortCriteria</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_SortCriteria</relatedStateVariable></argument>
        <argument><name>Result</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_Result</relatedStateVariable></argument>
        <argument><name>NumberReturned</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_Count</relatedStateVariable></argument>
        <argument><name>TotalMatches</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_Count</relatedStateVariable></argument>
        <argument><name>UpdateID</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_UpdateID</relatedStateVariable></argument>
      </argumentList>
    </action>
    <action>
      <name>GetSystemUpdateID</name>
      <argumentList>
        <argument><name>Id</name><direction>out</direction><relatedStateVariable>SystemUpdateID</relatedStateVariable></argument>
      </argumentList>
    </action>
    <action>
      <name>GetSearchCapabilities</name>
      <argumentList>
        <argument><name>SearchCaps</name><direction>out</direction><relatedStateVariable>SearchCapabilities</relatedStateVariable></argument>
      </argumentList>
    </action>
    <action>
      <name>GetSortCapabilities</name>
      <argumentList>
        <argument><name>SortCaps</name><direction>out</direction><relatedStateVariable>SortCapabilities</relatedStateVariable></argument>
      </argumentList>
    </action>
  </actionList>
  <serviceStateTable>
    <stateVariable><name>A_ARG_TYPE_ObjectID</name><sendEventsAttribute>no</sendEventsAttribute><dataType>string</dataType></stateVariable>
    <stateVariable><name>A_ARG_TYPE_Result</name><sendEventsAttribute>no</sendEventsAttribute><dataType>string</dataType></stateVariable>
    <stateVariable><name>A_ARG_TYPE_BrowseFlag</name><sendEventsAttribute>no</sendEventsAttribute><dataType>string</dataType><allowedValueList><allowedValue>BrowseMetadata</allowedValue><allowedValue>BrowseDirectChildren</allowedValue></allowedValueList></stateVariable>
    <stateVariable><name>A_ARG_TYPE_Filter</name><sendEventsAttribute>no</sendEventsAttribute><dataType>string</dataType></stateVariable>
    <stateVariable><name>A_ARG_TYPE_SortCriteria</name><sendEventsAttribute>no</sendEventsAttribute><dataType>string</dataType></stateVariable>
    <stateVariable><name>A_ARG_TYPE_Index</name><sendEventsAttribute>no</sendEventsAttribute><dataType>ui4</dataType></stateVariable>
    <stateVariable><name>A_ARG_TYPE_Count</name><sendEventsAttribute>no</sendEventsAttribute><dataType>ui4</dataType></stateVariable>
    <stateVariable><name>A_ARG_TYPE_UpdateID</name><sendEventsAttribute>no</sendEventsAttribute><dataType>ui4</dataType></stateVariable>
    <stateVariable><name>SearchCapabilities</name><sendEventsAttribute>no</sendEventsAttribute><dataType>string</dataType></stateVariable>
    <stateVariable><name>SortCapabilities</name><sendEventsAttribute>no</sendEventsAttribute><dataType>string</dataType></stateVariable>
    <stateVariable sendEventsAttribute="yes"><name>SystemUpdateID</name><dataType>ui4</dataType></stateVariable>
    <stateVariable sendEventsAttribute="yes"><name>ContainerUpdateIDs</name><dataType>string</dataType></stateVariable>
  </serviceStateTable>
</scpd>"#;

static CONNECTION_MANAGER_SCPD: &str = r#"<?xml version="1.0"?>
<scpd xmlns="urn:schemas-upnp-org:service-1-0">
  <specVersion><major>1</major><minor>0</minor></specVersion>
  <actionList>
    <action>
      <name>GetProtocolInfo</name>
      <argumentList>
        <argument><name>Source</name><direction>out</direction><relatedStateVariable>SourceProtocolInfo</relatedStateVariable></argument>
        <argument><name>Sink</name><direction>out</direction><relatedStateVariable>SinkProtocolInfo</relatedStateVariable></argument>
      </argumentList>
    </action>
    <action>
      <name>GetCurrentConnectionIDs</name>
      <argumentList>
        <argument><name>ConnectionIDs</name><direction>out</direction><relatedStateVariable>CurrentConnectionIDs</relatedStateVariable></argument>
      </argumentList>
    </action>
    <action>
      <name>GetCurrentConnectionInfo</name>
      <argumentList>
        <argument><name>ConnectionID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_ConnectionID</relatedStateVariable></argument>
        <argument><name>RcsID</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_RcsID</relatedStateVariable></argument>
        <argument><name>AVTransportID</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_AVTransportID</relatedStateVariable></argument>
        <argument><name>ProtocolInfo</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_ProtocolInfo</relatedStateVariable></argument>
        <argument><name>PeerConnectionManager</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_ConnectionManager</relatedStateVariable></argument>
        <argument><name>PeerConnectionID</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_ConnectionID</relatedStateVariable></argument>
        <argument><name>Direction</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_Direction</relatedStateVariable></argument>
        <argument><name>Status</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_ConnectionStatus</relatedStateVariable></argument>
      </argumentList>
    </action>
  </actionList>
  <serviceStateTable>
    <stateVariable sendEventsAttribute="yes"><name>SourceProtocolInfo</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEventsAttribute="yes"><name>SinkProtocolInfo</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEventsAttribute="yes"><name>CurrentConnectionIDs</name><dataType>string</dataType></stateVariable>
    <stateVariable><name>A_ARG_TYPE_ConnectionStatus</name><sendEventsAttribute>no</sendEventsAttribute><dataType>string</dataType></stateVariable>
    <stateVariable><name>A_ARG_TYPE_ConnectionManager</name><sendEventsAttribute>no</sendEventsAttribute><dataType>string</dataType></stateVariable>
    <stateVariable><name>A_ARG_TYPE_Direction</name><sendEventsAttribute>no</sendEventsAttribute><dataType>string</dataType></stateVariable>
    <stateVariable><name>A_ARG_TYPE_ProtocolInfo</name><sendEventsAttribute>no</sendEventsAttribute><dataType>string</dataType></stateVariable>
    <stateVariable><name>A_ARG_TYPE_ConnectionID</name><sendEventsAttribute>no</sendEventsAttribute><dataType>i4</dataType></stateVariable>
    <stateVariable><name>A_ARG_TYPE_AVTransportID</name><sendEventsAttribute>no</sendEventsAttribute><dataType>i4</dataType></stateVariable>
    <stateVariable><name>A_ARG_TYPE_RcsID</name><sendEventsAttribute>no</sendEventsAttribute><dataType>i4</dataType></stateVariable>
  </serviceStateTable>
</scpd>"#;

// -- Tests ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_converts_seconds() {
        assert_eq!(format_duration(225.0), "0:03:45");
        assert_eq!(format_duration(3661.0), "1:01:01");
        assert_eq!(format_duration(0.0), "0:00:00");
    }

    #[test]
    fn xml_escape_replaces_special_chars() {
        assert_eq!(xml_escape("a & b"), "a &amp; b");
        assert_eq!(xml_escape("<tag>"), "&lt;tag&gt;");
        assert_eq!(xml_escape(r#"say "hi""#), "say &quot;hi&quot;");
        assert_eq!(xml_escape("it's"), "it&apos;s");
    }

    #[test]
    fn extract_soap_action_from_header() {
        let header = "\"urn:schemas-upnp-org:service:ContentDirectory:1#Browse\"";
        assert_eq!(extract_soap_action(header, ""), Some("Browse".to_string()));
        let header2 = "urn:schemas-upnp-org:service:ContentDirectory:1#GetSystemUpdateID";
        assert_eq!(
            extract_soap_action(header2, ""),
            Some("GetSystemUpdateID".to_string())
        );
    }

    #[test]
    fn extract_xml_element_finds_value() {
        let xml = "<s:Body><u:Browse><ObjectID>music</ObjectID><BrowseFlag>BrowseDirectChildren</BrowseFlag></u:Browse></s:Body>";
        assert_eq!(extract_xml_element(xml, "ObjectID"), Some("music"));
        assert_eq!(
            extract_xml_element(xml, "BrowseFlag"),
            Some("BrowseDirectChildren")
        );
        assert_eq!(extract_xml_element(xml, "Missing"), None);
    }

    #[test]
    fn build_device_xml_embeds_friendly_name_udn_and_address() {
        let xml = build_device_xml(
            "My Box",
            "uuid-123",
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5)),
            8200,
        );
        assert!(xml.contains("<friendlyName>My Box</friendlyName>"));
        assert!(xml.contains("uuid:uuid-123"));
        assert!(xml.contains("192.168.1.5:8200"));
    }

    #[test]
    fn soap_error_wraps_code_and_description() {
        let xml = soap_error(701, "No Such Object");
        assert!(xml.contains("701"));
        assert!(xml.contains("No Such Object"));
    }

    #[test]
    fn wrap_didl_and_wrap_browse_response_embed_items_and_counts() {
        let didl = wrap_didl("<item/>");
        assert!(didl.contains("<item/>"));
        assert!(didl.starts_with("&lt;DIDL-Lite") || didl.contains("DIDL-Lite"));

        let resp = wrap_browse_response(&["<item/>".to_string()], 1, 5);
        assert!(resp.contains("<NumberReturned>1</NumberReturned>"));
        assert!(resp.contains("<TotalMatches>5</TotalMatches>"));
    }

    #[test]
    fn container_xml_includes_child_count_when_present() {
        let with_count = container_xml("id1", "parent1", "Name", "object.container", Some(3));
        assert!(with_count.contains("childCount=\"3\""));
        let without_count = container_xml("id1", "parent1", "Name", "object.container", None);
        assert!(!without_count.contains("childCount"));
    }

    #[test]
    fn track_to_didl_embeds_stream_url_and_metadata() {
        let track = TrackRow {
            id: "t1".to_string(),
            title: "My Song".to_string(),
            file_path: "/music/t1.mp3".to_string(),
            file_size: Some(1000),
            duration: Some(125.0),
            genre: Some("Rock".to_string()),
            track_number: Some(3),
            artist_name: Some("Some Artist".to_string()),
            album_title: Some("Some Album".to_string()),
        };
        let xml = track_to_didl(&track, "all", &IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 8200);
        assert!(xml.contains("My Song"));
        assert!(xml.contains("Some Artist"));
        assert!(xml.contains("Some Album"));
        assert!(xml.contains("http://10.0.0.1:8200/dlna/media/track/t1"));
    }

    // -- DB-backed handle_browse tests -----------------------------------------

    fn temp_db(prefix: &str) -> Connection {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("dlna-test-{prefix}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        boogiebox_db::init_db(&dir).unwrap().connection
    }

    fn seed_track(conn: &Connection) -> (String, String, String) {
        let library_id = uuid::Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO libraries(id, path, name) VALUES (?, '/music', 'Lib')",
            rusqlite::params![library_id],
        )
        .unwrap();
        let artist_id = uuid::Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO artists(id, name) VALUES (?, 'Test Artist')",
            rusqlite::params![artist_id],
        )
        .unwrap();
        let album_id = uuid::Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO albums(id, title, artist_id) VALUES (?, 'Test Album', ?)",
            rusqlite::params![album_id, artist_id],
        )
        .unwrap();
        let track_id = uuid::Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path, genre) \
             VALUES (?, ?, ?, ?, 'Test Track', '/music/t.mp3', 'Rock')",
            rusqlite::params![track_id, library_id, artist_id, album_id],
        )
        .unwrap();
        (artist_id, album_id, track_id)
    }

    fn local_ip() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))
    }

    #[test]
    fn handle_browse_root_lists_the_music_container() {
        let conn = temp_db("browse-root");
        seed_track(&conn);
        let xml = handle_browse(&conn, "0", "BrowseDirectChildren", 0, 0, &local_ip(), 8200);
        assert!(xml.contains("id=&quot;music&quot;"));
        assert!(xml.contains("<NumberReturned>1</NumberReturned>"));
    }

    #[test]
    fn handle_browse_music_lists_all_artists_albums_and_genres_containers() {
        let conn = temp_db("browse-music");
        seed_track(&conn);
        let xml = handle_browse(
            &conn,
            "music",
            "BrowseDirectChildren",
            0,
            0,
            &local_ip(),
            8200,
        );
        assert!(xml.contains("id=&quot;all&quot;"));
        assert!(xml.contains("id=&quot;artists&quot;"));
        assert!(xml.contains("id=&quot;albums&quot;"));
        assert!(xml.contains("id=&quot;genres&quot;"));
    }

    #[test]
    fn handle_browse_all_lists_tracks_as_didl_items() {
        let conn = temp_db("browse-all");
        seed_track(&conn);
        let xml = handle_browse(
            &conn,
            "all",
            "BrowseDirectChildren",
            0,
            0,
            &local_ip(),
            8200,
        );
        assert!(xml.contains("Test Track"));
        assert!(xml.contains("<NumberReturned>1</NumberReturned>"));
    }

    #[test]
    fn handle_browse_artists_then_drills_into_artist_albums() {
        let conn = temp_db("browse-artists");
        let (artist_id, _album_id, _track_id) = seed_track(&conn);
        let artists_xml = handle_browse(
            &conn,
            "artists",
            "BrowseDirectChildren",
            0,
            0,
            &local_ip(),
            8200,
        );
        assert!(artists_xml.contains("Test Artist"));

        let artist_container_xml = handle_browse(
            &conn,
            &format!("artist:{artist_id}"),
            "BrowseDirectChildren",
            0,
            0,
            &local_ip(),
            8200,
        );
        assert!(artist_container_xml.contains("Test Album"));
    }

    #[test]
    fn handle_browse_albums_then_drills_into_album_tracks() {
        let conn = temp_db("browse-albums");
        let (_artist_id, album_id, _track_id) = seed_track(&conn);
        let albums_xml = handle_browse(
            &conn,
            "albums",
            "BrowseDirectChildren",
            0,
            0,
            &local_ip(),
            8200,
        );
        assert!(albums_xml.contains("Test Album"));

        let album_tracks_xml = handle_browse(
            &conn,
            &format!("album:{album_id}"),
            "BrowseDirectChildren",
            0,
            0,
            &local_ip(),
            8200,
        );
        assert!(album_tracks_xml.contains("Test Track"));
    }

    #[test]
    fn handle_browse_genres_then_drills_into_genre_tracks() {
        let conn = temp_db("browse-genres");
        seed_track(&conn);
        let genres_xml = handle_browse(
            &conn,
            "genres",
            "BrowseDirectChildren",
            0,
            0,
            &local_ip(),
            8200,
        );
        assert!(genres_xml.contains("Rock"));

        let genre_tracks_xml = handle_browse(
            &conn,
            "genre:Rock",
            "BrowseDirectChildren",
            0,
            0,
            &local_ip(),
            8200,
        );
        assert!(genre_tracks_xml.contains("Test Track"));
    }

    #[test]
    fn handle_browse_unknown_object_id_returns_soap_error() {
        let conn = temp_db("browse-unknown");
        let xml = handle_browse(
            &conn,
            "no-such-id",
            "BrowseDirectChildren",
            0,
            0,
            &local_ip(),
            8200,
        );
        assert!(xml.contains("701"));
    }

    #[test]
    fn handle_browse_metadata_for_root_music_and_track() {
        let conn = temp_db("browse-metadata");
        let (_artist_id, _album_id, track_id) = seed_track(&conn);

        let root_xml = handle_browse(&conn, "0", "BrowseMetadata", 0, 0, &local_ip(), 8200);
        assert!(root_xml.contains("id=&quot;0&quot;"));

        let music_xml = handle_browse(&conn, "music", "BrowseMetadata", 0, 0, &local_ip(), 8200);
        assert!(music_xml.contains("id=&quot;music&quot;"));

        let track_xml = handle_browse(
            &conn,
            &format!("track:{track_id}"),
            "BrowseMetadata",
            0,
            0,
            &local_ip(),
            8200,
        );
        assert!(track_xml.contains("Test Track"));

        let missing_xml = handle_browse(
            &conn,
            "track:does-not-exist",
            "BrowseMetadata",
            0,
            0,
            &local_ip(),
            8200,
        );
        assert!(missing_xml.contains("701"));
    }

    #[test]
    fn read_dlna_settings_returns_defaults_and_generates_a_udn() {
        let conn = temp_db("dlna-settings");
        let settings = read_dlna_settings(&conn);
        assert!(!settings.enabled);
        assert_eq!(settings.port, DEFAULT_DLNA_PORT);
        assert_eq!(settings.friendly_name, "BoogieBox");
        assert!(!settings.udn.is_empty());

        // A second read reuses the persisted UDN instead of generating a new one.
        let settings_again = read_dlna_settings(&conn);
        assert_eq!(settings.udn, settings_again.udn);
    }
}
