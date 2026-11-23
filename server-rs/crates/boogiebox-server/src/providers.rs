//! Defines Rust server support logic for Providers.

use reqwest::Client;
use serde_json::Value;
use std::time::Instant;

const USER_AGENT: &str = "BoogieBox/1.0";

/// Resolves a provider's API base URL, checking an env var override first so tests
/// can point requests at a local `wiremock` server instead of the real provider
/// (see `wip/server-rust-coverage-gap-plan.md` Phase 3). Production code paths never
/// set these env vars, so this is a no-op outside tests.
fn provider_base_url(env_var: &str, default: &'static str) -> String {
    std::env::var(env_var).unwrap_or_else(|_| default.to_owned())
}

fn discogs_api_base() -> String {
    provider_base_url("BOOGIEBOX_DISCOGS_API_BASE", "https://api.discogs.com")
}

fn deezer_api_base() -> String {
    provider_base_url("BOOGIEBOX_DEEZER_API_BASE", "https://api.deezer.com")
}

fn spotify_accounts_base() -> String {
    provider_base_url(
        "BOOGIEBOX_SPOTIFY_ACCOUNTS_BASE",
        "https://accounts.spotify.com",
    )
}

fn spotify_api_base() -> String {
    provider_base_url("BOOGIEBOX_SPOTIFY_API_BASE", "https://api.spotify.com")
}

fn lrclib_api_base() -> String {
    provider_base_url("BOOGIEBOX_LRCLIB_API_BASE", "https://lrclib.net")
}

fn lyrics_ovh_api_base() -> String {
    provider_base_url("BOOGIEBOX_LYRICS_OVH_API_BASE", "https://api.lyrics.ovh")
}

pub(crate) fn lastfm_api_base() -> String {
    provider_base_url(
        "BOOGIEBOX_LASTFM_API_BASE",
        "https://ws.audioscrobbler.com/2.0/",
    )
}

// -- Last.fm key health (wip/lastfm-replacement-plan.md phase 0) ----------------
//
// From 2026-12-01 Last.fm only accepts the keys of Pro subscribers. A refused key (error 10 or 26, or HTTP 403) is
// remembered here, so every lane and the scrobbler stop calling Last.fm, nothing is recorded as "no data", and
// Settings can say why. Last.fm support is being removed; this only covers releases before that.

/// What Last.fm last said about an API key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LastfmKeyHealth {
    /// Not asked yet (or a refusal old enough to ask again).
    Unknown,
    Ok,
    Rejected,
}

/// A refused key is tried once more after this long (its owner may have bought Pro).
const LASTFM_REJECTED_RECHECK: std::time::Duration = std::time::Duration::from_secs(6 * 3600);

/// Error text of a Last.fm call that was skipped or refused because of the key.
pub const LASTFM_KEY_REJECTED: &str = "key-rejected";

static LASTFM_KEY_HEALTH: std::sync::Mutex<Vec<(u64, LastfmKeyHealth, Instant)>> =
    std::sync::Mutex::new(Vec::new());

fn lastfm_key_hash(key: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.trim().hash(&mut hasher);
    hasher.finish()
}

/// What Last.fm last said about `key`.
pub fn lastfm_key_health(key: &str) -> LastfmKeyHealth {
    let hash = lastfm_key_hash(key);
    let known = LASTFM_KEY_HEALTH.lock().unwrap_or_else(|p| p.into_inner());
    match known.iter().find(|(h, _, _)| *h == hash) {
        Some((_, LastfmKeyHealth::Rejected, at)) if at.elapsed() >= LASTFM_REJECTED_RECHECK => {
            LastfmKeyHealth::Unknown
        }
        Some((_, health, _)) => *health,
        None => LastfmKeyHealth::Unknown,
    }
}

/// False while Last.fm refuses `key`: callers skip Last.fm instead of calling it.
pub fn lastfm_key_usable(key: &str) -> bool {
    lastfm_key_health(key) != LastfmKeyHealth::Rejected
}

pub(crate) fn record_lastfm_key_health(key: &str, health: LastfmKeyHealth) {
    let hash = lastfm_key_hash(key);
    let mut known = LASTFM_KEY_HEALTH.lock().unwrap_or_else(|p| p.into_inner());
    let before = known.iter().position(|(h, _, _)| *h == hash);
    let was = before.map(|i| known[i].1);
    if health == LastfmKeyHealth::Rejected && was != Some(LastfmKeyHealth::Rejected) {
        tracing::warn!(
            "Last.fm refused the API key (keys need a Pro subscription from 1 December 2026): \
             Last.fm lookups and scrobbles are paused"
        );
    }
    let entry = (hash, health, Instant::now());
    match before {
        Some(i) => known[i] = entry,
        None => {
            // Keys rarely change; keep the list small.
            if known.len() >= 16 {
                known.remove(0);
            }
            known.push(entry);
        }
    }
}

/// True when Last.fm's reply refuses the key itself: error 10 (invalid key) or 26 (suspended), or a 403 with no
/// other error code (Last.fm also answers 403 for an expired user session, error 9). Recorded either way, so a
/// working key clears an old refusal.
pub(crate) fn lastfm_reply_rejects_key(key: &str, status: u16, body: &Value) -> bool {
    let rejected = match body["error"].as_i64() {
        Some(code) => matches!(code, 10 | 26),
        None => status == 403,
    };
    if rejected {
        record_lastfm_key_health(key, LastfmKeyHealth::Rejected);
    } else if (200..300).contains(&status) {
        record_lastfm_key_health(key, LastfmKeyHealth::Ok);
    }
    rejected
}

/// Status, JSON body (the parse error when it isn't JSON) and whether the key was refused.
async fn read_lastfm_reply(
    key: &str,
    response: reqwest::Response,
) -> (u16, Result<Value, String>, bool) {
    let status = response.status().as_u16();
    let body = response.json::<Value>().await.map_err(|e| e.to_string());
    let rejected = lastfm_reply_rejects_key(key, status, body.as_ref().unwrap_or(&Value::Null));
    (status, body, rejected)
}

/// Asks Last.fm once whether it takes `key` (Settings › Last.fm).
pub async fn check_lastfm_key(client: &Client, key: &str) -> LastfmKeyHealth {
    let url = format!(
        "{}?method=chart.gettopartists&api_key={}&format=json&limit=1",
        lastfm_api_base(),
        urlencoding::encode(key.trim())
    );
    let t0 = log_request("lastfm", "key_check", "");
    let Ok(response) = client.get(&url).timeout(PROVIDER_TIMEOUT).send().await else {
        return LastfmKeyHealth::Unknown;
    };
    log_response("lastfm", "key_check", t0, response.status());
    let (status, _, rejected) = read_lastfm_reply(key, response).await;
    if rejected {
        LastfmKeyHealth::Rejected
    } else if (200..300).contains(&status) {
        LastfmKeyHealth::Ok
    } else {
        LastfmKeyHealth::Unknown
    }
}

/// Logs an outbound metadata-provider request. Only visible when scan debug
/// logging is enabled — routed via this module's target to `scan-debug.log`.
fn log_request(provider: &str, action: &str, detail: &str) -> Instant {
    tracing::debug!("provider call → {provider} {action} {detail}");
    Instant::now()
}

/// Logs the outcome of an outbound metadata-provider request: HTTP status
/// (when available) and elapsed time.
fn log_response(provider: &str, action: &str, start: Instant, status: impl std::fmt::Display) {
    tracing::debug!(
        "provider call ← {provider} {action} status={status} elapsed_ms={}",
        start.elapsed().as_millis()
    );
}

/// A conservatively validated provider artist result. Identity and image are
/// returned together so callers cannot persist an ID from a different hit.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtistProviderMatch {
    pub external_id: String,
    pub canonical_name: String,
    pub image_url: Option<String>,
    pub confidence: f64,
}

/// Provider-neutral related artist candidate used by the local resolver.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct RelatedArtistCandidate {
    pub external_id: Option<String>,
    pub name: String,
    pub url: Option<String>,
    pub image_url: Option<String>,
    pub match_score: Option<f64>,
    pub rank: usize,
}

fn normalize_artist_name(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn valid_artist_image(value: &str) -> bool {
    !value.is_empty()
        && !value.contains("spacer")
        && !value.contains("artist_default")
        && !value.contains("default_avatar")
        && !value.starts_with("https://st.discogs.com")
}

/// Discogs `format` search-result entries mix physical media (Vinyl, CD) with
/// release-type descriptors (Album, EP, Single, Compilation). Scans for a
/// known descriptor rather than assuming the first entry is the type.
fn discogs_release_type_from_formats(formats: &[Value]) -> Option<String> {
    const KNOWN_TYPES: &[(&str, &str)] = &[
        ("compilation", "Compilation"),
        ("maxi-single", "Single"),
        ("ep", "EP"),
        ("single", "Single"),
        ("album", "Album"),
    ];
    formats.iter().filter_map(|v| v.as_str()).find_map(|entry| {
        let lower = entry.to_ascii_lowercase();
        KNOWN_TYPES
            .iter()
            .find(|(needle, _)| lower == *needle)
            .map(|(_, label)| (*label).to_owned())
    })
}

// ── Discogs ───────────────────────────────────────────────────────────────────

fn normalize_discogs_token(value: &str) -> String {
    let lower = value.to_lowercase();
    let no_brackets = regex_strip_brackets(&lower);
    let alphanum: String = no_brackets
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect();
    alphanum.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn regex_strip_brackets(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth_sq = 0u32;
    let mut depth_paren = 0u32;
    for c in s.chars() {
        match c {
            '[' => depth_sq += 1,
            ']' => {
                depth_sq = depth_sq.saturating_sub(1);
            }
            '(' => depth_paren += 1,
            ')' => {
                depth_paren = depth_paren.saturating_sub(1);
            }
            _ => {
                if depth_sq == 0 && depth_paren == 0 {
                    out.push(c);
                }
            }
        }
    }
    out
}

fn score_release_title_match(result: &Value, artist_name: &str, album_title: &str) -> i32 {
    let raw_title = result["title"].as_str().unwrap_or("");

    let (left, right) = if raw_title.contains(" - ") {
        let parts: Vec<&str> = raw_title.splitn(2, " - ").collect();
        (parts[0], parts.get(1).copied().unwrap_or(""))
    } else {
        ("", raw_title)
    };

    let target_artist = normalize_discogs_token(artist_name);
    let target_album = normalize_discogs_token(album_title);
    let result_artist = normalize_discogs_token(left);
    let result_album = normalize_discogs_token(right);

    let mut score = 0i32;
    if !result_album.is_empty() && !target_album.is_empty() {
        if result_album == target_album {
            score += 6;
        } else if result_album.contains(&target_album) || target_album.contains(&result_album) {
            score += 3;
        }
    }
    if !result_artist.is_empty() && !target_artist.is_empty() {
        if result_artist == target_artist {
            score += 4;
        } else if result_artist.contains(&target_artist) || target_artist.contains(&result_artist) {
            score += 2;
        }
    }
    if result["year"].is_number() {
        score += 1;
    }
    score
}

fn score_discogs_cover_result(result: &Value, artist_name: &str, album_title: &str) -> i32 {
    let cover = result["cover_image"].as_str().unwrap_or("");
    if cover.is_empty() || cover.contains("spacer") || cover.starts_with("https://st.discogs.com") {
        return -1;
    }
    score_release_title_match(result, artist_name, album_title)
}

fn pick_discogs_cover_image(
    results: &[Value],
    artist_name: &str,
    album_title: &str,
) -> Option<String> {
    let placeholder = "https://st.discogs.com";
    let mut scored: Vec<(&Value, i32)> = results
        .iter()
        .map(|r| (r, score_discogs_cover_result(r, artist_name, album_title)))
        .filter(|(_, s)| *s >= 0)
        .collect();
    scored.sort_by_key(|(_, s)| std::cmp::Reverse(*s));

    let hit = scored.first().map(|(r, _)| *r).or_else(|| {
        results.iter().find(|r| {
            let cover = r["cover_image"].as_str().unwrap_or("");
            !cover.is_empty() && !cover.contains("spacer") && !cover.starts_with(placeholder)
        })
    })?;

    hit["cover_image"].as_str().map(str::to_owned)
}

/// Documents the Search Discogs Album Cover public API surface.
pub async fn search_discogs_album_cover(
    client: &Client,
    token: &str,
    artist_name: &str,
    album_title: &str,
) -> Option<String> {
    let q = format!("{} {}", artist_name, album_title);
    let url = format!(
        "{}/database/search?type=release&q={}&per_page=3&page=1",
        discogs_api_base(),
        urlencoding::encode(q.trim())
    );

    let t0 = log_request("discogs", "album_cover", &q);
    let resp = client
        .get(&url)
        .header("Authorization", format!("Discogs token={token}"))
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .ok()?;
    log_response("discogs", "album_cover", t0, resp.status());

    if !resp.status().is_success() {
        return None;
    }

    let data: Value = resp.json().await.ok()?;
    let results = data["results"].as_array()?;
    pick_discogs_cover_image(results, artist_name, album_title)
}

fn pick_discogs_artist_match(results: &[Value], artist_name: &str) -> Option<ArtistProviderMatch> {
    let target = normalize_artist_name(artist_name);
    results.iter().find_map(|result| {
        let canonical_name = result["title"].as_str()?.trim();
        if normalize_artist_name(canonical_name) != target {
            return None;
        }
        let external_id = result["id"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| result["id"].as_i64().map(|value| value.to_string()))?;
        let image_url = result["cover_image"]
            .as_str()
            .filter(|value| valid_artist_image(value))
            .or_else(|| {
                result["thumb"]
                    .as_str()
                    .filter(|value| valid_artist_image(value))
            })
            .map(str::to_owned);
        Some(ArtistProviderMatch {
            external_id,
            canonical_name: canonical_name.to_owned(),
            image_url,
            confidence: 0.9,
        })
    })
}

/// Searches Discogs for a conservatively validated artist identity.
pub async fn search_discogs_artist_match(
    client: &Client,
    token: &str,
    artist_name: &str,
) -> Option<ArtistProviderMatch> {
    let url = format!(
        "{}/database/search?type=artist&q={}&per_page=3&page=1",
        discogs_api_base(),
        urlencoding::encode(artist_name.trim())
    );

    let t0 = log_request("discogs", "artist_match", artist_name);
    let resp = client
        .get(&url)
        .header("Authorization", format!("Discogs token={token}"))
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .ok()?;
    log_response("discogs", "artist_match", t0, resp.status());

    if !resp.status().is_success() {
        return None;
    }

    let data: Value = resp.json().await.ok()?;
    let results = data["results"].as_array()?;
    pick_discogs_artist_match(results, artist_name)
}

/// Documents the Search Discogs Artist Image public API surface.
pub async fn search_discogs_artist_image(
    client: &Client,
    token: &str,
    artist_name: &str,
) -> Option<String> {
    search_discogs_artist_match(client, token, artist_name)
        .await?
        .image_url
}

// ── Deezer ────────────────────────────────────────────────────────────────────

fn pick_deezer_artist_match(artists: &[Value], artist_name: &str) -> Option<ArtistProviderMatch> {
    let target = normalize_artist_name(artist_name);
    artists.iter().find_map(|artist| {
        let canonical_name = artist["name"].as_str()?.trim();
        if normalize_artist_name(canonical_name) != target {
            return None;
        }
        let external_id = artist["id"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| artist["id"].as_i64().map(|value| value.to_string()))?;
        let image_url = artist["picture_xl"]
            .as_str()
            .or_else(|| artist["picture_big"].as_str())
            .or_else(|| artist["picture_medium"].as_str())
            .filter(|value| valid_artist_image(value))
            .map(str::to_owned);
        Some(ArtistProviderMatch {
            external_id,
            canonical_name: canonical_name.to_owned(),
            image_url,
            confidence: 0.9,
        })
    })
}

/// Searches Deezer for a conservatively validated artist identity.
pub async fn search_deezer_artist_match(
    client: &Client,
    artist_name: &str,
) -> Option<ArtistProviderMatch> {
    let url = format!(
        "{}/search/artist?q={}",
        deezer_api_base(),
        urlencoding::encode(artist_name.trim())
    );

    let t0 = log_request("deezer", "artist_match", artist_name);
    let resp = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .ok()?;
    log_response("deezer", "artist_match", t0, resp.status());

    if !resp.status().is_success() {
        return None;
    }

    let data: Value = resp.json().await.ok()?;
    let artists = data["data"].as_array()?;
    pick_deezer_artist_match(artists, artist_name)
}

/// Documents the Search Deezer Artist Image public API surface.
pub async fn search_deezer_artist_image(client: &Client, artist_name: &str) -> Option<String> {
    search_deezer_artist_match(client, artist_name)
        .await?
        .image_url
}

// ── Spotify ───────────────────────────────────────────────────────────────────

/// Documents the Get Spotify Access Token public API surface.
pub async fn get_spotify_access_token(
    client: &Client,
    client_id: &str,
    client_secret: &str,
) -> Option<String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let credentials = STANDARD.encode(format!("{client_id}:{client_secret}"));
    let t0 = log_request("spotify", "access_token", "client_credentials");
    let resp = client
        .post(format!("{}/api/token", spotify_accounts_base()))
        .header("Authorization", format!("Basic {credentials}"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("grant_type=client_credentials")
        .send()
        .await
        .ok()?;
    log_response("spotify", "access_token", t0, resp.status());

    if !resp.status().is_success() {
        return None;
    }

    let data: Value = resp.json().await.ok()?;
    data["access_token"].as_str().map(str::to_owned)
}

fn pick_spotify_artist_match(artists: &[Value], artist_name: &str) -> Option<ArtistProviderMatch> {
    let target = normalize_artist_name(artist_name);
    artists.iter().find_map(|artist| {
        let canonical_name = artist["name"].as_str()?.trim();
        if normalize_artist_name(canonical_name) != target {
            return None;
        }
        let external_id = artist["id"].as_str()?.trim();
        if external_id.is_empty() {
            return None;
        }
        let image_url = artist["images"]
            .as_array()
            .and_then(|images| images.first())
            .and_then(|image| image["url"].as_str())
            .filter(|value| valid_artist_image(value))
            .map(str::to_owned);
        Some(ArtistProviderMatch {
            external_id: external_id.to_owned(),
            canonical_name: canonical_name.to_owned(),
            image_url,
            confidence: 0.9,
        })
    })
}

/// Searches Spotify with an already acquired client-credentials token.
pub async fn search_spotify_artist_match_with_token(
    client: &Client,
    token: &str,
    artist_name: &str,
) -> Option<ArtistProviderMatch> {
    let url = format!(
        "{}/v1/search?type=artist&limit=3&q={}",
        spotify_api_base(),
        urlencoding::encode(artist_name.trim())
    );
    let t0 = log_request("spotify", "artist_match", artist_name);
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .ok()?;
    log_response("spotify", "artist_match", t0, resp.status());
    if !resp.status().is_success() {
        return None;
    }
    let data: Value = resp.json().await.ok()?;
    pick_spotify_artist_match(data["artists"]["items"].as_array()?, artist_name)
}

/// Searches Spotify for a conservatively validated artist identity.
pub async fn search_spotify_artist_match(
    client: &Client,
    client_id: &str,
    client_secret: &str,
    artist_name: &str,
) -> Option<ArtistProviderMatch> {
    let token = get_spotify_access_token(client, client_id, client_secret).await?;
    search_spotify_artist_match_with_token(client, &token, artist_name).await
}

/// Documents the Search Spotify Artist Image public API surface.
pub async fn search_spotify_artist_image(
    client: &Client,
    client_id: &str,
    client_secret: &str,
    artist_name: &str,
) -> Option<String> {
    search_spotify_artist_match(client, client_id, client_secret, artist_name)
        .await?
        .image_url
}

// ── Metadata search (multi-provider) ─────────────────────────────────────────

/// Why a metadata-search provider was skipped in the failover chain (§
/// "Refresh Metadata rate-limit messaging" — previously a 429 from any
/// provider silently looked identical to "no matches", with nothing
/// distinguishing it from a genuine empty result for the user).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSearchFailure {
    RateLimited,
    Unavailable,
}

/// One provider's failure during a metadata search, surfaced to the client
/// alongside whatever results the rest of the chain did find.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderSearchWarning {
    pub provider: String,
    pub reason: ProviderSearchFailure,
}

/// Public Metadata Search Result data shape used by BoogieBox.
#[derive(Debug, serde::Serialize)]
pub struct MetadataSearchResult {
    /// Documents the Provider public API surface.
    pub provider: String,
    /// Documents the Title public API surface.
    pub title: Option<String>,
    /// Documents the Artist public API surface.
    pub artist: Option<String>,
    /// Documents the Year public API surface.
    pub year: Option<String>,
    /// The client reads `releaseType`.
    #[serde(rename = "releaseType")]
    pub release_type: Option<String>,
    /// Documents the Genre public API surface.
    pub genre: Option<String>,
    /// The client reads `image`.
    #[serde(rename = "image")]
    pub cover_url: Option<String>,
    /// Tells namesakes apart (MusicBrainz disambiguation and area).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disambiguation: Option<String>,
    /// Documents the Tracklist public API surface.
    pub tracklist: Option<Vec<Value>>,
    /// Documents the Description public API surface.
    pub description: Option<String>,
    /// Documents the Extra public API surface.
    pub extra: Option<Value>,
}

/// Combines every configured provider's results without stopping early —
/// used by `post_scan::run_sync_discogs_album_metadata`'s cross-provider
/// agreement check (label/release-type/year are only applied when every
/// matched provider agrees), which needs to see all of them, not just
/// whichever answered first. Deliberately Last.fm-free and behaviorally
/// identical to the pre-failover `search_metadata`, since that job's
/// agreement logic wasn't part of this change.
pub async fn search_metadata_combined(
    client: &Client,
    discogs_token: Option<&str>,
    spotify_client_id: Option<&str>,
    spotify_client_secret: Option<&str>,
    artist: &str,
    album: Option<&str>,
) -> Vec<MetadataSearchResult> {
    let mut results = Vec::new();

    if let Some(token) = discogs_token {
        if let Some(album_title) = album {
            if let Ok(discogs_results) =
                search_discogs_metadata(client, token, artist, album_title).await
            {
                results.extend(discogs_results);
            }
        }
    }

    if let (Some(id), Some(secret)) = (spotify_client_id, spotify_client_secret) {
        if let Ok(spotify_results) =
            search_spotify_metadata(client, id, secret, artist, album).await
        {
            results.extend(spotify_results);
        }
    }

    results
}

/// Searches for artist/album metadata as a failover chain — MusicBrainz
/// first (keyless, several candidates), then Discogs (album searches only;
/// Discogs has no artist-only search wired up here), then Spotify, then
/// Last.fm while its key still works — stopping at the first provider that
/// returns at least one match. A provider that errors (rate limit or
/// otherwise) doesn't stop the chain, but its failure is recorded and
/// returned alongside whatever the rest of the chain found, so the client
/// can tell "genuinely no matches" apart from "a provider failed".
// Back to 7 arguments when the Last.fm key goes.
#[allow(clippy::too_many_arguments)]
pub async fn search_metadata(
    client: &Client,
    musicbrainz: bool,
    lastfm_key: Option<&str>,
    discogs_token: Option<&str>,
    spotify_client_id: Option<&str>,
    spotify_client_secret: Option<&str>,
    artist: &str,
    album: Option<&str>,
) -> (Vec<MetadataSearchResult>, Vec<ProviderSearchWarning>) {
    let mut warnings = Vec::new();

    if musicbrainz {
        match search_musicbrainz_metadata(client, artist, album).await {
            Ok(results) if !results.is_empty() => return (results, warnings),
            Ok(_) => {}
            Err(reason) => warnings.push(ProviderSearchWarning {
                provider: "musicbrainz".to_owned(),
                reason,
            }),
        }
    }

    if let Some(token) = discogs_token {
        if let Some(album_title) = album {
            match search_discogs_metadata(client, token, artist, album_title).await {
                Ok(results) if !results.is_empty() => return (results, warnings),
                Ok(_) => {}
                Err(reason) => warnings.push(ProviderSearchWarning {
                    provider: "discogs".to_owned(),
                    reason,
                }),
            }
        }
    }

    if let (Some(id), Some(secret)) = (spotify_client_id, spotify_client_secret) {
        match search_spotify_metadata(client, id, secret, artist, album).await {
            Ok(results) if !results.is_empty() => return (results, warnings),
            Ok(_) => {}
            Err(reason) => warnings.push(ProviderSearchWarning {
                provider: "spotify".to_owned(),
                reason,
            }),
        }
    }

    if let Some(key) = lastfm_key.filter(|key| lastfm_key_usable(key)) {
        match search_lastfm_metadata(client, key, artist, album).await {
            Ok(results) => return (results, warnings),
            Err(reason) => warnings.push(ProviderSearchWarning {
                provider: "lastfm".to_owned(),
                reason,
            }),
        }
    }

    (Vec::new(), warnings)
}

/// Search words with Lucene syntax and operators removed, lowercased so a title word like "AND" stays a word.
fn lucene_terms(text: &str) -> String {
    text.chars()
        .map(|c| {
            if matches!(
                c,
                '+' | '-'
                    | '&'
                    | '|'
                    | '!'
                    | '('
                    | ')'
                    | '{'
                    | '}'
                    | '['
                    | ']'
                    | '^'
                    | '"'
                    | '~'
                    | '*'
                    | '?'
                    | ':'
                    | '\\'
                    | '/'
            ) {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Lowest MusicBrainz search score a Refresh Metadata candidate may have.
const MUSICBRAINZ_MIN_CANDIDATE_SCORE: i64 = 50;

/// Cover Art Archive front image for a release group (the browser loads it; 404 when there is none).
fn cover_art_archive_front(release_group_id: &str) -> String {
    format!("https://coverartarchive.org/release-group/{release_group_id}/front-250")
}

/// The most voted tag of a MusicBrainz search hit.
fn musicbrainz_top_tag(item: &Value) -> Option<String> {
    item["tags"]
        .as_array()?
        .iter()
        .filter_map(|tag| Some((tag["count"].as_i64().unwrap_or(0), tag["name"].as_str()?)))
        .max_by_key(|(count, _)| *count)
        .map(|(_, name)| name.to_owned())
}

/// Joins the non-empty parts that tell namesakes apart.
fn musicbrainz_disambiguation(parts: &[Option<&str>]) -> Option<String> {
    let joined = parts
        .iter()
        .flatten()
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    (!joined.is_empty()).then_some(joined)
}

/// Album, single, EP or compilation from a release group's primary and secondary types.
fn musicbrainz_release_type(item: &Value) -> Option<String> {
    let compilation = item["secondary-types"]
        .as_array()
        .is_some_and(|types| types.iter().any(|t| t.as_str() == Some("Compilation")));
    if compilation {
        return Some("compilation".to_owned());
    }
    item["primary-type"]
        .as_str()
        .map(str::to_ascii_lowercase)
        .filter(|kind| matches!(kind.as_str(), "album" | "single" | "ep"))
}

/// Refresh Metadata candidates from MusicBrainz: up to five artists, or
/// release groups when `album` is given, best search score first.
async fn search_musicbrainz_metadata(
    client: &Client,
    artist: &str,
    album: Option<&str>,
) -> Result<Vec<MetadataSearchResult>, ProviderSearchFailure> {
    let artist_name: String = artist
        .chars()
        .filter(|c| !matches!(c, '"' | '\\'))
        .collect::<String>()
        .trim()
        .to_owned();
    let artist_terms = lucene_terms(artist);
    let album_terms = album.map(lucene_terms).filter(|terms| !terms.is_empty());
    if artist_name.is_empty() || artist_terms.is_empty() {
        return Ok(Vec::new());
    }
    // Loose terms, not an exact phrase: tag titles carry extras ("Ep", "[Bonus Tracks]",
    // "Pres.") that a phrase never matches. On 20 library albums: 19 found vs 10.
    let (entity, query, subject) = match (&album_terms, album) {
        (Some(terms), Some(title)) => (
            "release-group",
            format!("releasegroup:({terms}) AND artist:\"{artist_name}\""),
            format!("{artist_name} — {title}"),
        ),
        _ => (
            "artist",
            format!("artist:({artist_terms})"),
            artist_name.clone(),
        ),
    };
    let album_title = album_terms;
    let url = format!(
        "{}/{entity}/?query={}&fmt=json&limit=5",
        musicbrainz_api_base(),
        urlencoding::encode(&query)
    );
    let t0 = log_request("musicbrainz", "metadata_search", &subject);
    let response = client
        .get(&url)
        .header("User-Agent", MUSICBRAINZ_USER_AGENT)
        .timeout(PROVIDER_TIMEOUT)
        .send()
        .await
        .map_err(|_| ProviderSearchFailure::Unavailable)?;
    log_response("musicbrainz", "metadata_search", t0, response.status());
    check_metabrainz_status(&response).map_err(|error| match error {
        ProviderFetchError::RateLimited => ProviderSearchFailure::RateLimited,
        _ => ProviderSearchFailure::Unavailable,
    })?;
    let data: Value = response
        .json()
        .await
        .map_err(|_| ProviderSearchFailure::Unavailable)?;
    let key = if album_title.is_some() {
        "release-groups"
    } else {
        "artists"
    };
    let hits = data[key]
        .as_array()
        .ok_or(ProviderSearchFailure::Unavailable)?;
    let confident = hits.iter().filter(|item| {
        item["score"].as_i64().unwrap_or(0) >= MUSICBRAINZ_MIN_CANDIDATE_SCORE
            && item["id"].as_str().is_some_and(is_valid_mbid)
    });
    let results = if album_title.is_some() {
        confident
            .map(|item| {
                let credit = item["artist-credit"].as_array().map(|credits| {
                    credits
                        .iter()
                        .map(|c| {
                            format!(
                                "{}{}",
                                c["name"]
                                    .as_str()
                                    .or_else(|| c["artist"]["name"].as_str())
                                    .unwrap_or(""),
                                c["joinphrase"].as_str().unwrap_or("")
                            )
                        })
                        .collect::<String>()
                });
                MetadataSearchResult {
                    provider: "musicbrainz".to_owned(),
                    title: optional_nonempty_json_string(&item["title"]),
                    artist: credit
                        .map(|c| c.trim().to_owned())
                        .filter(|c| !c.is_empty()),
                    year: item["first-release-date"]
                        .as_str()
                        .and_then(|date| date.get(..4))
                        .map(str::to_owned),
                    release_type: musicbrainz_release_type(item),
                    genre: musicbrainz_top_tag(item),
                    cover_url: item["id"].as_str().map(cover_art_archive_front),
                    disambiguation: musicbrainz_disambiguation(&[item["disambiguation"].as_str()]),
                    tracklist: None,
                    description: None,
                    extra: Some(item.clone()),
                }
            })
            .collect()
    } else {
        confident
            .map(|item| MetadataSearchResult {
                provider: "musicbrainz".to_owned(),
                title: optional_nonempty_json_string(&item["name"]),
                artist: optional_nonempty_json_string(&item["name"]),
                year: item["life-span"]["begin"]
                    .as_str()
                    .and_then(|date| date.get(..4))
                    .map(str::to_owned),
                release_type: None,
                genre: musicbrainz_top_tag(item),
                cover_url: None,
                disambiguation: musicbrainz_disambiguation(&[
                    item["disambiguation"].as_str(),
                    item["area"]["name"].as_str(),
                ]),
                tracklist: None,
                description: None,
                extra: Some(item.clone()),
            })
            .collect()
    };
    Ok(results)
}

/// One-result-or-none Last.fm lookup (`artist.getinfo`/`album.getinfo` are
/// direct-lookup APIs, not a fuzzy multi-candidate search like Discogs/
/// Spotify) mapped onto the shared `MetadataSearchResult` shape so it can
/// slot into the same failover chain and the same "pick a result" UI.
async fn search_lastfm_metadata(
    client: &Client,
    api_key: &str,
    artist: &str,
    album: Option<&str>,
) -> Result<Vec<MetadataSearchResult>, ProviderSearchFailure> {
    let lastfm_api_root = lastfm_api_base();
    let (url, log_kind, log_subject) = if let Some(album_name) = album {
        (
            format!(
                "{lastfm_api_root}?method=album.getinfo&artist={}&album={}&api_key={}&format=json",
                urlencoding::encode(artist),
                urlencoding::encode(album_name),
                api_key
            ),
            "album_metadata_search",
            format!("{artist} — {album_name}"),
        )
    } else {
        (
            format!(
                "{lastfm_api_root}?method=artist.getinfo&artist={}&api_key={}&format=json&autocorrect=1",
                urlencoding::encode(artist),
                api_key
            ),
            "artist_metadata_search",
            artist.to_owned(),
        )
    };

    let t0 = log_request("lastfm", log_kind, &log_subject);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|_| ProviderSearchFailure::Unavailable)?;
    log_response("lastfm", log_kind, t0, resp.status());

    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(ProviderSearchFailure::RateLimited);
    }
    let (status, body, rejected) = read_lastfm_reply(api_key, resp).await;
    if rejected || !(200..300).contains(&status) {
        return Err(ProviderSearchFailure::Unavailable);
    }
    let data = body.map_err(|_| ProviderSearchFailure::Unavailable)?;

    if let Some(album_name) = album {
        let node = &data["album"];
        if node.is_null() {
            return Ok(Vec::new());
        }
        let summary = node["wiki"]["summary"].as_str().unwrap_or("");
        let image = node["image"].as_array().and_then(|imgs| {
            imgs.iter()
                .rev()
                .find_map(|i| optional_nonempty_json_string(&i["#text"]))
        });
        let genre = node["tags"]["tag"]
            .as_array()
            .and_then(|tags| tags.first())
            .and_then(|tag| optional_nonempty_json_string(&tag["name"]));
        Ok(vec![MetadataSearchResult {
            provider: "lastfm".to_owned(),
            title: Some(album_name.to_owned()),
            artist: optional_nonempty_json_string(&node["artist"])
                .or_else(|| Some(artist.to_owned())),
            year: None,
            release_type: None,
            genre,
            cover_url: image,
            disambiguation: None,
            tracklist: None,
            description: if summary.is_empty() {
                None
            } else {
                Some(summary.to_owned())
            },
            extra: None,
        }])
    } else {
        let node = &data["artist"];
        if node.is_null() || node["name"].as_str().is_none() {
            return Ok(Vec::new());
        }
        let summary = node["bio"]["summary"].as_str().unwrap_or("");
        let image = node["image"].as_array().and_then(|imgs| {
            imgs.iter()
                .rev()
                .find_map(|i| optional_nonempty_json_string(&i["#text"]))
        });
        let genre = node["tags"]["tag"]
            .as_array()
            .and_then(|tags| tags.first())
            .and_then(|tag| optional_nonempty_json_string(&tag["name"]));
        Ok(vec![MetadataSearchResult {
            provider: "lastfm".to_owned(),
            title: optional_nonempty_json_string(&node["name"]).or_else(|| Some(artist.to_owned())),
            artist: Some(artist.to_owned()),
            year: None,
            release_type: None,
            genre,
            cover_url: image,
            disambiguation: None,
            tracklist: None,
            description: if summary.is_empty() {
                None
            } else {
                Some(summary.to_owned())
            },
            extra: None,
        }])
    }
}

async fn search_discogs_metadata(
    client: &Client,
    token: &str,
    artist: &str,
    album: &str,
) -> Result<Vec<MetadataSearchResult>, ProviderSearchFailure> {
    let q = format!("{} {}", artist, album);
    let url = format!(
        "{}/database/search?type=release&q={}&per_page=5&page=1",
        discogs_api_base(),
        urlencoding::encode(q.trim())
    );

    let t0 = log_request("discogs", "metadata_search", &q);
    let resp = client
        .get(&url)
        .header("Authorization", format!("Discogs token={token}"))
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|_| ProviderSearchFailure::Unavailable)?;
    log_response("discogs", "metadata_search", t0, resp.status());

    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(ProviderSearchFailure::RateLimited);
    }
    if !resp.status().is_success() {
        return Err(ProviderSearchFailure::Unavailable);
    }

    let data: Value = resp
        .json()
        .await
        .map_err(|_| ProviderSearchFailure::Unavailable)?;
    let items = data["results"]
        .as_array()
        .ok_or(ProviderSearchFailure::Unavailable)?;
    tracing::debug!(
        "provider result discogs metadata_search query={q:?} hits={}",
        items.len()
    );

    let results = items
        .iter()
        .map(|item| {
            let raw_title = item["title"].as_str().unwrap_or("");
            let (result_artist, result_title) = if raw_title.contains(" - ") {
                let parts: Vec<&str> = raw_title.splitn(2, " - ").collect();
                (
                    Some(parts[0].trim().to_owned()),
                    Some(parts.get(1).copied().unwrap_or("").trim().to_owned()),
                )
            } else {
                (None, Some(raw_title.trim().to_owned()))
            };

            let cover_url = item["cover_image"]
                .as_str()
                .filter(|c| {
                    !c.is_empty()
                        && !c.contains("spacer")
                        && !c.starts_with("https://st.discogs.com")
                })
                .map(str::to_owned);

            MetadataSearchResult {
                provider: "discogs".to_owned(),
                title: result_title,
                artist: result_artist,
                year: item["year"].as_str().map(str::to_owned),
                release_type: item["format"]
                    .as_array()
                    .and_then(|f| discogs_release_type_from_formats(f)),
                genre: item["genre"]
                    .as_array()
                    .and_then(|g| g.first())
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                cover_url,
                disambiguation: None,
                tracklist: None,
                description: None,
                extra: Some(item.clone()),
            }
        })
        .collect();

    Ok(results)
}

async fn search_spotify_metadata(
    client: &Client,
    client_id: &str,
    client_secret: &str,
    artist: &str,
    album: Option<&str>,
) -> Result<Vec<MetadataSearchResult>, ProviderSearchFailure> {
    let token = get_spotify_access_token(client, client_id, client_secret)
        .await
        .ok_or(ProviderSearchFailure::Unavailable)?;

    let q = if let Some(al) = album {
        format!("artist:{} album:{}", artist, al)
    } else {
        format!("artist:{}", artist)
    };

    let search_type = if album.is_some() { "album" } else { "artist" };
    let url = format!(
        "{}/v1/search?type={search_type}&limit=5&q={}",
        spotify_api_base(),
        urlencoding::encode(&q)
    );

    let t0 = log_request("spotify", "metadata_search", &q);
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|_| ProviderSearchFailure::Unavailable)?;
    log_response("spotify", "metadata_search", t0, resp.status());

    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(ProviderSearchFailure::RateLimited);
    }
    if !resp.status().is_success() {
        return Err(ProviderSearchFailure::Unavailable);
    }

    let data: Value = resp
        .json()
        .await
        .map_err(|_| ProviderSearchFailure::Unavailable)?;

    if album.is_some() {
        let items = data["albums"]["items"]
            .as_array()
            .ok_or(ProviderSearchFailure::Unavailable)?;
        let results = items
            .iter()
            .map(|item| {
                let cover_url = item["images"]
                    .as_array()
                    .and_then(|imgs| imgs.first())
                    .and_then(|img| img["url"].as_str())
                    .map(str::to_owned);

                MetadataSearchResult {
                    provider: "spotify".to_owned(),
                    title: item["name"].as_str().map(str::to_owned),
                    artist: item["artists"]
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(|a| a["name"].as_str())
                        .map(str::to_owned),
                    year: item["release_date"]
                        .as_str()
                        .map(|d| d.chars().take(4).collect()),
                    release_type: item["album_type"].as_str().map(str::to_owned),
                    genre: None,
                    cover_url,
                    disambiguation: None,
                    tracklist: None,
                    description: None,
                    extra: Some(item.clone()),
                }
            })
            .collect();
        Ok(results)
    } else {
        let items = data["artists"]["items"]
            .as_array()
            .ok_or(ProviderSearchFailure::Unavailable)?;
        let results = items
            .iter()
            .map(|item| {
                let cover_url = item["images"]
                    .as_array()
                    .and_then(|imgs| imgs.first())
                    .and_then(|img| img["url"].as_str())
                    .map(str::to_owned);

                MetadataSearchResult {
                    provider: "spotify".to_owned(),
                    title: item["name"].as_str().map(str::to_owned),
                    artist: item["name"].as_str().map(str::to_owned),
                    year: None,
                    release_type: None,
                    genre: item["genres"]
                        .as_array()
                        .and_then(|g| g.first())
                        .and_then(|v| v.as_str())
                        .map(str::to_owned),
                    cover_url,
                    disambiguation: None,
                    tracklist: None,
                    description: None,
                    extra: Some(item.clone()),
                }
            })
            .collect();
        Ok(results)
    }
}

// ── Image download ────────────────────────────────────────────────────────────

/// Documents the Download Image public API surface.
pub async fn download_image(client: &Client, url: &str) -> Option<(Vec<u8>, String)> {
    let t0 = log_request("image", "download", url);
    let resp = match client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
    {
        Ok(r) => r,
        Err(err) => {
            // `.ok()?` here would silently swallow the real cause (timeout,
            // TLS, DNS, connection refused) — log it so a provider-specific
            // download failure (e.g. Deezer failing while Discogs succeeds)
            // is diagnosable from server logs instead of a bare "not found".
            tracing::warn!("provider call ← image download failed url={url} error={err}");
            return None;
        }
    };
    log_response("image", "download", t0, resp.status());

    if !resp.status().is_success() {
        tracing::warn!(
            "provider call ← image download non-success url={url} status={}",
            resp.status()
        );
        return None;
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();

    let ext = if content_type.contains("image/png") {
        ".png"
    } else if content_type.contains("image/webp") {
        ".webp"
    } else {
        ".jpg"
    };

    let bytes = resp.bytes().await.ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some((bytes.to_vec(), ext.to_owned()))
}

// ── Lyrics providers ──────────────────────────────────────────────────────────

/// Public Lyrics Result data shape used by BoogieBox.
#[derive(Debug)]
pub struct LyricsResult {
    /// Documents the Lyrics public API surface.
    pub lyrics: String,
    /// Documents the Source public API surface.
    pub source: String,
    /// Documents the Synced public API surface.
    pub synced: Option<Vec<SyncedLine>>,
}

/// Public Synced Line data shape used by BoogieBox.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SyncedLine {
    /// Documents the Time public API surface.
    pub time: f64,
    /// Documents the Text public API surface.
    pub text: String,
}

fn clean_text(input: &str) -> String {
    let normalized = input.replace("\r\n", "\n");
    // Collapse 3+ newlines to 2
    let mut result = String::new();
    let mut newline_count = 0usize;
    for c in normalized.chars() {
        if c == '\n' {
            newline_count += 1;
            if newline_count <= 2 {
                result.push(c);
            }
        } else {
            newline_count = 0;
            result.push(c);
        }
    }
    result.trim().to_owned()
}

fn parse_synced_lyrics(raw: &str) -> Vec<SyncedLine> {
    let mut parsed = Vec::new();
    for line in raw.split('\n') {
        let line = line.trim();
        // Match [MM:SS.mmm] or [MM:SS]
        if !line.starts_with('[') {
            continue;
        }
        let close = match line.find(']') {
            Some(i) => i,
            None => continue,
        };
        let timestamp = &line[1..close];
        let text = line[close + 1..].trim().to_owned();
        if text.is_empty() {
            continue;
        }
        let parts: Vec<&str> = timestamp.splitn(2, ':').collect();
        if parts.len() != 2 {
            continue;
        }
        let min: f64 = parts[0].parse().unwrap_or(-1.0);
        let sec_str = parts[1];
        let (sec_int, ms_frac) = if let Some(dot) = sec_str.find('.') {
            let s: f64 = sec_str[..dot].parse().unwrap_or(-1.0);
            let ms_raw = &sec_str[dot + 1..];
            let ms: f64 = match ms_raw.len() {
                3 => ms_raw.parse::<f64>().unwrap_or(0.0),
                2 => ms_raw.parse::<f64>().unwrap_or(0.0) * 10.0,
                1 => ms_raw.parse::<f64>().unwrap_or(0.0) * 100.0,
                _ => 0.0,
            };
            (s, ms / 1000.0)
        } else {
            (sec_str.parse().unwrap_or(-1.0), 0.0)
        };
        if min < 0.0 || sec_int < 0.0 {
            continue;
        }
        parsed.push(SyncedLine {
            time: min * 60.0 + sec_int + ms_frac,
            text,
        });
    }
    parsed.sort_by(|a, b| {
        a.time
            .partial_cmp(&b.time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    parsed
}

/// Documents the Fetch Lrclib Lyrics public API surface.
pub async fn fetch_lrclib_lyrics(
    client: &Client,
    artist: &str,
    title: &str,
) -> Option<LyricsResult> {
    let url = format!(
        "{}/api/get?artist_name={}&track_name={}",
        lrclib_api_base(),
        urlencoding::encode(artist),
        urlencoding::encode(title)
    );

    let t0 = log_request("lrclib", "lyrics", &format!("{artist} — {title}"));
    let resp = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .ok()?;
    log_response("lrclib", "lyrics", t0, resp.status());

    if !resp.status().is_success() {
        return None;
    }

    let data: Value = resp.json().await.ok()?;
    let plain = data["plainLyrics"]
        .as_str()
        .map(clean_text)
        .filter(|s| !s.is_empty());
    let synced_raw = data["syncedLyrics"].as_str().unwrap_or("");
    let synced_lines = if !synced_raw.is_empty() {
        let lines = parse_synced_lyrics(synced_raw);
        if lines.is_empty() {
            None
        } else {
            Some(lines)
        }
    } else {
        None
    };

    if let Some(lyrics) = plain {
        return Some(LyricsResult {
            lyrics,
            source: "lrclib".to_owned(),
            synced: synced_lines,
        });
    }
    if let Some(ref synced) = synced_lines {
        let lyrics = synced
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        return Some(LyricsResult {
            lyrics,
            source: "lrclib".to_owned(),
            synced: synced_lines,
        });
    }
    None
}

/// Documents the Fetch Lyricsovh public API surface.
pub async fn fetch_lyricsovh(client: &Client, artist: &str, title: &str) -> Option<LyricsResult> {
    let url = format!(
        "{}/v1/{}/{}",
        lyrics_ovh_api_base(),
        urlencoding::encode(artist),
        urlencoding::encode(title)
    );

    let t0 = log_request("lyrics.ovh", "lyrics", &format!("{artist} — {title}"));
    let resp = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .ok()?;
    log_response("lyrics.ovh", "lyrics", t0, resp.status());

    if !resp.status().is_success() {
        return None;
    }

    let data: Value = resp.json().await.ok()?;
    let lyrics = data["lyrics"]
        .as_str()
        .map(clean_text)
        .filter(|s| !s.is_empty())?;

    Some(LyricsResult {
        lyrics,
        source: "lyrics.ovh".to_owned(),
        synced: None,
    })
}

// ── Last.fm ───────────────────────────────────────────────────────────────────

/// Public Last Fm Info Payload data shape used by BoogieBox.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LastFmInfoPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mbid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_name: Option<String>,
    /// Documents the Summary public API surface.
    pub summary: String,
    /// Documents the Full public API surface.
    pub full: String,
    /// Documents the Listeners public API surface.
    pub listeners: Option<String>,
    /// Documents the Playcount public API surface.
    pub playcount: Option<String>,
    /// Documents the Url public API surface.
    pub url: Option<String>,
    /// Documents the Image public API surface.
    pub image: Option<String>,
    /// Documents the Tags public API surface.
    pub tags: Vec<String>,
}

pub(crate) fn optional_nonempty_json_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn parse_lastfm_artist_info(data: &Value) -> Option<LastFmInfoPayload> {
    let artist = &data["artist"];
    if artist.is_null() || artist["name"].as_str().is_none() {
        return None;
    }
    let image = artist["image"].as_array().and_then(|images| {
        images
            .iter()
            .rev()
            .find_map(|image| optional_nonempty_json_string(&image["#text"]))
    });
    let tags = artist["tags"]["tag"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|tag| optional_nonempty_json_string(&tag["name"]))
                .collect()
        })
        .unwrap_or_default();
    Some(LastFmInfoPayload {
        mbid: optional_nonempty_json_string(&artist["mbid"]),
        canonical_name: optional_nonempty_json_string(&artist["name"]),
        summary: artist["bio"]["summary"].as_str().unwrap_or("").to_owned(),
        full: artist["bio"]["content"].as_str().unwrap_or("").to_owned(),
        listeners: artist["stats"]["listeners"].as_str().map(str::to_owned),
        playcount: artist["stats"]["playcount"].as_str().map(str::to_owned),
        url: optional_nonempty_json_string(&artist["url"]),
        image,
        tags,
    })
}

/// Documents the Fetch Lastfm Artist Info public API surface.
pub async fn fetch_lastfm_artist_info(
    client: &Client,
    api_key: &str,
    artist_name: &str,
) -> Option<LastFmInfoPayload> {
    let lastfm_api_root = lastfm_api_base();
    let url = format!(
        "{lastfm_api_root}?method=artist.getinfo&artist={}&api_key={}&format=json&autocorrect=1",
        urlencoding::encode(artist_name),
        api_key
    );
    if !lastfm_key_usable(api_key) {
        return None;
    }
    let t0 = log_request("lastfm", "artist_info", artist_name);
    let resp = client.get(&url).send().await.ok()?;
    log_response("lastfm", "artist_info", t0, resp.status());
    let (_, body, rejected) = read_lastfm_reply(api_key, resp).await;
    if rejected {
        return None;
    }
    parse_lastfm_artist_info(&body.ok()?)
}

fn parse_lastfm_similar_artists(data: &Value) -> Vec<RelatedArtistCandidate> {
    data["similarartists"]["artist"]
        .as_array()
        .map(|artists| {
            artists
                .iter()
                .enumerate()
                .filter_map(|(index, artist)| {
                    let name = optional_nonempty_json_string(&artist["name"])?;
                    let match_score = artist["match"]
                        .as_f64()
                        .or_else(|| artist["match"].as_str()?.parse::<f64>().ok())
                        .map(|score| score.clamp(0.0, 1.0));
                    let image_url = artist["image"].as_array().and_then(|images| {
                        images
                            .iter()
                            .rev()
                            .find_map(|image| optional_nonempty_json_string(&image["#text"]))
                    });
                    Some(RelatedArtistCandidate {
                        external_id: optional_nonempty_json_string(&artist["mbid"]),
                        name,
                        url: optional_nonempty_json_string(&artist["url"]),
                        image_url,
                        match_score,
                        rank: index + 1,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Fetches Last.fm's ranked similar-artist graph for a source artist.
pub async fn fetch_lastfm_similar_artists(
    client: &Client,
    api_key: &str,
    artist_name: &str,
    mbid: Option<&str>,
    limit: usize,
) -> Result<Vec<RelatedArtistCandidate>, String> {
    let identity = mbid
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("mbid={}", urlencoding::encode(value)))
        .unwrap_or_else(|| format!("artist={}", urlencoding::encode(artist_name.trim())));
    let lastfm_api_root = lastfm_api_base();
    let url = format!(
        "{lastfm_api_root}?method=artist.getSimilar&{identity}&api_key={}&format=json&autocorrect=1&limit={}",
        urlencoding::encode(api_key),
        limit.clamp(1, 100)
    );
    if !lastfm_key_usable(api_key) {
        return Err(LASTFM_KEY_REJECTED.to_owned());
    }
    let t0 = log_request("lastfm", "similar_artists", artist_name);
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    log_response("lastfm", "similar_artists", t0, response.status());
    let (status, body, rejected) = read_lastfm_reply(api_key, response).await;
    if rejected {
        return Err(LASTFM_KEY_REJECTED.to_owned());
    }
    if !(200..300).contains(&status) {
        return Err(format!("Last.fm returned {status}"));
    }
    let data = body?;
    if data["error"].is_number() {
        return Err(data["message"]
            .as_str()
            .unwrap_or("Last.fm request failed")
            .to_owned());
    }
    let candidates = parse_lastfm_similar_artists(&data);
    tracing::debug!(
        "provider result lastfm similar_artists artist={artist_name:?} hits={}",
        candidates.len()
    );
    Ok(candidates)
}

fn parse_deezer_related_artists(data: &Value) -> Vec<RelatedArtistCandidate> {
    data["data"]
        .as_array()
        .map(|artists| {
            artists
                .iter()
                .enumerate()
                .filter_map(|(index, artist)| {
                    let name = optional_nonempty_json_string(&artist["name"])?;
                    let external_id = artist["id"]
                        .as_str()
                        .map(str::to_owned)
                        .or_else(|| artist["id"].as_i64().map(|value| value.to_string()));
                    let image_url = artist["picture_xl"]
                        .as_str()
                        .or_else(|| artist["picture_big"].as_str())
                        .or_else(|| artist["picture_medium"].as_str())
                        .filter(|value| valid_artist_image(value))
                        .map(str::to_owned);
                    Some(RelatedArtistCandidate {
                        external_id,
                        name,
                        url: optional_nonempty_json_string(&artist["link"]),
                        image_url,
                        match_score: None,
                        rank: index + 1,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Fetches Deezer's ordered related-artist list.
pub async fn fetch_deezer_related_artists(
    client: &Client,
    artist_id: &str,
    limit: usize,
) -> Result<Vec<RelatedArtistCandidate>, String> {
    let url = format!(
        "{}/artist/{}/related?limit={}",
        deezer_api_base(),
        urlencoding::encode(artist_id.trim()),
        limit.clamp(1, 100)
    );
    let t0 = log_request("deezer", "related_artists", artist_id);
    let response = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    log_response("deezer", "related_artists", t0, response.status());
    if !response.status().is_success() {
        return Err(format!("Deezer returned {}", response.status().as_u16()));
    }
    let data: Value = response.json().await.map_err(|error| error.to_string())?;
    if data["error"].is_object() {
        return Err("Deezer request failed".to_owned());
    }
    let candidates = parse_deezer_related_artists(&data);
    tracing::debug!(
        "provider result deezer related_artists artist_id={artist_id:?} hits={}",
        candidates.len()
    );
    Ok(candidates)
}

/// Documents the Fetch Lastfm Album Info public API surface.
pub async fn fetch_lastfm_album_info(
    client: &Client,
    api_key: &str,
    artist_name: &str,
    album_name: &str,
) -> Option<LastFmInfoPayload> {
    let lastfm_api_root = lastfm_api_base();
    let url = format!(
        "{lastfm_api_root}?method=album.getinfo&artist={}&album={}&api_key={}&format=json",
        urlencoding::encode(artist_name),
        urlencoding::encode(album_name),
        api_key
    );
    let t0 = log_request(
        "lastfm",
        "album_info",
        &format!("{artist_name} — {album_name}"),
    );
    if !lastfm_key_usable(api_key) {
        return None;
    }
    let resp = client.get(&url).send().await.ok()?;
    log_response("lastfm", "album_info", t0, resp.status());
    let (_, body, rejected) = read_lastfm_reply(api_key, resp).await;
    if rejected {
        return None;
    }
    let data = body.ok()?;
    let album = &data["album"];
    if album.is_null() {
        return None;
    }
    let summary = album["wiki"]["summary"].as_str().unwrap_or("").to_owned();
    let full = album["wiki"]["content"].as_str().unwrap_or("").to_owned();
    let listeners = album["listeners"].as_str().map(str::to_owned);
    let playcount = album["playcount"].as_str().map(str::to_owned);
    let url = album["url"].as_str().map(str::to_owned);
    let image = album["image"]
        .as_array()
        .and_then(|imgs| {
            imgs.iter()
                .rev()
                .find(|i| !i["#text"].as_str().unwrap_or("").is_empty())
        })
        .and_then(|i| i["#text"].as_str())
        .map(str::to_owned);
    let tags = album["tags"]["tag"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Some(LastFmInfoPayload {
        mbid: None,
        canonical_name: None,
        summary,
        full,
        listeners,
        playcount,
        url,
        image,
        tags,
    })
}

/// Documents the Fetch Lastfm Artist Top Tags public API surface.
pub async fn fetch_lastfm_artist_top_tags(
    client: &Client,
    api_key: &str,
    artist_name: &str,
) -> Vec<(String, u64)> {
    let lastfm_api_root = lastfm_api_base();
    let url = format!(
        "{lastfm_api_root}?method=artist.gettoptags&artist={}&api_key={}&format=json",
        urlencoding::encode(artist_name),
        api_key
    );
    if !lastfm_key_usable(api_key) {
        return vec![];
    }
    let t0 = log_request("lastfm", "artist_top_tags", artist_name);
    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    log_response("lastfm", "artist_top_tags", t0, resp.status());
    let data: Value = match read_lastfm_reply(api_key, resp).await {
        (_, Ok(v), false) => v,
        _ => return vec![],
    };
    data["toptags"]["tag"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    let name = t["name"].as_str()?.to_owned();
                    let count = t["count"].as_u64().unwrap_or(0);
                    Some((name, count))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Documents the Fetch Lastfm Top Tracks public API surface.
pub async fn fetch_lastfm_top_tracks(
    client: &Client,
    api_key: &str,
    artist_name: &str,
) -> Result<Vec<Value>, String> {
    let lastfm_api_root = lastfm_api_base();
    let url = format!(
        "{lastfm_api_root}?method=artist.getTopTracks&api_key={}&artist={}&format=json&autocorrect=1&limit=10",
        api_key,
        urlencoding::encode(artist_name),
    );
    if !lastfm_key_usable(api_key) {
        return Err(LASTFM_KEY_REJECTED.to_owned());
    }
    let t0 = log_request("lastfm", "top_tracks", artist_name);
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    log_response("lastfm", "top_tracks", t0, resp.status());
    let (_, body, rejected) = read_lastfm_reply(api_key, resp).await;
    if rejected {
        return Err(LASTFM_KEY_REJECTED.to_owned());
    }
    let data = body?;
    if data["error"].is_number() {
        return Err(data["message"].as_str().unwrap_or("Not found").to_owned());
    }
    let raw = data["toptracks"]["track"]
        .as_array()
        .ok_or_else(|| "No tracks found".to_owned())?;
    let mut tracks: Vec<Value> = raw
        .iter()
        .filter_map(|t| {
            let name = t["name"].as_str()?.trim().to_owned();
            if name.is_empty() {
                return None;
            }
            let playcount: u64 = t["playcount"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if playcount == 0 {
                return None;
            }
            let mut obj = serde_json::json!({ "name": name, "playcount": playcount });
            if let Some(n) = t["listeners"].as_str().and_then(|s| s.parse::<u64>().ok()) {
                obj["listeners"] = serde_json::json!(n);
            }
            if let Some(u) = t["url"].as_str() {
                obj["url"] = serde_json::json!(u);
            }
            Some(obj)
        })
        .collect();
    if tracks.is_empty() {
        return Err("No tracks found".to_owned());
    }
    tracks.sort_by(|a, b| {
        b["playcount"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&a["playcount"].as_u64().unwrap_or(0))
    });
    tracks.truncate(5);
    Ok(tracks)
}

// -- Artist Radio metadata (wip/artist-radio-v2-plan.md §3) ---------------------

/// Why a tag/similarity lookup produced nothing. `RateLimited` must stay
/// distinguishable from "the provider has no data" so callers can back off
/// instead of negative-caching a miss that was really a throttle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderFetchError {
    RateLimited,
    /// Last.fm refuses the API key (see [`lastfm_key_usable`]): stop, record nothing.
    KeyRejected,
    Failed(String),
}

impl std::fmt::Display for ProviderFetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderFetchError::RateLimited => write!(f, "provider rate limit exceeded"),
            ProviderFetchError::KeyRejected => write!(f, "Last.fm refused the API key"),
            ProviderFetchError::Failed(message) => write!(f, "{message}"),
        }
    }
}

const PROVIDER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// MusicBrainz requires a descriptive User-Agent with contact information.
pub(crate) const MUSICBRAINZ_USER_AGENT: &str = concat!(
    "BoogieBox/",
    env!("CARGO_PKG_VERSION"),
    " ( https://github.com/yossironnen/BoogieBox )"
);

/// ListenBrainz Labs similar-artists algorithm (session-based, 9000 days).
const LISTENBRAINZ_SIMILAR_ALGORITHM: &str =
    "session_based_days_9000_session_300_contribution_5_threshold_15_limit_50_skip_30";

pub(crate) fn musicbrainz_api_base() -> String {
    provider_base_url(
        "BOOGIEBOX_MUSICBRAINZ_API_BASE",
        "https://musicbrainz.org/ws/2",
    )
}

pub(crate) fn listenbrainz_api_base() -> String {
    provider_base_url(
        "BOOGIEBOX_LISTENBRAINZ_API_BASE",
        "https://api.listenbrainz.org",
    )
}

/// One of an artist's most played songs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopSong {
    pub name: String,
    /// ListenBrainz listen count (Deezer gives an order, not a count).
    pub listens: Option<u64>,
    #[serde(skip)]
    pub recording_mbid: Option<String>,
    /// The song's page: the MusicBrainz recording, or the Deezer track.
    pub url: Option<String>,
}

/// An artist's songs by ListenBrainz listen count, most listened first (all ListenBrainz has, no limit).
pub async fn fetch_listenbrainz_top_recordings(
    client: &Client,
    artist_mbid: &str,
) -> Result<Vec<TopSong>, ProviderFetchError> {
    if !is_valid_mbid(artist_mbid) {
        return Err(ProviderFetchError::Failed(
            "invalid MusicBrainz id".to_owned(),
        ));
    }
    let url = format!(
        "{}/1/popularity/top-recordings-for-artist/{artist_mbid}",
        listenbrainz_api_base()
    );
    let t0 = log_request("listenbrainz", "top_recordings", artist_mbid);
    let response = client
        .get(&url)
        .header("User-Agent", MUSICBRAINZ_USER_AGENT)
        .timeout(PROVIDER_TIMEOUT)
        .send()
        .await
        .map_err(|error| ProviderFetchError::Failed(error.to_string()))?;
    log_response("listenbrainz", "top_recordings", t0, response.status());
    check_metabrainz_status(&response)?;
    let data: Value = response
        .json()
        .await
        .map_err(|error| ProviderFetchError::Failed(error.to_string()))?;
    let mut songs: Vec<TopSong> = data
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let name = optional_nonempty_json_string(&row["recording_name"])?;
                    let listens = row["total_listen_count"].as_u64().filter(|n| *n > 0)?;
                    let mbid = optional_nonempty_json_string(&row["recording_mbid"])
                        .filter(|id| is_valid_mbid(id))
                        .map(|id| id.to_ascii_lowercase());
                    Some(TopSong {
                        url: mbid
                            .as_deref()
                            .map(|id| format!("https://musicbrainz.org/recording/{id}")),
                        name,
                        listens: Some(listens),
                        recording_mbid: mbid,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    songs.sort_by_key(|song| std::cmp::Reverse(song.listens));
    Ok(songs)
}

/// Deezer's top songs for an artist, in Deezer's order.
pub async fn fetch_deezer_artist_top(
    client: &Client,
    deezer_artist_id: &str,
    limit: usize,
) -> Result<Vec<TopSong>, ProviderFetchError> {
    let url = format!(
        "{}/artist/{}/top?limit={}",
        deezer_api_base(),
        urlencoding::encode(deezer_artist_id.trim()),
        limit.clamp(1, 100)
    );
    let t0 = log_request("deezer", "artist_top", deezer_artist_id);
    let response = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .timeout(PROVIDER_TIMEOUT)
        .send()
        .await
        .map_err(|error| ProviderFetchError::Failed(error.to_string()))?;
    log_response("deezer", "artist_top", t0, response.status());
    if response.status().as_u16() == 429 {
        return Err(ProviderFetchError::RateLimited);
    }
    if !response.status().is_success() {
        return Err(ProviderFetchError::Failed(format!(
            "Deezer returned {}",
            response.status().as_u16()
        )));
    }
    let data: Value = response
        .json()
        .await
        .map_err(|error| ProviderFetchError::Failed(error.to_string()))?;
    if data["error"].is_object() {
        // Deezer's code 4 is its quota.
        return Err(if data["error"]["code"].as_i64() == Some(4) {
            ProviderFetchError::RateLimited
        } else {
            ProviderFetchError::Failed("Deezer request failed".to_owned())
        });
    }
    Ok(data["data"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    Some(TopSong {
                        name: optional_nonempty_json_string(&row["title"])?,
                        listens: None,
                        recording_mbid: None,
                        url: optional_nonempty_json_string(&row["link"]),
                    })
                })
                .take(limit)
                .collect()
        })
        .unwrap_or_default())
}

fn listenbrainz_labs_api_base() -> String {
    provider_base_url(
        "BOOGIEBOX_LISTENBRAINZ_LABS_API_BASE",
        "https://labs.api.listenbrainz.org",
    )
}

/// MusicBrainz IDs are UUIDs; reject anything else before it reaches a URL path.
pub fn is_valid_mbid(value: &str) -> bool {
    value.len() == 36 && value.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn json_count(value: &Value) -> u64 {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
        .unwrap_or(0)
}

fn parse_tag_counts(tags: &Value) -> Vec<(String, u64)> {
    tags.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    let name = t["name"].as_str()?.trim().to_owned();
                    (!name.is_empty()).then(|| (name, json_count(&t["count"])))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Fetches Last.fm's crowd tags for one track (`track.getTopTags`).
///
/// Unlike [`fetch_lastfm_artist_top_tags`] this returns a `Result` so a rate
/// limit (HTTP 429 or Last.fm error 29) is never confused with a track that
/// simply has no tags (error 6 / an empty list → `Ok(vec![])`).
pub async fn fetch_lastfm_track_top_tags(
    client: &Client,
    api_key: &str,
    artist_name: &str,
    track_title: &str,
) -> Result<Vec<(String, u64)>, ProviderFetchError> {
    let lastfm_api_root = lastfm_api_base();
    let url = format!(
        "{lastfm_api_root}?method=track.gettoptags&artist={}&track={}&api_key={}&format=json&autocorrect=1",
        urlencoding::encode(artist_name),
        urlencoding::encode(track_title),
        urlencoding::encode(api_key),
    );
    if !lastfm_key_usable(api_key) {
        return Err(ProviderFetchError::KeyRejected);
    }
    let t0 = log_request(
        "lastfm",
        "track_top_tags",
        &format!("{artist_name} — {track_title}"),
    );
    let response = client
        .get(&url)
        .timeout(PROVIDER_TIMEOUT)
        .send()
        .await
        .map_err(|error| ProviderFetchError::Failed(error.to_string()))?;
    log_response("lastfm", "track_top_tags", t0, response.status());
    if response.status().as_u16() == 429 {
        return Err(ProviderFetchError::RateLimited);
    }
    let (status, body, rejected) = read_lastfm_reply(api_key, response).await;
    if rejected {
        return Err(ProviderFetchError::KeyRejected);
    }
    if !(200..300).contains(&status) {
        return Err(ProviderFetchError::Failed(format!(
            "Last.fm returned {status}"
        )));
    }
    let data = body.map_err(ProviderFetchError::Failed)?;
    if let Some(code) = data["error"].as_i64() {
        return match code {
            29 => Err(ProviderFetchError::RateLimited),
            6 => Ok(Vec::new()),
            _ => Err(ProviderFetchError::Failed(
                data["message"]
                    .as_str()
                    .unwrap_or("Last.fm request failed")
                    .to_owned(),
            )),
        };
    }
    Ok(parse_tag_counts(&data["toptags"]["tag"]))
}

/// Most songs asked for per artist; on a sample of the dev library this covered 99% of its tracks.
const LASTFM_TRACK_COUNTS_LIMIT: u32 = 1000;

/// One artist's songs on Last.fm with their global scrobble counts. An artist Last.fm doesn't know is an empty
/// list; a throttle stays an error so it's never recorded as "no data".
pub async fn fetch_lastfm_artist_track_counts(
    client: &Client,
    api_key: &str,
    artist_name: &str,
) -> Result<Vec<boogiebox_db::popularity::PopularityEntry>, ProviderFetchError> {
    let lastfm_api_root = lastfm_api_base();
    let url = format!(
        "{lastfm_api_root}?method=artist.gettoptracks&artist={}&api_key={}&format=json&autocorrect=1&limit={LASTFM_TRACK_COUNTS_LIMIT}",
        urlencoding::encode(artist_name),
        urlencoding::encode(api_key),
    );
    if !lastfm_key_usable(api_key) {
        return Err(ProviderFetchError::KeyRejected);
    }
    let t0 = log_request("lastfm", "artist_track_counts", artist_name);
    let response = client
        .get(&url)
        .timeout(PROVIDER_TIMEOUT)
        .send()
        .await
        .map_err(|error| ProviderFetchError::Failed(error.to_string()))?;
    log_response("lastfm", "artist_track_counts", t0, response.status());
    if response.status().as_u16() == 429 {
        return Err(ProviderFetchError::RateLimited);
    }
    let (status, body, rejected) = read_lastfm_reply(api_key, response).await;
    if rejected {
        return Err(ProviderFetchError::KeyRejected);
    }
    if !(200..300).contains(&status) {
        return Err(ProviderFetchError::Failed(format!(
            "Last.fm returned {status}"
        )));
    }
    let data = body.map_err(ProviderFetchError::Failed)?;
    if let Some(code) = data["error"].as_i64() {
        return match code {
            29 => Err(ProviderFetchError::RateLimited),
            6 => Ok(Vec::new()),
            _ => Err(ProviderFetchError::Failed(
                data["message"]
                    .as_str()
                    .unwrap_or("Last.fm request failed")
                    .to_owned(),
            )),
        };
    }
    let count = |value: &Value| -> Option<u64> {
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
    };
    let track = &data["toptracks"]["track"];
    // Last.fm sends a lone object instead of a one-item list.
    let tracks: &[Value] = match track {
        Value::Array(list) => list,
        Value::Object(_) => std::slice::from_ref(track),
        _ => &[],
    };
    Ok(tracks
        .iter()
        .filter_map(|entry| {
            let title = entry["name"].as_str()?.trim();
            let scrobbles = count(&entry["playcount"])?;
            (!title.is_empty() && scrobbles > 0).then(|| {
                boogiebox_db::popularity::PopularityEntry {
                    recording_mbid: None,
                    title: title.to_owned(),
                    scrobbles,
                    listeners: count(&entry["listeners"]),
                }
            })
        })
        .collect())
}

/// MusicBrainz and ListenBrainz signal throttling with HTTP 429/503.
pub(crate) fn check_metabrainz_status(
    response: &reqwest::Response,
) -> Result<(), ProviderFetchError> {
    match response.status().as_u16() {
        429 | 503 => Err(ProviderFetchError::RateLimited),
        code if !response.status().is_success() => {
            Err(ProviderFetchError::Failed(format!("returned {code}")))
        }
        _ => Ok(()),
    }
}

/// Resolves an artist name to a MusicBrainz ID. Only an unambiguous exact
/// (normalized) name match with a high search score is accepted; namesakes
/// produce `Ok(None)` rather than a guess.
pub async fn search_musicbrainz_artist(
    client: &Client,
    artist_name: &str,
) -> Result<Option<String>, ProviderFetchError> {
    let cleaned: String = artist_name
        .chars()
        .filter(|c| !matches!(c, '"' | '\\'))
        .collect();
    if cleaned.trim().is_empty() {
        return Ok(None);
    }
    let query = format!("artist:\"{}\"", cleaned.trim());
    let url = format!(
        "{}/artist/?query={}&fmt=json&limit=5",
        musicbrainz_api_base(),
        urlencoding::encode(&query)
    );
    let t0 = log_request("musicbrainz", "artist_search", artist_name);
    let response = client
        .get(&url)
        .header("User-Agent", MUSICBRAINZ_USER_AGENT)
        .timeout(PROVIDER_TIMEOUT)
        .send()
        .await
        .map_err(|error| ProviderFetchError::Failed(error.to_string()))?;
    log_response("musicbrainz", "artist_search", t0, response.status());
    check_metabrainz_status(&response)?;
    let data: Value = response
        .json()
        .await
        .map_err(|error| ProviderFetchError::Failed(error.to_string()))?;
    let wanted = normalize_artist_name(&cleaned);
    let matches: Vec<&Value> = data["artists"]
        .as_array()
        .map(|artists| {
            artists
                .iter()
                .filter(|artist| {
                    artist["name"]
                        .as_str()
                        .is_some_and(|name| normalize_artist_name(name) == wanted)
                        && artist["score"].as_i64().unwrap_or(0) >= 90
                })
                .collect()
        })
        .unwrap_or_default();
    if matches.len() != 1 {
        return Ok(None);
    }
    Ok(matches[0]["id"]
        .as_str()
        .filter(|id| is_valid_mbid(id))
        .map(str::to_owned))
}

/// The MusicBrainz recording of a song: a search hit scoring 90+ whose artist credit names `artist_name` and
/// whose title is the same song (as [`boogiebox_db::popularity::normalize_title`] reduces both). `Ok(None)`
/// when nothing fits.
pub async fn search_musicbrainz_recording(
    client: &Client,
    artist_name: &str,
    title: &str,
) -> Result<Option<String>, ProviderFetchError> {
    let clean = |text: &str| -> String {
        text.chars()
            .filter(|c| !matches!(c, '"' | '\\'))
            .collect::<String>()
            .trim()
            .to_owned()
    };
    let (artist, song) = (clean(artist_name), clean(title));
    if artist.is_empty() || song.is_empty() {
        return Ok(None);
    }
    let query = format!("artist:\"{artist}\" AND recording:\"{song}\"");
    let url = format!(
        "{}/recording/?query={}&fmt=json&limit=5",
        musicbrainz_api_base(),
        urlencoding::encode(&query)
    );
    let t0 = log_request(
        "musicbrainz",
        "recording_search",
        &format!("{artist} — {song}"),
    );
    let response = client
        .get(&url)
        .header("User-Agent", MUSICBRAINZ_USER_AGENT)
        .timeout(PROVIDER_TIMEOUT)
        .send()
        .await
        .map_err(|error| ProviderFetchError::Failed(error.to_string()))?;
    log_response("musicbrainz", "recording_search", t0, response.status());
    check_metabrainz_status(&response)?;
    let data: Value = response
        .json()
        .await
        .map_err(|error| ProviderFetchError::Failed(error.to_string()))?;
    let wanted_artist = normalize_artist_name(&artist);
    let wanted_title = boogiebox_db::popularity::normalize_title(&song);
    Ok(data["recordings"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|rec| {
            rec["score"].as_i64().unwrap_or(0) >= 90
                && rec["title"]
                    .as_str()
                    .is_some_and(|t| boogiebox_db::popularity::normalize_title(t) == wanted_title)
                && rec["artist-credit"].as_array().is_some_and(|credits| {
                    credits.iter().any(|c| {
                        c["name"]
                            .as_str()
                            .or_else(|| c["artist"]["name"].as_str())
                            .is_some_and(|n| normalize_artist_name(n) == wanted_artist)
                    })
                })
        })
        .and_then(|rec| rec["id"].as_str())
        .filter(|id| is_valid_mbid(id))
        .map(str::to_ascii_lowercase))
}

/// Fetches MusicBrainz's user-voted genres/tags for a recording as `(name, votes)`.
pub async fn fetch_musicbrainz_recording_tags(
    client: &Client,
    mbid: &str,
) -> Result<Vec<(String, u64)>, ProviderFetchError> {
    fetch_musicbrainz_entity_tags(client, "recording", mbid).await
}

/// Fetches MusicBrainz's user-voted genres/tags for an artist as `(name, votes)`.
pub async fn fetch_musicbrainz_artist_tags(
    client: &Client,
    mbid: &str,
) -> Result<Vec<(String, u64)>, ProviderFetchError> {
    fetch_musicbrainz_entity_tags(client, "artist", mbid).await
}

async fn fetch_musicbrainz_entity_tags(
    client: &Client,
    entity: &str,
    mbid: &str,
) -> Result<Vec<(String, u64)>, ProviderFetchError> {
    if !is_valid_mbid(mbid) {
        return Err(ProviderFetchError::Failed(
            "invalid MusicBrainz id".to_owned(),
        ));
    }
    let url = format!(
        "{}/{entity}/{mbid}?inc=tags+genres&fmt=json",
        musicbrainz_api_base()
    );
    let t0 = log_request("musicbrainz", &format!("{entity}_tags"), mbid);
    let response = client
        .get(&url)
        .header("User-Agent", MUSICBRAINZ_USER_AGENT)
        .timeout(PROVIDER_TIMEOUT)
        .send()
        .await
        .map_err(|error| ProviderFetchError::Failed(error.to_string()))?;
    log_response(
        "musicbrainz",
        &format!("{entity}_tags"),
        t0,
        response.status(),
    );
    if response.status().as_u16() == 404 {
        return Ok(Vec::new());
    }
    check_metabrainz_status(&response)?;
    let data: Value = response
        .json()
        .await
        .map_err(|error| ProviderFetchError::Failed(error.to_string()))?;
    let mut merged: Vec<(String, u64)> = parse_tag_counts(&data["tags"]);
    for (name, count) in parse_tag_counts(&data["genres"]) {
        match merged.iter_mut().find(|(existing, _)| *existing == name) {
            Some(entry) => entry.1 = entry.1.max(count),
            None => merged.push((name, count)),
        }
    }
    Ok(merged)
}

fn parse_listenbrainz_similar_artists(
    data: &Value,
    source_mbid: &str,
    limit: usize,
) -> Vec<RelatedArtistCandidate> {
    let Some(items) = data.as_array() else {
        return Vec::new();
    };
    let usable: Vec<(&Value, String)> = items
        .iter()
        .filter_map(|item| {
            let id = item["artist_mbid"].as_str()?.trim();
            let name = item["name"].as_str()?.trim();
            (!name.is_empty() && is_valid_mbid(id) && !id.eq_ignore_ascii_case(source_mbid))
                .then(|| (item, id.to_owned()))
        })
        .collect();
    let top = usable
        .iter()
        .map(|(item, _)| item["score"].as_f64().unwrap_or(0.0))
        .fold(0.0_f64, f64::max);
    usable
        .into_iter()
        .take(limit.clamp(1, 100))
        .enumerate()
        .map(|(index, (item, id))| RelatedArtistCandidate {
            match_score: (top > 0.0)
                .then(|| (item["score"].as_f64().unwrap_or(0.0) / top).clamp(0.0, 1.0)),
            url: Some(format!("https://musicbrainz.org/artist/{id}")),
            external_id: Some(id),
            name: item["name"].as_str().unwrap_or_default().trim().to_owned(),
            image_url: None,
            rank: index + 1,
        })
        .collect()
}

/// Fetches ListenBrainz Labs' similar artists for a MusicBrainz artist id.
/// Scores are normalized against the strongest hit so they read as 0..1.
pub async fn fetch_listenbrainz_similar_artists(
    client: &Client,
    mbid: &str,
    limit: usize,
) -> Result<Vec<RelatedArtistCandidate>, ProviderFetchError> {
    if !is_valid_mbid(mbid) {
        return Err(ProviderFetchError::Failed(
            "invalid MusicBrainz id".to_owned(),
        ));
    }
    let url = format!(
        "{}/similar-artists/json?artist_mbids={mbid}&algorithm={LISTENBRAINZ_SIMILAR_ALGORITHM}",
        listenbrainz_labs_api_base()
    );
    let t0 = log_request("listenbrainz", "similar_artists", mbid);
    let response = client
        .get(&url)
        .header("User-Agent", MUSICBRAINZ_USER_AGENT)
        .timeout(PROVIDER_TIMEOUT)
        .send()
        .await
        .map_err(|error| ProviderFetchError::Failed(error.to_string()))?;
    log_response("listenbrainz", "similar_artists", t0, response.status());
    check_metabrainz_status(&response)?;
    let data: Value = response
        .json()
        .await
        .map_err(|error| ProviderFetchError::Failed(error.to_string()))?;
    Ok(parse_listenbrainz_similar_artists(&data, mbid, limit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_artist_pickers_require_exact_normalized_names() {
        let deezer = serde_json::json!([
            {"id": 1, "name": "Massive Attack Tribute", "picture_xl": "https://img/wrong.jpg"},
            {"id": 2, "name": "Massive-Attack", "picture_xl": "https://img/right.jpg"}
        ]);
        let selected = pick_deezer_artist_match(deezer.as_array().unwrap(), "Massive Attack")
            .expect("exact normalized match");
        assert_eq!(selected.external_id, "2");
        assert_eq!(selected.image_url.as_deref(), Some("https://img/right.jpg"));

        let spotify = serde_json::json!([
            {"id": "wrong", "name": "Portishead Tribute", "images": [{"url": "https://img/wrong.jpg"}]},
            {"id": "right", "name": "Portishead", "images": [{"url": "https://img/right.jpg"}]}
        ]);
        assert_eq!(
            pick_spotify_artist_match(spotify.as_array().unwrap(), "Portishead")
                .unwrap()
                .external_id,
            "right"
        );
        assert!(pick_spotify_artist_match(spotify.as_array().unwrap(), "Tricky").is_none());
    }

    #[test]
    fn discogs_picker_keeps_selected_identity_and_image_together() {
        let results = serde_json::json!([
            {"id": 11, "title": "UNKLE Tribute", "cover_image": "https://img/wrong.jpg"},
            {"id": 22, "title": "UNKLE", "cover_image": "https://img/right.jpg", "thumb": "https://img/thumb.jpg"}
        ]);
        let selected = pick_discogs_artist_match(results.as_array().unwrap(), "UNKLE").unwrap();
        assert_eq!(selected.external_id, "22");
        assert_eq!(selected.canonical_name, "UNKLE");
        assert_eq!(selected.image_url.as_deref(), Some("https://img/right.jpg"));
    }

    #[test]
    fn lastfm_artist_info_parses_identity_and_old_cache_defaults() {
        let data = serde_json::json!({
            "artist": {
                "name": "Massive Attack",
                "mbid": "10adbe5c-6cb4-4d2a-82a2-bfbdd0488cbd",
                "url": "https://last.fm/music/Massive+Attack",
                "stats": {"listeners": "10", "playcount": "20"},
                "bio": {"summary": "Summary", "content": "Full"},
                "image": [{"#text": ""}, {"#text": "https://img/artist.jpg"}],
                "tags": {"tag": [{"name": "trip-hop"}]}
            }
        });
        let parsed = parse_lastfm_artist_info(&data).unwrap();
        assert_eq!(
            parsed.mbid.as_deref(),
            Some("10adbe5c-6cb4-4d2a-82a2-bfbdd0488cbd")
        );
        assert_eq!(parsed.canonical_name.as_deref(), Some("Massive Attack"));
        assert_eq!(parsed.tags, vec!["trip-hop"]);

        let old: LastFmInfoPayload = serde_json::from_value(serde_json::json!({
            "summary": "Old", "full": "Old", "listeners": null,
            "playcount": null, "url": null, "image": null, "tags": []
        }))
        .unwrap();
        assert_eq!(old.mbid, None);
        assert_eq!(old.canonical_name, None);
    }

    #[test]
    fn related_artist_parsers_preserve_rank_score_and_ids() {
        let lastfm = serde_json::json!({
            "similarartists": {"artist": [
                {"name": "Portishead", "mbid": "mbid-portishead", "match": "0.93", "url": "https://last.fm/portishead", "image": [{"#text": "https://img/p.jpg"}]},
                {"name": "Tricky", "mbid": "", "match": 1.5}
            ]}
        });
        let candidates = parse_lastfm_similar_artists(&lastfm);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].rank, 1);
        assert_eq!(
            candidates[0].external_id.as_deref(),
            Some("mbid-portishead")
        );
        assert_eq!(candidates[0].match_score, Some(0.93));
        assert_eq!(candidates[1].match_score, Some(1.0));

        let deezer = serde_json::json!({"data": [
            {"id": 27, "name": "Daft Punk", "link": "https://deezer.com/artist/27", "picture_big": "https://img/dp.jpg"}
        ]});
        let related = parse_deezer_related_artists(&deezer);
        assert_eq!(related[0].external_id.as_deref(), Some("27"));
        assert_eq!(related[0].rank, 1);
        assert_eq!(related[0].match_score, None);

        assert!(parse_lastfm_similar_artists(&serde_json::json!({})).is_empty());
        assert!(parse_deezer_related_artists(&serde_json::json!({})).is_empty());
    }
}

/// Phase 3 of wip/server-rust-coverage-gap-plan.md: exercises the real HTTP-fetch
/// functions (as opposed to `mod tests` above, which only covers offline
/// parsing/picker logic) against a local `wiremock` server, via the
/// `BOOGIEBOX_*_API_BASE` env var overrides added to each `*_api_base()` helper.
///
/// These tests mutate process-wide env vars, so every test here holds `ENV_LOCK`
/// for its whole body to serialize against every other test in this module —
/// otherwise two tests running concurrently (the default) could each point their
/// providers at the other's mock server.
#[cfg(test)]
pub(crate) mod provider_fetch_tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// `pub(crate)` so other modules' tests (e.g. `post_scan.rs`, which drives
    /// these same provider-fetch functions through its orchestration lanes) can
    /// serialize against this module's tests instead of racing on the same
    /// process-wide `BOOGIEBOX_*_API_BASE` env vars via a second, unrelated lock.
    pub(crate) static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn search_discogs_album_cover_returns_first_valid_cover() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_DISCOGS_API_BASE", server.uri());

        Mock::given(method("GET"))
            .and(path("/database/search"))
            .and(query_param("type", "release"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{ "cover_image": "https://images.example/cover.jpg" }]
            })))
            .mount(&server)
            .await;

        let client = Client::new();
        let cover = search_discogs_album_cover(&client, "tok", "Artist", "Album").await;
        assert_eq!(cover.as_deref(), Some("https://images.example/cover.jpg"));

        std::env::remove_var("BOOGIEBOX_DISCOGS_API_BASE");
    }

    #[tokio::test]
    async fn search_discogs_album_cover_returns_none_on_server_error() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_DISCOGS_API_BASE", server.uri());

        Mock::given(method("GET"))
            .and(path("/database/search"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = Client::new();
        let cover = search_discogs_album_cover(&client, "tok", "Artist", "Album").await;
        assert!(cover.is_none());

        std::env::remove_var("BOOGIEBOX_DISCOGS_API_BASE");
    }

    #[tokio::test]
    async fn search_deezer_artist_match_and_image_use_the_first_exact_hit() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_DEEZER_API_BASE", server.uri());

        Mock::given(method("GET"))
            .and(path("/search/artist"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {"id": 1, "name": "Daft Punk Tribute", "picture_xl": "https://img/wrong.jpg"},
                    {"id": 27, "name": "Daft Punk", "picture_xl": "https://img/right.jpg"}
                ]
            })))
            .mount(&server)
            .await;

        let client = Client::new();
        let matched = search_deezer_artist_match(&client, "Daft Punk")
            .await
            .unwrap();
        assert_eq!(matched.external_id, "27");

        let image = search_deezer_artist_image(&client, "Daft Punk").await;
        assert_eq!(image.as_deref(), Some("https://img/right.jpg"));

        std::env::remove_var("BOOGIEBOX_DEEZER_API_BASE");
    }

    #[tokio::test]
    async fn get_spotify_access_token_succeeds_and_fails() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_SPOTIFY_ACCOUNTS_BASE", server.uri());

        Mock::given(method("POST"))
            .and(path("/api/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "access_token": "tok123" })),
            )
            .mount(&server)
            .await;

        let client = Client::new();
        let token = get_spotify_access_token(&client, "id", "secret").await;
        assert_eq!(token.as_deref(), Some("tok123"));

        std::env::remove_var("BOOGIEBOX_SPOTIFY_ACCOUNTS_BASE");
    }

    #[tokio::test]
    async fn search_metadata_combines_discogs_and_spotify_results() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_DISCOGS_API_BASE", server.uri());
        std::env::set_var("BOOGIEBOX_SPOTIFY_ACCOUNTS_BASE", server.uri());
        std::env::set_var("BOOGIEBOX_SPOTIFY_API_BASE", server.uri());

        Mock::given(method("GET"))
            .and(path("/database/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{ "title": "Artist - Album", "cover_image": "https://images.example/cover.jpg" }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "access_token": "tok123" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "albums": { "items": [] }
            })))
            .mount(&server)
            .await;

        let client = Client::new();
        let results = search_metadata_combined(
            &client,
            Some("discogs-tok"),
            Some("spotify-id"),
            Some("spotify-secret"),
            "Artist",
            Some("Album"),
        )
        .await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider, "discogs");
        assert_eq!(results[0].title.as_deref(), Some("Album"));

        std::env::remove_var("BOOGIEBOX_DISCOGS_API_BASE");
        std::env::remove_var("BOOGIEBOX_SPOTIFY_ACCOUNTS_BASE");
        std::env::remove_var("BOOGIEBOX_SPOTIFY_API_BASE");
    }

    // -- search_metadata failover chain (MusicBrainz -> Discogs -> Spotify -> Last.fm)

    #[tokio::test]
    async fn search_metadata_uses_lastfm_last_when_it_is_the_only_provider() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());

        Mock::given(method("GET"))
            .and(path("/"))
            .and(query_param("method", "artist.getinfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "artist": { "name": "Madonna", "bio": { "summary": "Pop icon." } }
            })))
            .mount(&server)
            .await;
        let client = Client::new();
        let (results, warnings) = search_metadata(
            &client,
            false,
            Some("lastfm-key"),
            None,
            None,
            None,
            "Madonna",
            None,
        )
        .await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider, "lastfm");
        assert_eq!(results[0].title.as_deref(), Some("Madonna"));
        assert!(warnings.is_empty());

        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    #[tokio::test]
    async fn search_metadata_falls_through_when_musicbrainz_has_nothing_and_skips_lastfm_after_a_match(
    ) {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_MUSICBRAINZ_API_BASE", server.uri());
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());
        std::env::set_var("BOOGIEBOX_SPOTIFY_ACCOUNTS_BASE", server.uri());
        std::env::set_var("BOOGIEBOX_SPOTIFY_API_BASE", server.uri());

        Mock::given(method("GET"))
            .and(path("/artist/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "artists": [{ "id": "79239441-bfd5-4981-a70c-55c3f15c1287", "score": 30, "name": "Madonnas" }]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/"))
            .and(query_param("method", "artist.getinfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "artist": { "name": "Madonna" }
            })))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "access_token": "tok123" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "artists": { "items": [{ "name": "Madonna", "images": [] }] }
            })))
            .mount(&server)
            .await;

        let client = Client::new();
        let (results, warnings) = search_metadata(
            &client,
            true,
            Some("lastfm-key"),
            None,
            Some("spotify-id"),
            Some("spotify-secret"),
            "Madonna",
            None,
        )
        .await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider, "spotify");
        assert!(
            warnings.is_empty(),
            "a low-score (not erroring) MusicBrainz search isn't a warning"
        );

        std::env::remove_var("BOOGIEBOX_MUSICBRAINZ_API_BASE");
        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
        std::env::remove_var("BOOGIEBOX_SPOTIFY_ACCOUNTS_BASE");
        std::env::remove_var("BOOGIEBOX_SPOTIFY_API_BASE");
    }

    #[tokio::test]
    async fn search_metadata_records_a_rate_limit_warning_and_keeps_failing_over() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_MUSICBRAINZ_API_BASE", server.uri());
        std::env::set_var("BOOGIEBOX_SPOTIFY_ACCOUNTS_BASE", server.uri());
        std::env::set_var("BOOGIEBOX_SPOTIFY_API_BASE", server.uri());

        Mock::given(method("GET"))
            .and(path("/artist/"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "access_token": "tok123" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "artists": { "items": [] }
            })))
            .mount(&server)
            .await;

        let client = Client::new();
        let (results, warnings) = search_metadata(
            &client,
            true,
            None,
            None,
            Some("spotify-id"),
            Some("spotify-secret"),
            "Madonna",
            None,
        )
        .await;
        assert!(results.is_empty());
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].provider, "musicbrainz");
        assert_eq!(warnings[0].reason, ProviderSearchFailure::RateLimited);

        std::env::remove_var("BOOGIEBOX_MUSICBRAINZ_API_BASE");
        std::env::remove_var("BOOGIEBOX_SPOTIFY_ACCOUNTS_BASE");
        std::env::remove_var("BOOGIEBOX_SPOTIFY_API_BASE");
    }

    #[tokio::test]
    async fn musicbrainz_metadata_search_lists_artist_namesakes_to_pick_from() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_MUSICBRAINZ_API_BASE", server.uri());
        Mock::given(method("GET"))
            .and(path("/artist/"))
            .and(query_param("query", "artist:(nirvana)"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "artists": [
                    { "id": "5b11f4ce-a62d-471e-81fc-a69a8278c7da", "score": 100, "name": "Nirvana",
                      "disambiguation": "90s US grunge band", "area": { "name": "United States" },
                      "life-span": { "begin": "1987" },
                      "tags": [{ "count": 3, "name": "rock" }, { "count": 9, "name": "grunge" }] },
                    { "id": "9282c8b4-ca0b-4c6b-b7e3-4f7762dfc4d6", "score": 100, "name": "Nirvana",
                      "disambiguation": "60s band from the UK", "area": { "name": "United Kingdom" } },
                    { "id": "not-an-id", "score": 100, "name": "Broken" },
                    { "id": "0d8ba0c2-1d43-4de0-bc0e-b0b2b1a4a6f1", "score": 20, "name": "Nirvana Tribute" }
                ]
            })))
            .mount(&server)
            .await;

        let client = Client::new();
        let (results, warnings) =
            search_metadata(&client, true, None, None, None, None, "Nirvana", None).await;
        assert!(warnings.is_empty());
        assert_eq!(results.len(), 2, "bad ids and weak scores are dropped");
        assert!(results.iter().all(|r| r.provider == "musicbrainz"));
        assert_eq!(results[0].title.as_deref(), Some("Nirvana"));
        assert_eq!(results[0].year.as_deref(), Some("1987"));
        assert_eq!(results[0].genre.as_deref(), Some("grunge"));
        assert_eq!(
            results[0].disambiguation.as_deref(),
            Some("90s US grunge band · United States")
        );
        assert_eq!(
            results[1].disambiguation.as_deref(),
            Some("60s band from the UK · United Kingdom")
        );
        let json = serde_json::to_value(&results[0]).unwrap();
        assert!(json.get("image").is_some() && json.get("releaseType").is_some());
        assert!(json.get("cover_url").is_none());

        std::env::remove_var("BOOGIEBOX_MUSICBRAINZ_API_BASE");
    }

    #[tokio::test]
    async fn musicbrainz_metadata_search_maps_release_groups_for_albums() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_MUSICBRAINZ_API_BASE", server.uri());
        let rg = "1b022e01-4da6-387b-8658-8678046e4cef";
        Mock::given(method("GET"))
            .and(path("/release-group/"))
            .and(query_param(
                "query",
                "releasegroup:(nevermind) AND artist:\"Nirvana\"",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "release-groups": [
                    { "id": rg, "score": 100, "title": "Nevermind", "primary-type": "Album",
                      "first-release-date": "1991-09-24",
                      "artist-credit": [{ "name": "Nirvana", "joinphrase": "" }],
                      "tags": [{ "count": 5, "name": "grunge" }] },
                    { "id": "6e8a7b7c-0b2a-4a4a-9b1a-0d6d3c3e2f11", "score": 90, "title": "Nevermind Hits",
                      "primary-type": "Album", "secondary-types": ["Compilation"],
                      "artist-credit": [{ "name": "Nirvana", "joinphrase": " & " }, { "name": "Friends" }] },
                    { "id": "3c2f9a1e-5b8d-4e6f-a7c9-1d2e3f4a5b6c", "score": 80, "title": "Nevermind Live",
                      "primary-type": "Broadcast", "first-release-date": "" }
                ]
            })))
            .mount(&server)
            .await;

        let client = Client::new();
        let (results, _) = search_metadata(
            &client,
            true,
            None,
            None,
            None,
            None,
            "Nirvana",
            Some("Nevermind"),
        )
        .await;
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].year.as_deref(), Some("1991"));
        assert_eq!(results[0].release_type.as_deref(), Some("album"));
        assert_eq!(results[0].genre.as_deref(), Some("grunge"));
        assert_eq!(
            results[0].cover_url.as_deref(),
            Some(format!("https://coverartarchive.org/release-group/{rg}/front-250").as_str())
        );
        assert_eq!(results[1].release_type.as_deref(), Some("compilation"));
        assert_eq!(results[1].artist.as_deref(), Some("Nirvana & Friends"));
        assert_eq!(results[2].release_type, None);
        assert_eq!(results[2].year, None);

        std::env::remove_var("BOOGIEBOX_MUSICBRAINZ_API_BASE");
    }

    #[test]
    fn lucene_terms_drop_query_syntax_and_operators() {
        assert_eq!(
            lucene_terms("Chase (Us 12\" Promo) AND [Bonus: Tracks] && C+C / Vol.2?"),
            "chase us 12 promo and bonus tracks c c vol.2"
        );
        assert_eq!(lucene_terms(" ()  "), "");
    }

    #[tokio::test]
    async fn search_metadata_maps_a_spotify_429_to_a_rate_limited_warning() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_SPOTIFY_ACCOUNTS_BASE", server.uri());
        std::env::set_var("BOOGIEBOX_SPOTIFY_API_BASE", server.uri());

        Mock::given(method("POST"))
            .and(path("/api/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "access_token": "tok123" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/search"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;

        let client = Client::new();
        // No Last.fm key, no Discogs token — Spotify is the only provider in
        // the chain, matching the real regression this was found from.
        let (results, warnings) = search_metadata(
            &client,
            false,
            None,
            None,
            Some("spotify-id"),
            Some("spotify-secret"),
            "Madonna",
            None,
        )
        .await;
        assert!(results.is_empty());
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].provider, "spotify");
        assert_eq!(warnings[0].reason, ProviderSearchFailure::RateLimited);

        std::env::remove_var("BOOGIEBOX_SPOTIFY_ACCOUNTS_BASE");
        std::env::remove_var("BOOGIEBOX_SPOTIFY_API_BASE");
    }

    #[tokio::test]
    async fn search_lastfm_metadata_maps_album_lookup_fields() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());

        Mock::given(method("GET"))
            .and(path("/"))
            .and(query_param("method", "album.getinfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "album": {
                    "artist": "Madonna",
                    "wiki": { "summary": "A classic." },
                    "image": [{ "#text": "https://img/cover.jpg" }],
                    "tags": { "tag": [{ "name": "pop" }] }
                }
            })))
            .mount(&server)
            .await;

        let client = Client::new();
        let results = search_lastfm_metadata(&client, "lastfm-key", "Madonna", Some("Erotica"))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider, "lastfm");
        assert_eq!(results[0].title.as_deref(), Some("Erotica"));
        assert_eq!(results[0].artist.as_deref(), Some("Madonna"));
        assert_eq!(results[0].genre.as_deref(), Some("pop"));
        assert_eq!(
            results[0].cover_url.as_deref(),
            Some("https://img/cover.jpg")
        );
        assert_eq!(results[0].description.as_deref(), Some("A classic."));

        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    #[tokio::test]
    async fn search_lastfm_metadata_returns_empty_not_an_error_when_unknown() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());

        Mock::given(method("GET"))
            .and(path("/"))
            .and(query_param("method", "artist.getinfo"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "artist": null
            })))
            .mount(&server)
            .await;

        let client = Client::new();
        let results = search_lastfm_metadata(&client, "lastfm-key", "Unknown Artist", None)
            .await
            .unwrap();
        assert!(results.is_empty());

        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    #[tokio::test]
    async fn fetch_lrclib_lyrics_returns_plain_lyrics_and_none_when_missing() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LRCLIB_API_BASE", server.uri());

        Mock::given(method("GET"))
            .and(path("/api/get"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "plainLyrics": "la la la"
            })))
            .mount(&server)
            .await;

        let client = Client::new();
        let result = fetch_lrclib_lyrics(&client, "Artist", "Title")
            .await
            .unwrap();
        assert_eq!(result.lyrics, "la la la");
        assert_eq!(result.source, "lrclib");

        std::env::remove_var("BOOGIEBOX_LRCLIB_API_BASE");
    }

    #[tokio::test]
    async fn fetch_lrclib_lyrics_returns_none_on_404() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LRCLIB_API_BASE", server.uri());

        Mock::given(method("GET"))
            .and(path("/api/get"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let client = Client::new();
        assert!(fetch_lrclib_lyrics(&client, "Artist", "Title")
            .await
            .is_none());

        std::env::remove_var("BOOGIEBOX_LRCLIB_API_BASE");
    }

    #[tokio::test]
    async fn fetch_lyricsovh_returns_lyrics() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LYRICS_OVH_API_BASE", server.uri());

        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "lyrics": "fallback lyrics"
            })))
            .mount(&server)
            .await;

        let client = Client::new();
        let result = fetch_lyricsovh(&client, "Artist", "Title").await.unwrap();
        assert_eq!(result.lyrics, "fallback lyrics");
        assert_eq!(result.source, "lyrics.ovh");

        std::env::remove_var("BOOGIEBOX_LYRICS_OVH_API_BASE");
    }

    #[tokio::test]
    async fn fetch_lastfm_artist_info_parses_bio_and_tags() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());

        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "artist": {
                    "name": "Massive Attack",
                    "mbid": "mbid-1",
                    "url": "https://last.fm/music/Massive+Attack",
                    "bio": { "summary": "short bio", "content": "long bio" },
                    "stats": { "listeners": "100", "playcount": "200" },
                    "image": [{ "#text": "" }, { "#text": "https://img/artist.jpg" }],
                    "tags": { "tag": [{ "name": "trip-hop" }] }
                }
            })))
            .mount(&server)
            .await;

        let client = Client::new();
        let info = fetch_lastfm_artist_info(&client, "key", "Massive Attack")
            .await
            .unwrap();
        assert_eq!(info.summary, "short bio");
        assert_eq!(info.image.as_deref(), Some("https://img/artist.jpg"));
        assert_eq!(info.tags, vec!["trip-hop".to_string()]);

        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    #[tokio::test]
    async fn fetch_lastfm_artist_info_returns_none_when_artist_missing() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());

        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;

        let client = Client::new();
        assert!(fetch_lastfm_artist_info(&client, "key", "Nobody")
            .await
            .is_none());

        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    #[tokio::test]
    async fn fetch_lastfm_top_tracks_returns_track_list_and_errors_on_failure() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());

        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "toptracks": { "track": [
                    { "name": "Song One", "playcount": "500" },
                    { "name": "Song Two", "playcount": "300" }
                ] }
            })))
            .mount(&server)
            .await;

        let client = Client::new();
        let tracks = fetch_lastfm_top_tracks(&client, "key", "Artist")
            .await
            .unwrap();
        assert_eq!(tracks.len(), 2);

        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    #[tokio::test]
    async fn fetch_lastfm_artist_top_tags_parses_tag_counts() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());

        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "toptags": { "tag": [{ "name": "rock", "count": 50 }] }
            })))
            .mount(&server)
            .await;

        let client = Client::new();
        let tags = fetch_lastfm_artist_top_tags(&client, "key", "Artist").await;
        assert_eq!(tags, vec![("rock".to_string(), 50)]);

        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    #[tokio::test]
    async fn fetch_deezer_related_artists_parses_and_errors_on_server_failure() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_DEEZER_API_BASE", server.uri());

        Mock::given(method("GET"))
            .and(path("/artist/27/related"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": 5, "name": "Related Artist", "link": "https://deezer.com/artist/5", "picture_big": "https://img/r.jpg"}]
            })))
            .mount(&server)
            .await;

        let client = Client::new();
        let related = fetch_deezer_related_artists(&client, "27", 10)
            .await
            .unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].name, "Related Artist");

        std::env::remove_var("BOOGIEBOX_DEEZER_API_BASE");
    }

    const MBID_A: &str = "10adbe5e-a2c0-4bf3-8249-2b4cbf6e6ca8";
    const MBID_B: &str = "8f6bd1e4-fbe1-4f50-aa9b-94c450ec0f11";

    #[tokio::test]
    async fn artist_track_counts_parse_and_tell_throttles_from_unknown_artists() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());
        let client = Client::new();

        Mock::given(method("GET"))
            .and(query_param("method", "artist.gettoptracks"))
            .and(query_param("artist", "Erasure"))
            .and(query_param("autocorrect", "1"))
            .and(query_param("limit", "1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "toptracks": { "track": [
                    { "name": "A Little Respect", "playcount": "900", "listeners": "90" },
                    { "name": "Sometimes", "playcount": 500 },
                    { "name": "  ", "playcount": "5" },
                    { "name": "Never Scrobbled", "playcount": "0" },
                    { "name": "No Count" }
                ]}
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(query_param("artist", "Single"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "toptracks": { "track": { "name": "Only Song", "playcount": "7" } }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(query_param("artist", "Empty"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "toptracks": { "track": [] }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(query_param("artist", "Unknown"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "error": 6, "message": "The artist you supplied could not be found"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(query_param("artist", "Throttled"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "error": 29, "message": "Rate limit exceeded"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(query_param("artist", "Too Fast"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(query_param("artist", "Broken"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(query_param("artist", "Bad Key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "error": 10, "message": "Invalid API key"
            })))
            .mount(&server)
            .await;

        let fetch = |artist: &'static str| {
            let client = client.clone();
            async move { fetch_lastfm_artist_track_counts(&client, "key", artist).await }
        };
        let counts = fetch("Erasure").await.unwrap();
        assert_eq!(counts.len(), 2);
        assert_eq!(counts[0].title, "A Little Respect");
        assert_eq!(counts[0].scrobbles, 900);
        assert_eq!(counts[0].listeners, Some(90));
        assert_eq!(counts[1].scrobbles, 500);
        assert_eq!(counts[1].listeners, None);
        assert_eq!(fetch("Single").await.unwrap()[0].title, "Only Song");
        assert!(fetch("Empty").await.unwrap().is_empty());
        assert!(fetch("Unknown").await.unwrap().is_empty());
        assert_eq!(
            fetch("Throttled").await.unwrap_err(),
            ProviderFetchError::RateLimited
        );
        assert_eq!(
            fetch("Too Fast").await.unwrap_err(),
            ProviderFetchError::RateLimited
        );
        assert!(matches!(
            fetch("Broken").await.unwrap_err(),
            ProviderFetchError::Failed(_)
        ));
        // A refused key gets its own name here: the refusal is remembered for the whole process.
        assert_eq!(
            fetch_lastfm_artist_track_counts(&client, "track-counts-refused-key", "Bad Key")
                .await
                .unwrap_err(),
            ProviderFetchError::KeyRejected
        );
        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    #[tokio::test]
    async fn a_refused_lastfm_key_stops_every_lookup_after_one_answer() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());
        let client = Client::new();
        // Each key is refused once, in a different way; nothing asks Last.fm again after that.
        Mock::given(method("GET"))
            .and(query_param("api_key", "dead-10"))
            .respond_with(
                ResponseTemplate::new(403).set_body_json(
                    serde_json::json!({ "error": 10, "message": "Invalid API key" }),
                ),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(query_param("api_key", "dead-403"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(query_param("api_key", "dead-26"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({ "error": 26, "message": "Suspended API key" }),
                ),
            )
            // The lookup, then the Settings check (which always asks).
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(query_param("api_key", "live-key"))
            .and(query_param("method", "chart.gettopartists"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "artists": {} })),
            )
            .mount(&server)
            .await;

        assert!(fetch_lastfm_artist_info(&client, "dead-10", "Cher")
            .await
            .is_none());
        assert_eq!(lastfm_key_health("dead-10"), LastfmKeyHealth::Rejected);
        assert!(!lastfm_key_usable("dead-10"));
        assert!(
            fetch_lastfm_album_info(&client, "dead-10", "Cher", "Believe")
                .await
                .is_none()
        );
        assert!(fetch_lastfm_artist_top_tags(&client, "dead-10", "Cher")
            .await
            .is_empty());
        let (results, warnings) = search_metadata(
            &client,
            false,
            Some("dead-10"),
            None,
            None,
            None,
            "Cher",
            None,
        )
        .await;
        assert!(
            results.is_empty() && warnings.is_empty(),
            "skipped, not a failure"
        );

        assert_eq!(
            fetch_lastfm_track_top_tags(&client, "dead-403", "A", "B").await,
            Err(ProviderFetchError::KeyRejected)
        );
        assert!(matches!(
            fetch_lastfm_artist_track_counts(&client, "dead-403", "A").await,
            Err(ProviderFetchError::KeyRejected)
        ));

        assert_eq!(
            fetch_lastfm_top_tracks(&client, "dead-26", "A").await,
            Err(LASTFM_KEY_REJECTED.to_owned())
        );
        assert_eq!(
            fetch_lastfm_similar_artists(&client, "dead-26", "A", None, 10).await,
            Err(LASTFM_KEY_REJECTED.to_owned())
        );
        assert_eq!(
            check_lastfm_key(&client, "dead-26").await,
            LastfmKeyHealth::Rejected
        );

        // A key nobody has asked about yet is usable; one Last.fm takes is Ok.
        assert_eq!(lastfm_key_health("live-key"), LastfmKeyHealth::Unknown);
        assert!(lastfm_key_usable("live-key"));
        assert_eq!(
            check_lastfm_key(&client, "live-key").await,
            LastfmKeyHealth::Ok
        );
        // A user-session error (9) on a 403 is not the key.
        assert!(!lastfm_reply_rejects_key(
            "live-key",
            403,
            &serde_json::json!({ "error": 9 })
        ));
        assert!(lastfm_key_usable("live-key"));
        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    #[tokio::test]
    async fn fetch_lastfm_track_top_tags_parses_and_distinguishes_rate_limits_from_misses() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LASTFM_API_BASE", server.uri());
        let client = Client::new();

        Mock::given(method("GET"))
            .and(query_param("method", "track.gettoptags"))
            .and(query_param("track", "Teardrop"))
            .and(query_param("autocorrect", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "toptags": { "tag": [
                    { "name": "trip-hop", "count": 100 },
                    { "name": "melancholy", "count": "45" },
                    { "name": "  ", "count": 3 }
                ]}
            })))
            .mount(&server)
            .await;
        let tags = fetch_lastfm_track_top_tags(&client, "key", "Massive Attack", "Teardrop")
            .await
            .unwrap();
        assert_eq!(
            tags,
            vec![("trip-hop".to_owned(), 100), ("melancholy".to_owned(), 45)]
        );

        // Last.fm error 6 = track not found → an empty result, not a failure.
        Mock::given(method("GET"))
            .and(query_param("track", "Unknown"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "error": 6, "message": "Track not found" })),
            )
            .mount(&server)
            .await;
        assert!(fetch_lastfm_track_top_tags(&client, "key", "A", "Unknown")
            .await
            .unwrap()
            .is_empty());

        // Error 29 and HTTP 429 are rate limits; other errors are plain failures.
        Mock::given(method("GET"))
            .and(query_param("track", "Limited"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "error": 29, "message": "Rate Limit Exceeded" }),
            ))
            .mount(&server)
            .await;
        assert_eq!(
            fetch_lastfm_track_top_tags(&client, "key", "A", "Limited").await,
            Err(ProviderFetchError::RateLimited)
        );
        Mock::given(method("GET"))
            .and(query_param("track", "Throttled"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;
        assert_eq!(
            fetch_lastfm_track_top_tags(&client, "key", "A", "Throttled").await,
            Err(ProviderFetchError::RateLimited)
        );
        Mock::given(method("GET"))
            .and(query_param("track", "Broken"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({ "error": 8, "message": "Operation failed" }),
                ),
            )
            .mount(&server)
            .await;
        assert_eq!(
            fetch_lastfm_track_top_tags(&client, "key", "A", "Broken").await,
            Err(ProviderFetchError::Failed("Operation failed".to_owned()))
        );
        Mock::given(method("GET"))
            .and(query_param("track", "Down"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        assert!(matches!(
            fetch_lastfm_track_top_tags(&client, "key", "A", "Down").await,
            Err(ProviderFetchError::Failed(_))
        ));

        std::env::remove_var("BOOGIEBOX_LASTFM_API_BASE");
    }

    #[tokio::test]
    async fn search_musicbrainz_artist_accepts_only_one_confident_exact_match() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_MUSICBRAINZ_API_BASE", server.uri());
        let client = Client::new();

        Mock::given(method("GET"))
            .and(path("/artist/"))
            .and(query_param("query", "artist:\"Massive Attack\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "artists": [
                    { "id": MBID_A, "name": "Massive Attack", "score": 100 },
                    { "id": MBID_B, "name": "Massive Attack Tribute", "score": 95 }
                ]
            })))
            .mount(&server)
            .await;
        assert_eq!(
            search_musicbrainz_artist(&client, "Massive Attack")
                .await
                .unwrap()
                .as_deref(),
            Some(MBID_A)
        );

        // Two exact namesakes → ambiguous → no guess.
        Mock::given(method("GET"))
            .and(path("/artist/"))
            .and(query_param("query", "artist:\"Nirvana\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "artists": [
                    { "id": MBID_A, "name": "Nirvana", "score": 100 },
                    { "id": MBID_B, "name": "Nirvana", "score": 98 }
                ]
            })))
            .mount(&server)
            .await;
        assert_eq!(
            search_musicbrainz_artist(&client, "Nirvana").await.unwrap(),
            None
        );

        // Low score, throttling, and blank/quote-only names.
        Mock::given(method("GET"))
            .and(path("/artist/"))
            .and(query_param("query", "artist:\"Weak\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "artists": [{ "id": MBID_A, "name": "Weak", "score": 60 }]
            })))
            .mount(&server)
            .await;
        assert_eq!(
            search_musicbrainz_artist(&client, "Weak").await.unwrap(),
            None
        );
        Mock::given(method("GET"))
            .and(path("/artist/"))
            .and(query_param("query", "artist:\"Busy\""))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        assert_eq!(
            search_musicbrainz_artist(&client, "Busy").await,
            Err(ProviderFetchError::RateLimited)
        );
        assert_eq!(
            search_musicbrainz_artist(&client, "\"\"").await.unwrap(),
            None
        );

        std::env::remove_var("BOOGIEBOX_MUSICBRAINZ_API_BASE");
    }

    #[tokio::test]
    async fn fetch_musicbrainz_artist_tags_merges_tags_and_genres() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_MUSICBRAINZ_API_BASE", server.uri());
        let client = Client::new();

        Mock::given(method("GET"))
            .and(path(format!("/artist/{MBID_A}")))
            .and(query_param("inc", "tags genres")) // `+` decodes to a space in query strings
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tags": [{ "name": "downtempo", "count": 12 }, { "name": "british", "count": 5 }],
                "genres": [{ "name": "downtempo", "count": 14 }, { "name": "trip hop", "count": 9 }]
            })))
            .mount(&server)
            .await;
        let tags = fetch_musicbrainz_artist_tags(&client, MBID_A)
            .await
            .unwrap();
        assert_eq!(
            tags,
            vec![
                ("downtempo".to_owned(), 14),
                ("british".to_owned(), 5),
                ("trip hop".to_owned(), 9)
            ]
        );

        Mock::given(method("GET"))
            .and(path(format!("/artist/{MBID_B}")))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        assert!(fetch_musicbrainz_artist_tags(&client, MBID_B)
            .await
            .unwrap()
            .is_empty());

        // A malformed id never reaches the network.
        assert!(matches!(
            fetch_musicbrainz_artist_tags(&client, "../../etc").await,
            Err(ProviderFetchError::Failed(_))
        ));
        assert!(!is_valid_mbid("not-a-uuid"));
        assert!(is_valid_mbid(MBID_A));

        std::env::remove_var("BOOGIEBOX_MUSICBRAINZ_API_BASE");
    }

    #[tokio::test]
    async fn fetch_listenbrainz_similar_artists_normalizes_scores_and_skips_self() {
        let _guard = ENV_LOCK.lock().await;
        let server = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LISTENBRAINZ_LABS_API_BASE", server.uri());
        let client = Client::new();

        Mock::given(method("GET"))
            .and(path("/similar-artists/json"))
            .and(query_param("artist_mbids", MBID_A))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "artist_mbid": MBID_A, "name": "Massive Attack", "score": 9999 },
                { "artist_mbid": MBID_B, "name": "Portishead", "score": 8000 },
                { "artist_mbid": "a74b1b7f-71a5-4011-9441-d0b5e4122711", "name": "Radiohead", "score": 4000 },
                { "artist_mbid": "bad", "name": "Skipped", "score": 1 },
                { "artist_mbid": "cb67438a-7f50-4f2b-a6f1-2bb2729fd538", "name": " ", "score": 1 }
            ])))
            .mount(&server)
            .await;
        let similar = fetch_listenbrainz_similar_artists(&client, MBID_A, 10)
            .await
            .unwrap();
        assert_eq!(similar.len(), 2);
        assert_eq!(similar[0].name, "Portishead");
        assert_eq!(similar[0].external_id.as_deref(), Some(MBID_B));
        assert_eq!(similar[0].rank, 1);
        assert_eq!(similar[0].match_score, Some(1.0));
        assert!((similar[1].match_score.unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(
            fetch_listenbrainz_similar_artists(&client, MBID_A, 1)
                .await
                .unwrap()
                .len(),
            1
        );

        Mock::given(method("GET"))
            .and(query_param("artist_mbids", MBID_B))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;
        assert_eq!(
            fetch_listenbrainz_similar_artists(&client, MBID_B, 5).await,
            Err(ProviderFetchError::RateLimited)
        );
        assert!(matches!(
            fetch_listenbrainz_similar_artists(&client, "nope", 5).await,
            Err(ProviderFetchError::Failed(_))
        ));
        assert!(parse_listenbrainz_similar_artists(&serde_json::json!({}), MBID_A, 5).is_empty());
        assert_eq!(
            format!("{}", ProviderFetchError::Failed("boom".to_owned())),
            "boom"
        );
        assert!(format!("{}", ProviderFetchError::RateLimited).contains("rate limit"));

        std::env::remove_var("BOOGIEBOX_LISTENBRAINZ_LABS_API_BASE");
    }

    #[tokio::test]
    async fn download_image_returns_bytes_and_extension() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cover.png"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(vec![1u8, 2, 3, 4])
                    .insert_header("content-type", "image/png"),
            )
            .mount(&server)
            .await;

        let client = Client::new();
        let (bytes, ext) = download_image(&client, &format!("{}/cover.png", server.uri()))
            .await
            .unwrap();
        assert_eq!(bytes, vec![1, 2, 3, 4]);
        assert_eq!(ext, ".png");
    }
}

// ── URL encoding helper ───────────────────────────────────────────────────────

pub(crate) mod urlencoding {
    /// Documents the Encode public API surface.
    pub fn encode(s: &str) -> String {
        let mut out = String::new();
        for byte in s.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(byte as char);
                }
                _ => {
                    out.push('%');
                    out.push_str(&format!("{byte:02X}"));
                }
            }
        }
        out
    }
}
