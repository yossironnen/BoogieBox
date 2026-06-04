//! Defines Rust server support logic for Providers.

use reqwest::Client;
use serde_json::Value;

const USER_AGENT: &str = "BoogieBox/1.0";

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

fn score_discogs_cover_result(result: &Value, artist_name: &str, album_title: &str) -> i32 {
    let raw_title = result["title"].as_str().unwrap_or("");
    let cover = result["cover_image"].as_str().unwrap_or("");

    if cover.is_empty() || cover.contains("spacer") || cover.starts_with("https://st.discogs.com") {
        return -1;
    }

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
        "https://api.discogs.com/database/search?type=release&q={}&per_page=3&page=1",
        urlencoding::encode(q.trim())
    );

    let resp = client
        .get(&url)
        .header("Authorization", format!("Discogs token={token}"))
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let data: Value = resp.json().await.ok()?;
    let results = data["results"].as_array()?;
    pick_discogs_cover_image(results, artist_name, album_title)
}

/// Documents the Search Discogs Artist Image public API surface.
pub async fn search_discogs_artist_image(
    client: &Client,
    token: &str,
    artist_name: &str,
) -> Option<String> {
    let url = format!(
        "https://api.discogs.com/database/search?type=artist&q={}&per_page=3&page=1",
        urlencoding::encode(artist_name.trim())
    );

    let resp = client
        .get(&url)
        .header("Authorization", format!("Discogs token={token}"))
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let data: Value = resp.json().await.ok()?;
    let results = data["results"].as_array()?;

    for result in results {
        let thumb = result["thumb"].as_str().unwrap_or("");
        if !thumb.is_empty()
            && !thumb.contains("spacer")
            && !thumb.starts_with("https://st.discogs.com")
        {
            // Try the full-size cover_image first
            if let Some(cover) = result["cover_image"].as_str() {
                if !cover.is_empty()
                    && !cover.contains("spacer")
                    && !cover.starts_with("https://st.discogs.com")
                {
                    return Some(cover.to_owned());
                }
            }
            return Some(thumb.to_owned());
        }
    }
    None
}

// ── Deezer ────────────────────────────────────────────────────────────────────

/// Documents the Search Deezer Artist Image public API surface.
pub async fn search_deezer_artist_image(client: &Client, artist_name: &str) -> Option<String> {
    let url = format!(
        "https://api.deezer.com/search/artist?q={}",
        urlencoding::encode(artist_name.trim())
    );

    let resp = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let data: Value = resp.json().await.ok()?;
    let artists = data["data"].as_array()?;

    for artist in artists {
        let name = artist["name"].as_str().unwrap_or("");
        if name.to_lowercase() == artist_name.to_lowercase() {
            if let Some(pic) = artist["picture_xl"]
                .as_str()
                .or_else(|| artist["picture_big"].as_str())
                .or_else(|| artist["picture_medium"].as_str())
            {
                if !pic.is_empty()
                    && !pic.contains("artist_default")
                    && !pic.contains("default_avatar")
                {
                    return Some(pic.to_owned());
                }
            }
        }
    }

    // Second pass: take the first result with a real picture
    for artist in artists {
        if let Some(pic) = artist["picture_xl"]
            .as_str()
            .or_else(|| artist["picture_big"].as_str())
        {
            if !pic.is_empty() && !pic.contains("artist_default") && !pic.contains("default_avatar")
            {
                return Some(pic.to_owned());
            }
        }
    }
    None
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
    let resp = client
        .post("https://accounts.spotify.com/api/token")
        .header("Authorization", format!("Basic {credentials}"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("grant_type=client_credentials")
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let data: Value = resp.json().await.ok()?;
    data["access_token"].as_str().map(str::to_owned)
}

/// Documents the Search Spotify Artist Image public API surface.
pub async fn search_spotify_artist_image(
    client: &Client,
    client_id: &str,
    client_secret: &str,
    artist_name: &str,
) -> Option<String> {
    let token = get_spotify_access_token(client, client_id, client_secret).await?;
    let url = format!(
        "https://api.spotify.com/v1/search?type=artist&limit=3&q={}",
        urlencoding::encode(artist_name.trim())
    );

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let data: Value = resp.json().await.ok()?;
    let artists = data["artists"]["items"].as_array()?;

    for artist in artists {
        let name = artist["name"].as_str().unwrap_or("");
        if name.to_lowercase() == artist_name.to_lowercase() {
            if let Some(img) = artist["images"].as_array().and_then(|imgs| imgs.first()) {
                if let Some(url_str) = img["url"].as_str() {
                    return Some(url_str.to_owned());
                }
            }
        }
    }
    // First result fallback
    if let Some(artist) = artists.first() {
        if let Some(img) = artist["images"].as_array().and_then(|imgs| imgs.first()) {
            return img["url"].as_str().map(str::to_owned);
        }
    }
    None
}

// ── Metadata search (multi-provider) ─────────────────────────────────────────

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
    /// Documents the Release Type public API surface.
    pub release_type: Option<String>,
    /// Documents the Genre public API surface.
    pub genre: Option<String>,
    /// Documents the Cover Url public API surface.
    pub cover_url: Option<String>,
    /// Documents the Tracklist public API surface.
    pub tracklist: Option<Vec<Value>>,
    /// Documents the Description public API surface.
    pub description: Option<String>,
    /// Documents the Extra public API surface.
    pub extra: Option<Value>,
}

/// Documents the Search Metadata public API surface.
pub async fn search_metadata(
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
            if let Some(discogs_results) =
                search_discogs_metadata(client, token, artist, album_title).await
            {
                results.extend(discogs_results);
            }
        }
    }

    if let (Some(id), Some(secret)) = (spotify_client_id, spotify_client_secret) {
        if let Some(spotify_results) =
            search_spotify_metadata(client, id, secret, artist, album).await
        {
            results.extend(spotify_results);
        }
    }

    results
}

async fn search_discogs_metadata(
    client: &Client,
    token: &str,
    artist: &str,
    album: &str,
) -> Option<Vec<MetadataSearchResult>> {
    let q = format!("{} {}", artist, album);
    let url = format!(
        "https://api.discogs.com/database/search?type=release&q={}&per_page=5&page=1",
        urlencoding::encode(q.trim())
    );

    let resp = client
        .get(&url)
        .header("Authorization", format!("Discogs token={token}"))
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let data: Value = resp.json().await.ok()?;
    let items = data["results"].as_array()?;

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
                    .and_then(|f| f.first())
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                genre: item["genre"]
                    .as_array()
                    .and_then(|g| g.first())
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                cover_url,
                tracklist: None,
                description: None,
                extra: Some(item.clone()),
            }
        })
        .collect();

    Some(results)
}

async fn search_spotify_metadata(
    client: &Client,
    client_id: &str,
    client_secret: &str,
    artist: &str,
    album: Option<&str>,
) -> Option<Vec<MetadataSearchResult>> {
    let token = get_spotify_access_token(client, client_id, client_secret).await?;

    let q = if let Some(al) = album {
        format!("artist:{} album:{}", artist, al)
    } else {
        format!("artist:{}", artist)
    };

    let search_type = if album.is_some() { "album" } else { "artist" };
    let url = format!(
        "https://api.spotify.com/v1/search?type={search_type}&limit=5&q={}",
        urlencoding::encode(&q)
    );

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let data: Value = resp.json().await.ok()?;

    if album.is_some() {
        let items = data["albums"]["items"].as_array()?;
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
                    tracklist: None,
                    description: None,
                    extra: Some(item.clone()),
                }
            })
            .collect();
        Some(results)
    } else {
        let items = data["artists"]["items"].as_array()?;
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
                    tracklist: None,
                    description: None,
                    extra: Some(item.clone()),
                }
            })
            .collect();
        Some(results)
    }
}

// ── Image download ────────────────────────────────────────────────────────────

/// Documents the Download Image public API surface.
pub async fn download_image(client: &Client, url: &str) -> Option<(Vec<u8>, String)> {
    let resp = client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
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
        "https://lrclib.net/api/get?artist_name={}&track_name={}",
        urlencoding::encode(artist),
        urlencoding::encode(title)
    );

    let resp = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .ok()?;

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
        "https://api.lyrics.ovh/v1/{}/{}",
        urlencoding::encode(artist),
        urlencoding::encode(title)
    );

    let resp = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .ok()?;

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

const LASTFM_API_ROOT: &str = "https://ws.audioscrobbler.com/2.0/";

/// Public Last Fm Info Payload data shape used by BoogieBox.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct LastFmInfoPayload {
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

/// Documents the Fetch Lastfm Artist Info public API surface.
pub async fn fetch_lastfm_artist_info(
    client: &Client,
    api_key: &str,
    artist_name: &str,
) -> Option<LastFmInfoPayload> {
    let url = format!(
        "{LASTFM_API_ROOT}?method=artist.getinfo&artist={}&api_key={}&format=json",
        urlencoding::encode(artist_name),
        api_key
    );
    let data: Value = client.get(&url).send().await.ok()?.json().await.ok()?;
    let artist = &data["artist"];
    if artist.is_null() {
        return None;
    }
    let summary = artist["bio"]["summary"].as_str().unwrap_or("").to_owned();
    let full = artist["bio"]["content"].as_str().unwrap_or("").to_owned();
    let listeners = artist["stats"]["listeners"].as_str().map(str::to_owned);
    let playcount = artist["stats"]["playcount"].as_str().map(str::to_owned);
    let url = artist["url"].as_str().map(str::to_owned);
    let image = artist["image"]
        .as_array()
        .and_then(|imgs| {
            imgs.iter()
                .rev()
                .find(|i| !i["#text"].as_str().unwrap_or("").is_empty())
        })
        .and_then(|i| i["#text"].as_str())
        .map(str::to_owned);
    let tags = artist["tags"]["tag"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Some(LastFmInfoPayload {
        summary,
        full,
        listeners,
        playcount,
        url,
        image,
        tags,
    })
}

/// Documents the Fetch Lastfm Album Info public API surface.
pub async fn fetch_lastfm_album_info(
    client: &Client,
    api_key: &str,
    artist_name: &str,
    album_name: &str,
) -> Option<LastFmInfoPayload> {
    let url = format!(
        "{LASTFM_API_ROOT}?method=album.getinfo&artist={}&album={}&api_key={}&format=json",
        urlencoding::encode(artist_name),
        urlencoding::encode(album_name),
        api_key
    );
    let data: Value = client.get(&url).send().await.ok()?.json().await.ok()?;
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
    let url = format!(
        "{LASTFM_API_ROOT}?method=artist.gettoptags&artist={}&api_key={}&format=json",
        urlencoding::encode(artist_name),
        api_key
    );
    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(_) => return vec![],
    };
    let data: Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return vec![],
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
    let url = format!(
        "{LASTFM_API_ROOT}?method=artist.getTopTracks&api_key={}&artist={}&format=json&autocorrect=1&limit=10",
        api_key,
        urlencoding::encode(artist_name),
    );
    let data: Value = client
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
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

// ── URL encoding helper ───────────────────────────────────────────────────────

mod urlencoding {
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
