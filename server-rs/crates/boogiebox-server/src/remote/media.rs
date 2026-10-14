//! Play On media layer (`wip/play-on-renderers-plan.md` §4.4–4.6): unguessable short-lived media tokens,
//! per-renderer format decisions, renderer DIDL-Lite, and the unauthenticated `GET|HEAD /media/r/{token}`
//! handler renderers fetch audio and artwork from.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
use rand::Rng;

use crate::media_serve::{self, WavLayout};
use crate::upnp::device::SinkCaps;
use crate::upnp::didl::{format_duration, wrap_didl, xml_escape};
use crate::{ffmpeg, SharedState};

/// Extra lifetime granted beyond an item's duration.
pub const TOKEN_GRACE: Duration = Duration::from_secs(6 * 60 * 60);
/// Album art size requested for renderers (one of the cached thumbnail sizes).
pub const RENDERER_ART_SIZE: &str = "800";
const TOKEN_BYTES: usize = 24;

/// Admin-selected format policy for a renderer (`upnp_renderers.format_policy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatPolicy {
    /// Send files as-is when the renderer lists the format, otherwise transcode.
    Auto,
    /// Always send files as-is.
    Direct,
    /// Like `Auto`, but transcode to lossless WAV even when the renderer doesn't list WAV.
    Lossless,
    /// Always transcode to MP3 320.
    Mp3,
}

impl FormatPolicy {
    /// Parses the stored value; unknown values fall back to `Auto`.
    pub fn parse(s: &str) -> Self {
        match s {
            "direct" => Self::Direct,
            "lossless" => Self::Lossless,
            "mp3" => Self::Mp3,
            _ => Self::Auto,
        }
    }
}

/// How an item is delivered to a renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Delivery {
    /// The original file, byte-ranged.
    Direct,
    /// CBR MP3 320 transcode (byte-seekable when the duration is known).
    Mp3,
    /// Lossless PCM WAV transcode (byte-exact seeking; needs a known duration).
    Wav,
}

/// Chooses the delivery for a file of `mime` on a renderer accepting `sink`.
pub fn choose_delivery(
    mime: &str,
    policy: FormatPolicy,
    sink: &SinkCaps,
    duration_known: bool,
) -> Delivery {
    match policy {
        FormatPolicy::Direct => Delivery::Direct,
        FormatPolicy::Mp3 => Delivery::Mp3,
        FormatPolicy::Auto | FormatPolicy::Lossless => {
            if mime != "application/octet-stream" && sink.accepts(mime) {
                Delivery::Direct
            } else if duration_known
                && (policy == FormatPolicy::Lossless || sink.accepts("audio/wav"))
            {
                Delivery::Wav
            } else {
                Delivery::Mp3
            }
        }
    }
}

/// MIME advertised for a delivery: the renderer's own spelling for direct files, fixed for transcodes.
pub fn delivery_mime(delivery: Delivery, file_mime: &str, sink: &SinkCaps) -> String {
    match delivery {
        Delivery::Direct => sink.listed_alias(file_mime).unwrap_or_else(|| {
            file_mime
                .split(';')
                .next()
                .unwrap_or(file_mime)
                .trim()
                .to_string()
        }),
        Delivery::Mp3 => sink
            .listed_alias("audio/mpeg")
            .unwrap_or_else(|| "audio/mpeg".into()),
        Delivery::Wav => sink
            .listed_alias("audio/wav")
            .unwrap_or_else(|| "audio/wav".into()),
    }
}

/// DLNA 4th-field flags: byte seeking supported (`OP=01`), conversion indicator for transcodes, streaming
/// transfer mode + DLNA 1.5 flags.
pub fn dlna_flags(delivery: Delivery) -> &'static str {
    match delivery {
        Delivery::Direct => {
            "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000"
        }
        Delivery::Mp3 | Delivery::Wav => {
            "DLNA.ORG_OP=01;DLNA.ORG_CI=1;DLNA.ORG_FLAGS=01700000000000000000000000000000"
        }
    }
}

/// What a token grants access to. Paths are resolved (and authorized) when the token is issued.
#[derive(Debug, Clone, PartialEq)]
pub enum MediaTarget {
    /// A library track or rendered mix file.
    Audio {
        /// Id for logs (track id or `mix:<id>`).
        log_id: String,
        /// Absolute path on disk.
        file_path: PathBuf,
        /// Duration in seconds, when known.
        duration: Option<f64>,
        /// Source sample rate, when known.
        sample_rate: Option<u32>,
        /// Source channel count, when known.
        channels: Option<u16>,
    },
    /// Album artwork (800px cached thumbnail).
    AlbumArt {
        /// Album id.
        album_id: String,
    },
}

/// A resolved token.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaRef {
    /// What to serve.
    pub target: MediaTarget,
    /// How to serve audio (ignored for art).
    pub delivery: Delivery,
    /// MIME advertised to the renderer.
    pub mime: String,
    /// Renderer UDN the token was issued for (revoked with its session).
    pub owner: String,
    /// Expiry.
    pub expires_at: Instant,
}

/// An audio token saved with its session, so the renderer's stream URL keeps working after a server restart.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedToken {
    /// The token string (as in the renderer's URL).
    pub token: String,
    /// Id for logs.
    pub log_id: String,
    /// Absolute file path (authorized when the token was first issued).
    pub file_path: PathBuf,
    /// Duration (s).
    pub duration: Option<f64>,
    /// Source sample rate.
    pub sample_rate: Option<u32>,
    /// Source channel count.
    pub channels: Option<u16>,
    /// Delivery.
    pub delivery: Delivery,
    /// MIME advertised to the renderer.
    pub mime: String,
    /// Expiry (ms since epoch).
    pub expires_at_ms: u64,
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Registry of live media tokens.
#[derive(Debug, Clone, Default)]
pub struct MediaTokens {
    inner: Arc<Mutex<HashMap<String, MediaRef>>>,
}

fn new_token() -> String {
    let mut bytes = [0u8; TOKEN_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// True for strings shaped like a token (rejects anything else before touching the registry).
pub fn is_token_shaped(s: &str) -> bool {
    s.len() == TOKEN_BYTES * 2 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

impl MediaTokens {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, MediaRef>> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Issues a token valid for `ttl`.
    pub fn issue(
        &self,
        target: MediaTarget,
        delivery: Delivery,
        mime: String,
        owner: &str,
        ttl: Duration,
    ) -> String {
        let token = new_token();
        self.map().insert(
            token.clone(),
            MediaRef {
                target,
                delivery,
                mime,
                owner: owner.to_string(),
                expires_at: Instant::now() + ttl,
            },
        );
        token
    }

    /// Resolves a live token.
    pub fn get(&self, token: &str) -> Option<MediaRef> {
        let now = Instant::now();
        self.map()
            .get(token)
            .filter(|r| r.expires_at > now)
            .cloned()
    }

    /// Saveable copy of a live audio token (art tokens are not worth keeping).
    pub fn export(&self, token: &str) -> Option<PersistedToken> {
        let r = self.get(token)?;
        let MediaTarget::Audio {
            log_id,
            file_path,
            duration,
            sample_rate,
            channels,
        } = r.target
        else {
            return None;
        };
        let remaining = r.expires_at.saturating_duration_since(Instant::now());
        Some(PersistedToken {
            token: token.to_string(),
            log_id,
            file_path,
            duration,
            sample_rate,
            channels,
            delivery: r.delivery,
            mime: r.mime,
            expires_at_ms: epoch_ms() + remaining.as_millis() as u64,
        })
    }

    /// Re-registers a saved token for `owner`; false when it is malformed or already expired.
    pub fn restore(&self, owner: &str, saved: &PersistedToken) -> bool {
        let now = epoch_ms();
        if !is_token_shaped(&saved.token) || saved.expires_at_ms <= now {
            return false;
        }
        self.map().insert(
            saved.token.clone(),
            MediaRef {
                target: MediaTarget::Audio {
                    log_id: saved.log_id.clone(),
                    file_path: saved.file_path.clone(),
                    duration: saved.duration,
                    sample_rate: saved.sample_rate,
                    channels: saved.channels,
                },
                delivery: saved.delivery,
                mime: saved.mime.clone(),
                owner: owner.to_string(),
                expires_at: Instant::now() + Duration::from_millis(saved.expires_at_ms - now),
            },
        );
        true
    }

    /// Revokes every token issued for `owner`.
    pub fn revoke_owner(&self, owner: &str) {
        self.map().retain(|_, r| r.owner != owner);
    }

    /// Drops expired tokens; returns how many were removed.
    pub fn sweep(&self, now: Instant) -> usize {
        let mut map = self.map();
        let before = map.len();
        map.retain(|_, r| r.expires_at > now);
        before - map.len()
    }

    /// Number of live entries (diagnostics/tests).
    pub fn len(&self) -> usize {
        self.map().len()
    }

    /// True when no tokens are held.
    pub fn is_empty(&self) -> bool {
        self.map().is_empty()
    }
}

/// Token lifetime for an item of `duration` seconds.
pub fn token_ttl(duration: Option<f64>) -> Duration {
    let secs = duration
        .filter(|d| d.is_finite() && *d > 0.0)
        .unwrap_or(0.0)
        .min(24.0 * 3600.0);
    Duration::from_secs_f64(secs) + TOKEN_GRACE
}

/// Metadata written into a renderer DIDL item.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DidlItem {
    /// Item id (`bb:track:<id>` / `bb:mix:<id>`).
    pub id: String,
    /// Title.
    pub title: String,
    /// Artist.
    pub artist: Option<String>,
    /// Album.
    pub album: Option<String>,
    /// Genre.
    pub genre: Option<String>,
    /// Track number.
    pub track_number: Option<i64>,
    /// Duration in seconds.
    pub duration: Option<f64>,
    /// Album art URL.
    pub art_url: Option<String>,
    /// Media URL.
    pub res_url: String,
    /// `protocolInfo` MIME.
    pub mime: String,
    /// DLNA flags (4th protocolInfo field).
    pub dlna_flags: String,
    /// Size in bytes for direct files.
    pub size: Option<u64>,
}

fn opt_el(tag: &str, value: Option<&str>) -> String {
    value
        .filter(|v| !v.trim().is_empty())
        .map(|v| format!("<{tag}>{}</{tag}>", xml_escape(v)))
        .unwrap_or_default()
}

/// Builds the DIDL-Lite document passed as `CurrentURIMetaData` / `NextURIMetaData`.
pub fn renderer_didl(item: &DidlItem) -> String {
    let artist = item.artist.as_deref();
    let duration_attr = item
        .duration
        .filter(|d| *d > 0.0)
        .map(|d| format!(r#" duration="{}.000""#, format_duration(d)))
        .unwrap_or_default();
    let size_attr = item
        .size
        .map(|s| format!(r#" size="{s}""#))
        .unwrap_or_default();
    let track = item
        .track_number
        .filter(|n| *n > 0)
        .map(|n| format!("<upnp:originalTrackNumber>{n}</upnp:originalTrackNumber>"))
        .unwrap_or_default();
    wrap_didl(&format!(
        r#"<item id="{id}" parentID="bb" restricted="1"><dc:title>{title}</dc:title>{artist}{creator}{album}{genre}{track}{art}<upnp:class>object.item.audioItem.musicTrack</upnp:class><res protocolInfo="http-get:*:{mime}:{flags}"{duration_attr}{size_attr}>{url}</res></item>"#,
        id = xml_escape(&item.id),
        title = xml_escape(if item.title.trim().is_empty() {
            "Untitled"
        } else {
            &item.title
        }),
        artist = opt_el("upnp:artist", artist),
        creator = opt_el("dc:creator", artist),
        album = opt_el("upnp:album", item.album.as_deref()),
        genre = opt_el("upnp:genre", item.genre.as_deref()),
        art = opt_el("upnp:albumArtURI", item.art_url.as_deref()),
        mime = xml_escape(&item.mime),
        flags = xml_escape(&item.dlna_flags),
        url = xml_escape(&item.res_url),
    ))
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "Not found").into_response()
}

/// `GET|HEAD /media/r/{token}` — renderers have no session cookie; the token is the authorization.
pub async fn media_handler(
    State(state): State<SharedState>,
    Path(token): Path<String>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    if !is_token_shaped(&token) {
        return not_found();
    }
    let remote = state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .remote
        .clone();
    let Some(media) = remote.media().get(&token) else {
        return not_found();
    };
    let head_only = method == Method::HEAD;
    let wants_features = headers
        .get("getcontentFeatures.dlna.org")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim() == "1");
    tracing::debug!(
        target: "remote",
        token = %&token[..6],
        method = %method,
        range = headers.get(axum::http::header::RANGE).and_then(|v| v.to_str().ok()).unwrap_or("-"),
        "renderer media request"
    );

    match &media.target {
        MediaTarget::AlbumArt { album_id } => {
            let params = crate::routes::artwork_routes::ArtParams {
                size: Some(RENDERER_ART_SIZE.into()),
                refresh: None,
            };
            let mut resp = crate::routes::artwork_routes::album_art_handler(
                State(state),
                Path(album_id.clone()),
                Query(params),
                headers,
            )
            .await;
            if let Ok(v) = "Interactive".parse() {
                resp.headers_mut().insert("transferMode.dlna.org", v);
            }
            resp
        }
        MediaTarget::Audio {
            log_id,
            file_path,
            duration,
            sample_rate,
            channels,
        } => {
            if !file_path.is_file() {
                return not_found();
            }
            let mut extra = vec![("transferMode.dlna.org", "Streaming".to_string())];
            if wants_features {
                extra.push((
                    "contentFeatures.dlna.org",
                    dlna_flags(media.delivery).to_string(),
                ));
            }
            let range = headers
                .get(axum::http::header::RANGE)
                .and_then(|v| v.to_str().ok());
            match media.delivery {
                Delivery::Direct => {
                    media_serve::serve_file(file_path, &media.mime, &headers, head_only, &extra)
                        .await
                }
                Delivery::Mp3 => {
                    media_serve::serve_mp3_transcode(media_serve::Mp3Transcode {
                        file_path,
                        duration: *duration,
                        range,
                        quality: "high",
                        replay_gain: false,
                        head_only,
                        log_id,
                        log_ext: "renderer",
                        extra: &extra,
                    })
                    .await
                }
                Delivery::Wav => {
                    let rate = sample_rate.unwrap_or(44_100);
                    let layout = WavLayout::new(
                        duration.unwrap_or(0.0),
                        rate,
                        channels.unwrap_or(2),
                        if rate > 48_000 { 24 } else { 16 },
                    );
                    media_serve::serve_wav_transcode(media_serve::WavTranscode {
                        file_path,
                        layout,
                        range,
                        head_only,
                        log_id,
                        extra: &extra,
                    })
                    .await
                }
            }
        }
    }
}

/// Builds the base URL renderers use to reach this server's media route.
pub fn media_url(local_ip: std::net::IpAddr, port: u16, token: &str) -> String {
    match local_ip {
        std::net::IpAddr::V6(v6) => format!("http://[{v6}]:{port}/media/r/{token}"),
        std::net::IpAddr::V4(v4) => format!("http://{v4}:{port}/media/r/{token}"),
    }
}

/// File MIME for a path (same mapping browser playback uses).
pub fn file_mime(path: &std::path::Path) -> &'static str {
    ffmpeg::audio_mime_type(path)
}

/// An item ready to hand to a renderer.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedItem {
    /// Media URL (`CurrentURI` / `NextURI`).
    pub url: String,
    /// DIDL-Lite metadata.
    pub didl: String,
    /// Chosen delivery.
    pub delivery: Delivery,
    /// Duration in seconds, when known.
    pub duration: Option<f64>,
    /// Display title (for UI/logs).
    pub title: String,
}

/// Why an item could not be prepared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareError {
    /// The renderer is unknown to the registry.
    UnknownRenderer,
    /// Track/mix not found (or not owned by the requesting user).
    NotFound,
    /// The file is missing on disk.
    FileMissing,
    /// No local address can reach the renderer.
    NoRoute,
    /// Database failure.
    Db,
}

/// Renderer-specific inputs shared by every item prepared for it.
struct RendererContext {
    sink: SinkCaps,
    policy: FormatPolicy,
    base_ip: std::net::IpAddr,
}

/// Parses the first album id out of a mix's `cover_album_ids` JSON (ids may be strings or numbers).
pub fn first_cover_album_id(json: Option<&str>) -> Option<String> {
    let ids: Vec<serde_json::Value> = serde_json::from_str(json?).ok()?;
    ids.into_iter().find_map(|v| match v {
        serde_json::Value::String(s) if !s.is_empty() => Some(s),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

impl crate::remote::RemoteManager {
    async fn renderer_context(
        &self,
        db: &crate::DbPool,
        udn: &str,
    ) -> Result<RendererContext, PrepareError> {
        let entry = self
            .registry()
            .get(udn)
            .ok_or(PrepareError::UnknownRenderer)?;
        let policy = {
            let db = db.clone();
            let udn = udn.to_string();
            tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                boogiebox_db::remote::get_renderer(&conn, &udn)
                    .map(|r| r.map(|r| r.format_policy).unwrap_or_default())
            })
            .await
            .map_err(|_| PrepareError::Db)?
            .map_err(|_| PrepareError::Db)?
        };
        let renderer_ip: std::net::IpAddr = reqwest::Url::parse(&entry.location)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.trim_matches(['[', ']']).to_string()))
            .and_then(|h| h.parse().ok())
            .ok_or(PrepareError::NoRoute)?;
        let base_ip = self
            .running_settings()
            .and_then(|s| s.interface_ip)
            .map(std::net::IpAddr::V4)
            .or_else(|| crate::upnp::discovery::local_ip_for(renderer_ip))
            .ok_or(PrepareError::NoRoute)?;
        Ok(RendererContext {
            sink: entry.caps.sink,
            policy: FormatPolicy::parse(&policy),
            base_ip,
        })
    }

    fn art_url(
        &self,
        ctx: &RendererContext,
        udn: &str,
        album_id: Option<&str>,
        ttl: Duration,
    ) -> Option<String> {
        let album_id = album_id.filter(|a| !a.is_empty())?;
        let token = self.media().issue(
            MediaTarget::AlbumArt {
                album_id: album_id.to_string(),
            },
            Delivery::Direct,
            "image/jpeg".into(),
            udn,
            ttl,
        );
        Some(media_url(ctx.base_ip, self.http_port(), &token))
    }

    /// Prepares a library track for renderer `udn`: format decision, media + art tokens, DIDL.
    pub async fn prepare_track(
        &self,
        db: &crate::DbPool,
        udn: &str,
        track_id: &str,
    ) -> Result<PreparedItem, PrepareError> {
        let ctx = self.renderer_context(db, udn).await?;
        let meta = {
            let db = db.clone();
            let id = track_id.to_string();
            tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                boogiebox_db::remote::get_track_media_meta(&conn, &id)
            })
            .await
            .map_err(|_| PrepareError::Db)?
            .map_err(|_| PrepareError::Db)?
            .ok_or(PrepareError::NotFound)?
        };
        let file_path = PathBuf::from(&meta.file_path);
        if !file_path.is_file() {
            return Err(PrepareError::FileMissing);
        }
        let duration = meta.duration.filter(|d| *d > 0.0);
        let src_mime = file_mime(&file_path);
        let delivery = choose_delivery(src_mime, ctx.policy, &ctx.sink, duration.is_some());
        let mime = delivery_mime(delivery, src_mime, &ctx.sink);
        let ttl = token_ttl(duration);
        let token = self.media().issue(
            MediaTarget::Audio {
                log_id: meta.id.clone(),
                file_path,
                duration,
                sample_rate: meta.sample_rate.and_then(|r| u32::try_from(r).ok()),
                channels: meta.channels.and_then(|c| u16::try_from(c).ok()),
            },
            delivery,
            mime.clone(),
            udn,
            ttl,
        );
        let url = media_url(ctx.base_ip, self.http_port(), &token);
        let art_url = self.art_url(&ctx, udn, meta.album_id.as_deref(), ttl);
        let didl = renderer_didl(&DidlItem {
            id: format!("bb:track:{}", meta.id),
            title: meta.title.clone(),
            artist: meta.artist,
            album: meta.album,
            genre: meta.genre,
            track_number: meta.track_number,
            duration,
            art_url,
            res_url: url.clone(),
            mime,
            dlna_flags: dlna_flags(delivery).into(),
            size: (delivery == Delivery::Direct)
                .then_some(meta.file_size)
                .flatten()
                .and_then(|s| u64::try_from(s).ok()),
        });
        Ok(PreparedItem {
            url,
            didl,
            delivery,
            duration,
            title: meta.title,
        })
    }

    /// Prepares a rendered BoogieMix output owned by `user_id` for renderer `udn`.
    pub async fn prepare_mix(
        &self,
        db: &crate::DbPool,
        udn: &str,
        output_id: &str,
        user_id: &str,
    ) -> Result<PreparedItem, PrepareError> {
        let ctx = self.renderer_context(db, udn).await?;
        let (row, file_path) = {
            let db = db.clone();
            let output = boogiebox_db::music::coerce_entity_id(output_id);
            let user = boogiebox_db::music::coerce_entity_id(user_id);
            tokio::task::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(|p| p.into_inner());
                let row = boogiebox_db::boogiemix::get_mix_output(&conn, &output, &user)?;
                let file = boogiebox_db::boogiemix::get_mix_output_file(&conn, &output, &user)?;
                Ok::<_, boogiebox_db::jobs::JobError>(row.zip(file.map(|(p, _)| p)))
            })
            .await
            .map_err(|_| PrepareError::Db)?
            .map_err(|_| PrepareError::Db)?
            .ok_or(PrepareError::NotFound)?
        };
        let file_path = PathBuf::from(file_path);
        if !file_path.is_file() {
            return Err(PrepareError::FileMissing);
        }
        let duration = Some(row.duration_sec).filter(|d| *d > 0.0);
        let src_mime = file_mime(&file_path);
        let delivery = choose_delivery(src_mime, ctx.policy, &ctx.sink, duration.is_some());
        let mime = delivery_mime(delivery, src_mime, &ctx.sink);
        let ttl = token_ttl(duration);
        let token = self.media().issue(
            MediaTarget::Audio {
                log_id: format!("mix:{output_id}"),
                file_path,
                duration,
                sample_rate: None,
                channels: None,
            },
            delivery,
            mime.clone(),
            udn,
            ttl,
        );
        let url = media_url(ctx.base_ip, self.http_port(), &token);
        let art_url = self.art_url(
            &ctx,
            udn,
            first_cover_album_id(row.cover_album_ids.as_deref()).as_deref(),
            ttl,
        );
        let title = if row.name.trim().is_empty() {
            "BoogieMix".to_string()
        } else {
            row.name.clone()
        };
        let didl = renderer_didl(&DidlItem {
            id: format!("bb:mix:{output_id}"),
            title: title.clone(),
            artist: Some("BoogieMix".into()),
            album: row.playlist_name.clone(),
            genre: None,
            track_number: None,
            duration,
            art_url,
            res_url: url.clone(),
            mime,
            dlna_flags: dlna_flags(delivery).into(),
            size: (delivery == Delivery::Direct)
                .then(|| u64::try_from(row.file_size_bytes).ok())
                .flatten()
                .filter(|s| *s > 0),
        });
        Ok(PreparedItem {
            url,
            didl,
            delivery,
            duration,
            title,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(sink: &str) -> SinkCaps {
        SinkCaps::parse(sink)
    }

    #[test]
    fn delivery_decision_matrix() {
        let wiim = caps("http-get:*:audio/flac:*,http-get:*:audio/m4a:*,http-get:*:audio/mpeg:*,http-get:*:audio/wav:*");
        assert_eq!(
            choose_delivery("audio/flac", FormatPolicy::Auto, &wiim, true),
            Delivery::Direct
        );
        assert_eq!(
            choose_delivery("audio/mp4", FormatPolicy::Auto, &wiim, true),
            Delivery::Direct
        );
        assert_eq!(
            choose_delivery("audio/aiff", FormatPolicy::Auto, &wiim, true),
            Delivery::Wav
        );
        assert_eq!(
            choose_delivery("audio/aiff", FormatPolicy::Auto, &wiim, false),
            Delivery::Mp3
        );
        assert_eq!(
            choose_delivery("application/octet-stream", FormatPolicy::Auto, &wiim, true),
            Delivery::Wav
        );
        assert_eq!(
            choose_delivery("audio/aiff", FormatPolicy::Direct, &wiim, true),
            Delivery::Direct
        );
        assert_eq!(
            choose_delivery("audio/flac", FormatPolicy::Mp3, &wiim, true),
            Delivery::Mp3
        );

        let mp3_only = caps("http-get:*:audio/mpeg:*");
        assert_eq!(
            choose_delivery("audio/flac", FormatPolicy::Auto, &mp3_only, true),
            Delivery::Mp3
        );
        assert_eq!(
            choose_delivery("audio/flac", FormatPolicy::Lossless, &mp3_only, true),
            Delivery::Wav
        );
        let unknown = SinkCaps::default();
        assert_eq!(
            choose_delivery("audio/flac", FormatPolicy::Auto, &unknown, true),
            Delivery::Mp3
        );
    }

    #[test]
    fn policy_parse_and_mime_flags() {
        assert_eq!(FormatPolicy::parse("direct"), FormatPolicy::Direct);
        assert_eq!(FormatPolicy::parse("lossless"), FormatPolicy::Lossless);
        assert_eq!(FormatPolicy::parse("mp3"), FormatPolicy::Mp3);
        assert_eq!(FormatPolicy::parse("auto"), FormatPolicy::Auto);
        assert_eq!(FormatPolicy::parse("weird"), FormatPolicy::Auto);

        let wiim = caps("http-get:*:audio/m4a:*,http-get:*:audio/mp3:*");
        assert_eq!(
            delivery_mime(Delivery::Direct, "audio/mp4", &wiim),
            "audio/m4a"
        );
        assert_eq!(
            delivery_mime(Delivery::Direct, "audio/ogg; codecs=opus", &wiim),
            "audio/ogg"
        );
        assert_eq!(
            delivery_mime(Delivery::Mp3, "audio/flac", &wiim),
            "audio/mp3"
        );
        assert_eq!(
            delivery_mime(Delivery::Wav, "audio/aiff", &wiim),
            "audio/wav"
        );
        assert!(dlna_flags(Delivery::Direct).contains("CI=0"));
        assert!(dlna_flags(Delivery::Wav).contains("CI=1"));
        assert_eq!(file_mime(std::path::Path::new("x.FLAC")), "audio/flac");
    }

    #[test]
    fn audio_tokens_export_and_restore_with_the_same_string() {
        let t = MediaTokens::default();
        let audio = t.issue(
            MediaTarget::Audio {
                log_id: "t1".into(),
                file_path: PathBuf::from("D:/Music/a.flac"),
                duration: Some(200.0),
                sample_rate: Some(44100),
                channels: Some(2),
            },
            Delivery::Wav,
            "audio/wav".into(),
            "wiim",
            Duration::from_secs(600),
        );
        let art = t.issue(
            MediaTarget::AlbumArt {
                album_id: "al1".into(),
            },
            Delivery::Direct,
            "image/jpeg".into(),
            "wiim",
            Duration::from_secs(600),
        );
        assert!(t.export(&art).is_none(), "art is not saved");
        assert!(t.export("missing").is_none());
        let saved = t.export(&audio).unwrap();
        let json = serde_json::to_string(&saved).unwrap();
        assert!(json.contains("\"delivery\":\"wav\""));

        let fresh = MediaTokens::default();
        assert!(fresh.restore("wiim", &serde_json::from_str(&json).unwrap()));
        let back = fresh.get(&audio).unwrap();
        assert_eq!(
            (back.delivery, back.mime.as_str(), back.owner.as_str()),
            (Delivery::Wav, "audio/wav", "wiim")
        );
        assert!(matches!(back.target, MediaTarget::Audio { duration: Some(d), .. } if d == 200.0));
        assert!(
            !fresh.restore(
                "wiim",
                &PersistedToken {
                    expires_at_ms: 1,
                    ..saved.clone()
                }
            ),
            "expired"
        );
        assert!(
            !fresh.restore(
                "wiim",
                &PersistedToken {
                    token: "short".into(),
                    ..saved
                }
            ),
            "malformed"
        );
    }

    #[test]
    fn token_registry_issue_get_revoke_sweep() {
        let t = MediaTokens::default();
        let target = MediaTarget::AlbumArt {
            album_id: "al1".into(),
        };
        let a = t.issue(
            target.clone(),
            Delivery::Direct,
            "image/jpeg".into(),
            "wiim",
            Duration::from_secs(60),
        );
        let b = t.issue(
            target.clone(),
            Delivery::Direct,
            "image/jpeg".into(),
            "other",
            Duration::from_secs(60),
        );
        let expired = t.issue(
            target,
            Delivery::Direct,
            "image/jpeg".into(),
            "wiim",
            Duration::ZERO,
        );
        assert!(is_token_shaped(&a) && a != b);
        assert_eq!(t.get(&a).unwrap().owner, "wiim");
        assert!(t.get(&expired).is_none());
        assert!(t.get("nope").is_none());
        assert_eq!(t.len(), 3);
        assert_eq!(t.sweep(Instant::now()), 1);
        t.revoke_owner("wiim");
        assert!(t.get(&a).is_none());
        assert!(t.get(&b).is_some());
        assert!(!t.is_empty());
        assert!(!is_token_shaped("abc"));
        assert!(!is_token_shaped(&"g".repeat(48)));
    }

    #[test]
    fn ttl_and_url_helpers() {
        assert_eq!(token_ttl(None), TOKEN_GRACE);
        assert_eq!(token_ttl(Some(60.0)), TOKEN_GRACE + Duration::from_secs(60));
        assert_eq!(token_ttl(Some(f64::NAN)), TOKEN_GRACE);
        assert_eq!(
            token_ttl(Some(1e12)),
            TOKEN_GRACE + Duration::from_secs(86_400)
        );
        assert_eq!(
            media_url("10.0.0.2".parse().unwrap(), 3001, "ab"),
            "http://10.0.0.2:3001/media/r/ab"
        );
        assert_eq!(
            media_url("fe80::1".parse().unwrap(), 3001, "ab"),
            "http://[fe80::1]:3001/media/r/ab"
        );
    }

    fn token_of(url: &str) -> String {
        url.rsplit('/').next().unwrap().to_string()
    }

    #[tokio::test]
    async fn prepare_track_and_mix() {
        use crate::remote::registry::{tests::description, RendererCaps};
        let state = crate::test_support::build_test_state(&crate::test_support::temp_db_path(
            "remote-prepare",
        ));
        let db = state.db.clone().unwrap();
        let remote = state.remote.clone();
        let dir = std::env::temp_dir().join(format!("bb-prepare-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let flac = dir.join("a.flac");
        let aiff = dir.join("b.aiff");
        let aiff2 = dir.join("c.aiff");
        std::fs::write(&flac, b"flac").unwrap();
        std::fs::write(&aiff, b"aiff").unwrap();
        std::fs::write(&aiff2, b"aiff").unwrap();
        {
            let conn = db.lock().unwrap();
            conn.execute_batch(
                "INSERT INTO libraries(id, path, name) VALUES('lib1', 'D:\\Music', 'Music');
                 INSERT INTO artists(id, name) VALUES('ar1', 'Massive Attack');
                 INSERT INTO albums(id, title, artist_id) VALUES('al1', 'Mezzanine', 'ar1');",
            )
            .unwrap();
            for (id, path, title, dur) in [
                ("t1", flac.to_string_lossy().to_string(), "Angel", 379.0),
                ("t2", aiff.to_string_lossy().to_string(), "Teardrop", 330.0),
                (
                    "t3",
                    aiff2.to_string_lossy().to_string(),
                    "No Duration",
                    0.0,
                ),
                (
                    "t4",
                    dir.join("gone.flac").to_string_lossy().to_string(),
                    "Gone",
                    1.0,
                ),
            ] {
                conn.execute(
                    "INSERT INTO tracks(id, library_id, file_path, title, artist_id, album_id, duration, file_size, sample_rate, channels)
                     VALUES(?1, 'lib1', ?2, ?3, 'ar1', 'al1', ?4, 4, 96000, 2)",
                    rusqlite::params![id, path, title, dur],
                )
                .unwrap();
            }
            boogiebox_db::remote::upsert_renderer(
                &conn,
                &boogiebox_db::remote::RendererUpsert {
                    udn: "r1",
                    friendly_name: "Den",
                    manufacturer: None,
                    model_name: None,
                    vendor: "generic",
                    location_url: "http://127.0.0.1:1/d.xml",
                    capabilities: "{}",
                },
            )
            .unwrap();
        }
        remote.registry().insert_described(
            description("r1", "127.0.0.1"),
            RendererCaps {
                sink: SinkCaps::parse(
                    "http-get:*:audio/flac:*,http-get:*:audio/wav:*,http-get:*:audio/mpeg:*",
                ),
                ..Default::default()
            },
            "http://127.0.0.1:1/d.xml",
            60,
            Instant::now(),
        );
        remote.set_http_port(3999);

        let item = remote.prepare_track(&db, "r1", "t1").await.unwrap();
        assert_eq!(item.delivery, Delivery::Direct);
        assert!(item.url.starts_with("http://127.0.0.1:3999/media/r/"));
        assert!(item.didl.contains("<dc:title>Angel</dc:title>"));
        assert!(item.didl.contains("<upnp:album>Mezzanine</upnp:album>"));
        assert!(item.didl.contains("upnp:albumArtURI"));
        assert!(item.didl.contains(r#"size="4""#));
        match remote.media().get(&token_of(&item.url)).unwrap().target {
            MediaTarget::Audio {
                file_path,
                sample_rate,
                ..
            } => {
                assert_eq!(file_path, flac);
                assert_eq!(sample_rate, Some(96_000));
            }
            other => panic!("unexpected {other:?}"),
        }

        let wav = remote.prepare_track(&db, "r1", "t2").await.unwrap();
        assert_eq!(wav.delivery, Delivery::Wav);
        assert!(wav
            .didl
            .contains("http-get:*:audio/wav:DLNA.ORG_OP=01;DLNA.ORG_CI=1"));
        assert!(!wav.didl.contains("size="), "no size for transcodes");
        assert_eq!(
            remote
                .prepare_track(&db, "r1", "t3")
                .await
                .unwrap()
                .delivery,
            Delivery::Mp3
        );

        boogiebox_db::remote::update_renderer_prefs(
            &db.lock().unwrap(),
            "r1",
            &boogiebox_db::remote::RendererPrefs {
                format_policy: Some("mp3".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            remote
                .prepare_track(&db, "r1", "t1")
                .await
                .unwrap()
                .delivery,
            Delivery::Mp3
        );

        assert_eq!(
            remote.prepare_track(&db, "nobody", "t1").await,
            Err(PrepareError::UnknownRenderer)
        );
        assert_eq!(
            remote.prepare_track(&db, "r1", "missing").await,
            Err(PrepareError::NotFound)
        );
        assert_eq!(
            remote.prepare_track(&db, "r1", "t4").await,
            Err(PrepareError::FileMissing)
        );
        assert_eq!(
            remote.prepare_mix(&db, "r1", "mix-x", "u1").await,
            Err(PrepareError::NotFound)
        );

        // A renderer whose location isn't an IP literal can't be routed to.
        let mut odd = description("r2", "127.0.0.1");
        odd.udn = "r2".into();
        remote.registry().insert_described(
            odd,
            RendererCaps::default(),
            "http://speaker.local/d.xml",
            60,
            Instant::now(),
        );
        assert_eq!(
            remote.prepare_track(&db, "r2", "t1").await,
            Err(PrepareError::NoRoute)
        );
    }

    #[test]
    fn cover_album_id_parsing() {
        assert_eq!(
            first_cover_album_id(Some(r#"["al1","al2"]"#)).as_deref(),
            Some("al1")
        );
        assert_eq!(first_cover_album_id(Some("[42]")).as_deref(), Some("42"));
        assert_eq!(
            first_cover_album_id(Some(r#"["",null,"x"]"#)).as_deref(),
            Some("x")
        );
        assert_eq!(first_cover_album_id(Some("not json")), None);
        assert_eq!(first_cover_album_id(None), None);
    }

    #[test]
    fn didl_item_escapes_and_includes_optional_fields() {
        let didl = renderer_didl(&DidlItem {
            id: "bb:track:1".into(),
            title: "Tom & Jerry".into(),
            artist: Some("A <B>".into()),
            album: Some("Album".into()),
            genre: Some(" ".into()),
            track_number: Some(3),
            duration: Some(245.0),
            art_url: Some("http://10.0.0.2:3001/media/r/art".into()),
            res_url: "http://10.0.0.2:3001/media/r/x?y=1&z=2".into(),
            mime: "audio/flac".into(),
            dlna_flags: dlna_flags(Delivery::Direct).into(),
            size: Some(1234),
        });
        assert!(didl.contains("<dc:title>Tom &amp; Jerry</dc:title>"));
        assert!(didl.contains(
            "<upnp:artist>A &lt;B&gt;</upnp:artist><dc:creator>A &lt;B&gt;</dc:creator>"
        ));
        assert!(!didl.contains("upnp:genre"), "blank genre omitted");
        assert!(didl.contains("<upnp:originalTrackNumber>3</upnp:originalTrackNumber>"));
        assert!(
            didl.contains("<upnp:albumArtURI>http://10.0.0.2:3001/media/r/art</upnp:albumArtURI>")
        );
        assert!(didl.contains(r#"protocolInfo="http-get:*:audio/flac:DLNA.ORG_OP=01"#));
        assert!(didl.contains(r#"duration="0:04:05.000" size="1234""#));
        assert!(didl.contains("x?y=1&amp;z=2"));
        // Parses as XML.
        assert!(crate::upnp::xml::parse(&didl).is_ok());

        let bare = renderer_didl(&DidlItem {
            id: "i".into(),
            res_url: "u".into(),
            mime: "audio/mpeg".into(),
            ..Default::default()
        });
        assert!(bare.contains("<dc:title>Untitled</dc:title>"));
        assert!(!bare.contains("duration="));
        assert!(!bare.contains("albumArtURI"));
    }
}
