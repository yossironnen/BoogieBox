//! Artist bios and album descriptions without a key (`wip/lastfm-replacement-plan.md` phase 2): the English
//! Wikipedia summary reached through MusicBrainz → Wikidata links (never a name search, so no disambiguation
//! pages), then TheAudioDB with its free key, then a Last.fm answer cached before (until Last.fm support is removed).
//! Artist stats are ListenBrainz listen and listener counts.

use std::time::Duration;

use boogiebox_db::music::EntityId;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::providers::urlencoding;
use crate::providers::{
    check_metabrainz_status, is_valid_mbid, listenbrainz_api_base, musicbrainz_api_base,
    optional_nonempty_json_string, ProviderFetchError, MUSICBRAINZ_USER_AGENT,
};
use crate::DbPool;

const TIMEOUT: Duration = Duration::from_secs(10);
/// A found text is kept a week; "nothing anywhere" a day.
const FOUND_DAYS: i64 = 7;
const EMPTY_DAYS: i64 = 1;
/// TheAudioDB's free key (`https://www.theaudiodb.com/free_music_api`: 30 requests a minute).
const THEAUDIODB_FREE_KEY: &str = "123";

fn base(env_var: &str, default: &str) -> String {
    std::env::var(env_var).unwrap_or_else(|_| default.to_owned())
}

fn wikidata_base() -> String {
    base("BOOGIEBOX_WIKIDATA_API_BASE", "https://www.wikidata.org")
}

fn wikipedia_base() -> String {
    base("BOOGIEBOX_WIKIPEDIA_API_BASE", "https://en.wikipedia.org")
}

fn theaudiodb_base() -> String {
    base(
        "BOOGIEBOX_THEAUDIODB_API_BASE",
        &format!("https://www.theaudiodb.com/api/v1/json/{THEAUDIODB_FREE_KEY}"),
    )
}

/// What the bio block shows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct About {
    /// `wikipedia`, `theaudiodb` or `lastfm`; `None` when no source had a text.
    pub source: Option<String>,
    pub summary: String,
    /// The longer text behind "Read more", when the source has one.
    pub full: Option<String>,
    /// The source's page for this artist or album.
    pub url: Option<String>,
    /// ListenBrainz counts (artists only).
    pub stats: Option<AboutStats>,
}

/// ListenBrainz listen and listener counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AboutStats {
    pub listens: u64,
    pub listeners: u64,
}

struct Text {
    summary: String,
    full: Option<String>,
    url: Option<String>,
}

fn get(client: &Client, url: &str) -> reqwest::RequestBuilder {
    client
        .get(url)
        .header("User-Agent", MUSICBRAINZ_USER_AGENT)
        .timeout(TIMEOUT)
}

fn failed(error: impl std::fmt::Display) -> ProviderFetchError {
    ProviderFetchError::Failed(error.to_string())
}

/// The Wikidata item (`Q…`) and English Wikipedia title MusicBrainz links to an artist or release group.
async fn musicbrainz_wiki_links(
    client: &Client,
    entity: &str,
    mbid: &str,
) -> Result<(Option<String>, Option<String>), ProviderFetchError> {
    let url = format!(
        "{}/{entity}/{mbid}?inc=url-rels&fmt=json",
        musicbrainz_api_base()
    );
    let response = get(client, &url).send().await.map_err(failed)?;
    if response.status().as_u16() == 404 {
        return Ok((None, None));
    }
    check_metabrainz_status(&response)?;
    let data: Value = response.json().await.map_err(failed)?;
    let mut wikidata = None;
    let mut wikipedia = None;
    for relation in data["relations"].as_array().into_iter().flatten() {
        let Some(resource) = relation["url"]["resource"].as_str() else {
            continue;
        };
        match relation["type"].as_str() {
            Some("wikidata") => {
                wikidata = resource
                    .rsplit('/')
                    .next()
                    .filter(|id| id.starts_with('Q') && id[1..].chars().all(|c| c.is_ascii_digit()))
                    .map(str::to_owned);
            }
            Some("wikipedia") => {
                if let Some(title) = resource.strip_prefix("https://en.wikipedia.org/wiki/") {
                    wikipedia = Some(percent_decode(title));
                }
            }
            _ => {}
        }
    }
    Ok((wikidata, wikipedia))
}

/// `%XX` escapes in a URL path back to text (a malformed escape stays as it is).
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() && text.is_char_boundary(i + 3) {
            if let Ok(byte) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The English Wikipedia article of a Wikidata item.
async fn wikidata_enwiki_title(
    client: &Client,
    qid: &str,
) -> Result<Option<String>, ProviderFetchError> {
    let url = format!(
        "{}/w/api.php?action=wbgetentities&ids={qid}&props=sitelinks&sitefilter=enwiki&format=json",
        wikidata_base()
    );
    let response = get(client, &url).send().await.map_err(failed)?;
    if response.status().as_u16() == 429 {
        return Err(ProviderFetchError::RateLimited);
    }
    let data: Value = response.json().await.map_err(failed)?;
    Ok(optional_nonempty_json_string(
        &data["entities"][qid]["sitelinks"]["enwiki"]["title"],
    ))
}

/// A Wikipedia article's summary (its lead, as plain text); none for a missing or disambiguation page.
async fn wikipedia_summary(
    client: &Client,
    title: &str,
) -> Result<Option<Text>, ProviderFetchError> {
    let url = format!(
        "{}/api/rest_v1/page/summary/{}",
        wikipedia_base(),
        urlencoding::encode(&title.replace(' ', "_"))
    );
    let response = get(client, &url).send().await.map_err(failed)?;
    match response.status().as_u16() {
        404 => return Ok(None),
        429 => return Err(ProviderFetchError::RateLimited),
        code if !(200..300).contains(&code) => {
            return Err(failed(format!("Wikipedia returned {code}")))
        }
        _ => {}
    }
    let data: Value = response.json().await.map_err(failed)?;
    if data["type"].as_str() == Some("disambiguation") {
        return Ok(None);
    }
    Ok(
        optional_nonempty_json_string(&data["extract"]).map(|summary| Text {
            summary,
            full: None,
            url: optional_nonempty_json_string(&data["content_urls"]["desktop"]["page"]),
        }),
    )
}

/// The Wikipedia text for an artist (`artist`) or album (`release-group`) by its MusicBrainz id.
async fn wikipedia_text(
    client: &Client,
    entity: &str,
    mbid: &str,
) -> Result<Option<Text>, ProviderFetchError> {
    let (qid, direct) = musicbrainz_wiki_links(client, entity, mbid).await?;
    let title = match (direct, qid) {
        (Some(title), _) => Some(title),
        (None, Some(qid)) => wikidata_enwiki_title(client, &qid).await?,
        (None, None) => None,
    };
    match title {
        Some(title) => wikipedia_summary(client, &title).await,
        None => Ok(None),
    }
}

/// A long text as a summary (its first paragraph) and the whole text when there's more.
fn split_text(text: &str) -> Option<(String, Option<String>)> {
    let text = text.replace("\r\n", "\n");
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let first = text.split("\n\n").next().unwrap_or(text).trim().to_owned();
    let full = (first.len() < text.len()).then(|| text.to_owned());
    Some((first, full))
}

fn same_name(a: &str, b: &str) -> bool {
    let key = |s: &str| boogiebox_db::popularity::normalize_title(s);
    key(a) == key(b)
}

async fn theaudiodb_json(
    client: &Client,
    path_and_query: &str,
) -> Result<Value, ProviderFetchError> {
    let url = format!("{}/{path_and_query}", theaudiodb_base());
    let response = get(client, &url).send().await.map_err(failed)?;
    match response.status().as_u16() {
        429 => return Err(ProviderFetchError::RateLimited),
        code if !(200..300).contains(&code) => {
            return Err(failed(format!("TheAudioDB returned {code}")))
        }
        _ => {}
    }
    // An unknown artist can come back as an empty body.
    Ok(response.json().await.unwrap_or(Value::Null))
}

/// An artist's English biography on TheAudioDB: by MusicBrainz id, else by exact name.
async fn theaudiodb_artist(
    client: &Client,
    mbid: Option<&str>,
    name: &str,
) -> Result<Option<Text>, ProviderFetchError> {
    let pick = |data: &Value, check_name: bool| -> Option<Text> {
        let artist = data["artists"]
            .as_array()?
            .iter()
            .find(|a| !check_name || a["strArtist"].as_str().is_some_and(|n| same_name(n, name)))?;
        let (summary, full) = split_text(artist["strBiographyEN"].as_str()?)?;
        let url = optional_nonempty_json_string(&artist["idArtist"])
            .map(|id| format!("https://www.theaudiodb.com/artist/{id}"));
        Some(Text { summary, full, url })
    };
    if let Some(mbid) = mbid {
        let data = theaudiodb_json(client, &format!("artist-mb.php?i={mbid}")).await?;
        if let Some(text) = pick(&data, false) {
            return Ok(Some(text));
        }
    }
    let data = theaudiodb_json(
        client,
        &format!("search.php?s={}", urlencoding::encode(name)),
    )
    .await?;
    Ok(pick(&data, true))
}

/// An album's English description on TheAudioDB: by release-group id, else by artist and exact title.
async fn theaudiodb_album(
    client: &Client,
    release_group: Option<&str>,
    artist: &str,
    title: &str,
) -> Result<Option<Text>, ProviderFetchError> {
    let pick = |data: &Value, check_title: bool| -> Option<Text> {
        let album = data["album"].as_array()?.iter().find(|a| {
            !check_title || a["strAlbum"].as_str().is_some_and(|t| same_name(t, title))
        })?;
        let (summary, full) = split_text(album["strDescriptionEN"].as_str()?)?;
        let url = optional_nonempty_json_string(&album["idAlbum"])
            .map(|id| format!("https://www.theaudiodb.com/album/{id}"));
        Some(Text { summary, full, url })
    };
    if let Some(rg) = release_group {
        let data = theaudiodb_json(client, &format!("album-mb.php?i={rg}")).await?;
        if let Some(text) = pick(&data, false) {
            return Ok(Some(text));
        }
    }
    let data = theaudiodb_json(
        client,
        &format!(
            "searchalbum.php?s={}&a={}",
            urlencoding::encode(artist),
            urlencoding::encode(title)
        ),
    )
    .await?;
    Ok(pick(&data, true))
}

/// An artist's ListenBrainz listen and listener counts.
async fn listenbrainz_artist_stats(
    client: &Client,
    mbid: &str,
) -> Result<Option<AboutStats>, ProviderFetchError> {
    let url = format!("{}/1/popularity/artist", listenbrainz_api_base());
    let response = client
        .post(&url)
        .header("User-Agent", MUSICBRAINZ_USER_AGENT)
        .timeout(TIMEOUT)
        .json(&serde_json::json!({ "artist_mbids": [mbid] }))
        .send()
        .await
        .map_err(failed)?;
    check_metabrainz_status(&response)?;
    let data: Value = response.json().await.map_err(failed)?;
    let row = &data[0];
    Ok(
        match (
            row["total_listen_count"].as_u64(),
            row["total_user_count"].as_u64(),
        ) {
            (Some(listens), Some(listeners)) if listens > 0 => {
                Some(AboutStats { listens, listeners })
            }
            _ => None,
        },
    )
}

fn from_lastfm(cached: Option<String>) -> Option<Text> {
    // Read loosely: old cache entries differ in which fields they have.
    let info: Value = serde_json::from_str(&cached?).ok()?;
    let summary = optional_nonempty_json_string(&info["summary"])?;
    let full = optional_nonempty_json_string(&info["full"]).filter(|f| f.trim() != summary.trim());
    Some(Text {
        summary: summary.trim().to_owned(),
        full,
        url: optional_nonempty_json_string(&info["url"]),
    })
}

fn about_from(source: &str, text: Text, stats: Option<AboutStats>) -> About {
    About {
        source: Some(source.to_owned()),
        summary: text.summary,
        full: text.full,
        url: text.url,
        stats,
    }
}

fn artist_key(id: &EntityId) -> String {
    format!("about:artist:{id}")
}

fn album_key(id: &EntityId) -> String {
    format!("about:album:{id}")
}

/// The cached text for an artist, without asking anyone (Subsonic `getArtistInfo`).
pub fn cached_artist_about(conn: &rusqlite::Connection, id: &EntityId) -> Option<About> {
    boogiebox_db::artwork::get_lastfm_cached(conn, &artist_key(id))
        .and_then(|data| serde_json::from_str::<About>(&data).ok())
        .filter(|about| about.source.is_some())
}

async fn blocking<T: Send + 'static>(
    db: &DbPool,
    f: impl FnOnce(&rusqlite::Connection) -> T + Send + 'static,
) -> Option<T> {
    let db = db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db.lock().unwrap_or_else(|p| p.into_inner());
        f(&conn)
    })
    .await
    .ok()
}

async fn cache(db: &DbPool, key: String, about: &About, failed: bool) {
    // A lookup that failed and found nothing is asked again next time.
    if failed && about.source.is_none() && about.stats.is_none() {
        return;
    }
    let days = if about.source.is_some() {
        FOUND_DAYS
    } else {
        EMPTY_DAYS
    };
    if let Ok(data) = serde_json::to_string(about) {
        blocking(db, move |conn| {
            boogiebox_db::artwork::save_lastfm_cache(conn, &key, &data, days)
        })
        .await;
    }
}

struct ArtistRow {
    name: String,
    mbid: Option<String>,
    cached: Option<String>,
    lastfm: Option<String>,
}

/// An artist's bio and stats (`None`: no such artist).
pub async fn artist_about(db: &DbPool, http: &Client, id: &EntityId) -> Option<About> {
    let lookup = id.clone();
    let row = blocking(db, move |conn| -> Option<ArtistRow> {
        let name: String = conn
            .query_row("SELECT name FROM artists WHERE id = ?1", [&lookup], |r| {
                r.get(0)
            })
            .ok()?;
        Some(ArtistRow {
            mbid: boogiebox_db::radio::get_artist_musicbrainz_id(conn, &lookup)
                .ok()
                .flatten(),
            cached: boogiebox_db::artwork::get_lastfm_cached(conn, &artist_key(&lookup)),
            lastfm: boogiebox_db::artwork::get_lastfm_cached_stale(
                conn,
                &format!("artist:{}", name.to_lowercase()),
            ),
            name,
        })
    })
    .await
    .flatten()?;
    if let Some(about) = row
        .cached
        .and_then(|data| serde_json::from_str::<About>(&data).ok())
    {
        return Some(about);
    }
    let mbid = row.mbid.filter(|m| is_valid_mbid(m));
    let mut failed = false;
    let mut note = |result: Result<Option<Text>, ProviderFetchError>| match result {
        Ok(text) => text,
        Err(error) => {
            tracing::debug!("artist about: {error}");
            failed = true;
            None
        }
    };
    let stats = match mbid.as_deref() {
        Some(mbid) => listenbrainz_artist_stats(http, mbid).await.ok().flatten(),
        None => None,
    };
    let wikipedia = match mbid.as_deref() {
        Some(mbid) => note(wikipedia_text(http, "artist", mbid).await),
        None => None,
    };
    let about = if let Some(text) = wikipedia {
        about_from("wikipedia", text, stats)
    } else if let Some(text) = note(theaudiodb_artist(http, mbid.as_deref(), &row.name).await) {
        about_from("theaudiodb", text, stats)
    } else if let Some(text) = from_lastfm(row.lastfm) {
        about_from("lastfm", text, stats)
    } else {
        About {
            stats,
            ..About::default()
        }
    };
    cache(db, artist_key(id), &about, failed).await;
    Some(about)
}

struct AlbumRow {
    title: String,
    artist: String,
    release_group: Option<String>,
    cached: Option<String>,
    lastfm: Option<String>,
}

/// An album's description (`None`: no such album).
pub async fn album_about(db: &DbPool, http: &Client, id: &EntityId) -> Option<About> {
    let lookup = id.clone();
    let row = blocking(db, move |conn| -> Option<AlbumRow> {
        let (title, artist, release_group): (String, String, Option<String>) = conn
            .query_row(
                "SELECT al.title, COALESCE(NULLIF(TRIM(al.album_artist), ''), ar.name, ''),
                        al.musicbrainz_release_group_id
                 FROM albums al LEFT JOIN artists ar ON ar.id = al.artist_id
                 WHERE al.id = ?1",
                [&lookup],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok()?;
        Some(AlbumRow {
            cached: boogiebox_db::artwork::get_lastfm_cached(conn, &album_key(&lookup)),
            lastfm: boogiebox_db::artwork::get_lastfm_cached_stale(
                conn,
                &format!("album:{}:{}", artist.to_lowercase(), title.to_lowercase()),
            ),
            title,
            artist,
            release_group,
        })
    })
    .await
    .flatten()?;
    if let Some(about) = row
        .cached
        .and_then(|data| serde_json::from_str::<About>(&data).ok())
    {
        return Some(about);
    }
    let release_group = row.release_group.filter(|m| is_valid_mbid(m));
    let mut failed = false;
    let mut note = |result: Result<Option<Text>, ProviderFetchError>| match result {
        Ok(text) => text,
        Err(error) => {
            tracing::debug!("album about: {error}");
            failed = true;
            None
        }
    };
    let wikipedia = match release_group.as_deref() {
        Some(rg) => note(wikipedia_text(http, "release-group", rg).await),
        None => None,
    };
    let about = if let Some(text) = wikipedia {
        about_from("wikipedia", text, None)
    } else if row.title.trim().is_empty() || row.artist.trim().is_empty() {
        About::default()
    } else if let Some(text) =
        note(theaudiodb_album(http, release_group.as_deref(), &row.artist, &row.title).await)
    {
        about_from("theaudiodb", text, None)
    } else if let Some(text) = from_lastfm(row.lastfm) {
        about_from("lastfm", text, None)
    } else {
        About::default()
    };
    cache(db, album_key(id), &about, failed).await;
    Some(about)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikipedia_titles_are_decoded() {
        assert_eq!(percent_decode("Sigur_R%C3%B3s"), "Sigur_Rós");
        assert_eq!(percent_decode("AC%2FDC"), "AC/DC");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn a_long_text_splits_into_its_first_paragraph_and_the_rest() {
        assert_eq!(split_text("  "), None);
        assert_eq!(split_text("One line."), Some(("One line.".into(), None)));
        assert_eq!(
            split_text("First para.\r\n\r\nSecond para."),
            Some((
                "First para.".into(),
                Some("First para.\n\nSecond para.".into())
            ))
        );
    }
}
