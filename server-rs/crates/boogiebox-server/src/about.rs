//! Artist bios and album descriptions without a key (`wip/lastfm-replacement-plan.md` phase 2): the English
//! Wikipedia summary reached through MusicBrainz → Wikidata links (never a name search, so no disambiguation
//! pages), then TheAudioDB with its free key. An album whose files carry no MusicBrainz tag gets its release group
//! from one clear MusicBrainz search match, kept on the album.
//! Artist stats are ListenBrainz listen and listener counts.

use std::time::Duration;

use boogiebox_db::music::EntityId;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::musicbrainz_gate::{self, Priority};
use crate::providers::urlencoding;
use crate::providers::{
    check_metabrainz_status, is_valid_mbid, listenbrainz_api_base, lucene_terms,
    musicbrainz_api_base, normalize_artist_name, optional_nonempty_json_string, ProviderFetchError,
    MUSICBRAINZ_USER_AGENT,
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
    /// `wikipedia` or `theaudiodb`; `None` when no source had a text.
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
    let response = musicbrainz_gate::send(get(client, &url), Priority::Foreground)
        .await
        .map_err(failed)?;
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

/// An album title without its bracketed extras ("Brotherhood [Collector's Edition]" → "Brotherhood").
fn title_without_brackets(title: &str) -> String {
    let mut depth = 0usize;
    title
        .chars()
        .filter(|c| match c {
            '(' | '[' => {
                depth += 1;
                false
            }
            ')' | ']' => {
                depth = depth.saturating_sub(1);
                false
            }
            _ => depth == 0,
        })
        .collect()
}

/// A title as release groups are compared: `normalize_title`, without a trailing "EP" ("Rainmaker Ep" is
/// MusicBrainz's "Rainmaker").
fn album_title_key(title: &str) -> String {
    let trimmed = title.trim_end();
    let lower = trimmed.to_lowercase();
    let without_ep = [" ep", " e.p.", " e.p"]
        .iter()
        .find_map(|suffix| {
            lower
                .ends_with(suffix)
                .then(|| &trimmed[..trimmed.len() - suffix.len()])
        })
        .filter(|rest| !rest.trim().is_empty())
        .unwrap_or(trimmed);
    boogiebox_db::popularity::normalize_title(without_ep)
}

/// The release group in a MusicBrainz search answer that is clearly this album: the same title (as
/// `normalize_title` reduces both), credited to `artist` (unless the search was by artist id), of the album's own
/// type. Of several, the one without secondary types (live, remix…) counts; a compilation must be marked one.
/// `None` when nothing or more than one fits.
fn pick_release_group(
    data: &Value,
    title: &str,
    artist: Option<&str>,
    release_type: Option<&str>,
) -> Option<String> {
    let wanted_title = album_title_key(title);
    let wanted_artist = artist.map(normalize_artist_name);
    let (primary, compilation) = match release_type.map(str::trim) {
        Some("ep") => ("ep", false),
        Some("single") => ("single", false),
        Some("compilation") => ("album", true),
        _ => ("album", false),
    };
    let secondary = |group: &Value| -> Vec<String> {
        group["secondary-types"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t.as_str().map(str::to_ascii_lowercase))
            .collect()
    };
    let fits: Vec<&Value> = data["release-groups"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|group| {
            group["title"]
                .as_str()
                .is_some_and(|t| album_title_key(t) == wanted_title)
                && group["primary-type"]
                    .as_str()
                    .is_some_and(|t| t.eq_ignore_ascii_case(primary))
                && wanted_artist.as_deref().is_none_or(|wanted| {
                    group["artist-credit"].as_array().is_some_and(|credits| {
                        credits.iter().any(|c| {
                            c["name"]
                                .as_str()
                                .or_else(|| c["artist"]["name"].as_str())
                                .is_some_and(|n| normalize_artist_name(n) == wanted)
                        })
                    })
                })
        })
        .collect();
    let preferred: Vec<&Value> = fits
        .iter()
        .copied()
        .filter(|group| {
            let types = secondary(group);
            if compilation {
                types.iter().any(|t| t == "compilation")
            } else {
                types.is_empty()
            }
        })
        .collect();
    let chosen = match (preferred.len(), fits.len()) {
        (1, _) => preferred[0],
        (0, 1) if !compilation => fits[0],
        _ => return None,
    };
    chosen["id"]
        .as_str()
        .filter(|id| is_valid_mbid(id))
        .map(str::to_ascii_lowercase)
}

/// The release group of an album whose files carry no MusicBrainz tag, by title and artist (the artist's
/// MusicBrainz id when known). See [`pick_release_group`].
async fn search_release_group(
    client: &Client,
    artist: &str,
    artist_mbid: Option<&str>,
    title: &str,
    release_type: Option<&str>,
) -> Result<Option<String>, ProviderFetchError> {
    let terms = lucene_terms(&title_without_brackets(title));
    let artist_name: String = artist
        .chars()
        .filter(|c| !matches!(c, '"' | '\\'))
        .collect::<String>()
        .trim()
        .to_owned();
    if terms.is_empty() || (artist_mbid.is_none() && artist_name.is_empty()) {
        return Ok(None);
    }
    let query = match artist_mbid {
        Some(mbid) => format!("releasegroup:({terms}) AND arid:{mbid}"),
        None => format!("releasegroup:({terms}) AND artist:\"{artist_name}\""),
    };
    let url = format!(
        "{}/release-group/?query={}&fmt=json&limit=10",
        musicbrainz_api_base(),
        urlencoding::encode(&query)
    );
    let response = musicbrainz_gate::send(get(client, &url), Priority::Foreground)
        .await
        .map_err(failed)?;
    check_metabrainz_status(&response)?;
    let data: Value = response.json().await.map_err(failed)?;
    let by_name = artist_mbid.is_none().then_some(artist_name.as_str());
    Ok(pick_release_group(&data, title, by_name, release_type))
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

/// TheAudioDB's English text: `<field>EN` where an entry still has it, else `<field>` (where the API now puts the
/// English text; `<field>FR`, `<field>ES`… hold the other languages).
fn english_text(entry: &Value, field: &str) -> Option<String> {
    optional_nonempty_json_string(&entry[format!("{field}EN")])
        .or_else(|| optional_nonempty_json_string(&entry[field]))
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
        let (summary, full) = split_text(&english_text(artist, "strBiography")?)?;
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
        let (summary, full) = split_text(&english_text(album, "strDescription")?)?;
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

/// The cached text for an album, without asking anyone (Subsonic `getAlbumInfo`).
pub fn cached_album_about(conn: &rusqlite::Connection, id: &EntityId) -> Option<About> {
    boogiebox_db::artwork::get_lastfm_cached(conn, &album_key(id))
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
    keyless: bool,
}

/// An artist's bio and stats (`None`: no such artist). With the main metadata switch off only a saved answer is
/// shown; nobody is asked.
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
            keyless: crate::radio_metadata::keyless_enabled(conn),
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
    if !row.keyless {
        return Some(About::default());
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
    /// The album artist's MusicBrainz id, when the album artist is that artist.
    artist_mbid: Option<String>,
    release_type: Option<String>,
    release_group: Option<String>,
    cached: Option<String>,
    keyless: bool,
}

/// An album's description (`None`: no such album). With the main metadata switch off only a saved answer is shown.
pub async fn album_about(db: &DbPool, http: &Client, id: &EntityId) -> Option<About> {
    let lookup = id.clone();
    let row = blocking(db, move |conn| -> Option<AlbumRow> {
        type Columns = (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let (title, artist, artist_name, artist_mbid, release_type, release_group): Columns = conn
            .query_row(
                "SELECT al.title, COALESCE(NULLIF(TRIM(al.album_artist), ''), ar.name, ''), ar.name,
                        COALESCE(NULLIF(TRIM(ar.musicbrainz_artist_id), ''), NULLIF(TRIM(ar.lastfm_mbid), '')),
                        al.release_type, al.musicbrainz_release_group_id
                 FROM albums al LEFT JOIN artists ar ON ar.id = al.artist_id
                 WHERE al.id = ?1",
                [&lookup],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .ok()?;
        let same_artist = artist_name.is_some_and(|name| {
            normalize_artist_name(&name) == normalize_artist_name(&artist)
        });
        Some(AlbumRow {
            cached: boogiebox_db::artwork::get_lastfm_cached(conn, &album_key(&lookup)),
            keyless: crate::radio_metadata::keyless_enabled(conn),
            artist_mbid: artist_mbid.filter(|m| same_artist && is_valid_mbid(m)),
            title,
            artist,
            release_type,
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
    if !row.keyless {
        return Some(About::default());
    }
    let mut failed = false;
    let mut release_group = row.release_group.filter(|m| is_valid_mbid(m));
    if release_group.is_none() && !row.title.trim().is_empty() && !row.artist.trim().is_empty() {
        match search_release_group(
            http,
            &row.artist,
            row.artist_mbid.as_deref(),
            &row.title,
            row.release_type.as_deref(),
        )
        .await
        {
            Ok(Some(found)) => {
                // Kept like a tag from the files (a scan fills only an empty one).
                let (album_id, id) = (id.clone(), found.clone());
                blocking(db, move |conn| {
                    conn.execute(
                        "UPDATE albums SET musicbrainz_release_group_id = ?1
                         WHERE id = ?2 AND musicbrainz_release_group_id IS NULL",
                        rusqlite::params![id, album_id],
                    )
                })
                .await;
                release_group = Some(found);
            }
            Ok(None) => {}
            Err(error) => {
                tracing::debug!("album about: release group search: {error}");
                failed = true;
            }
        }
    }
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
    } else {
        About::default()
    };
    cache(db, album_key(id), &about, failed).await;
    Some(about)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(id: &str, title: &str, primary: &str, secondary: &[&str], artist: &str) -> Value {
        serde_json::json!({
            "id": id, "title": title, "primary-type": primary, "secondary-types": secondary,
            "artist-credit": [{ "name": artist }]
        })
    }

    const ALBUM: &str = "017f2a37-a78f-3578-9611-fa40408e5d90";
    const SINGLE: &str = "ecc129fd-419f-49ef-ac02-391cd8ef5c39";
    const LIVE: &str = "bfb2d112-e8be-4a07-a15b-64253c761833";
    const BEST_OF: &str = "33f5fa70-4715-4f8d-84e7-b962ee43f3b2";

    #[test]
    fn the_release_group_of_the_albums_own_type_is_picked() {
        let data = serde_json::json!({ "release-groups": [
            group(SINGLE, "Hounds of Love", "Single", &[], "Kate Bush"),
            group(ALBUM, "Hounds of Love", "Album", &[], "Kate Bush"),
            group(LIVE, "Hounds of Love", "Album", &["Live"], "Kate Bush"),
            group(BEST_OF, "Hounds of Love / Lionheart", "Album", &["Compilation"], "Kate Bush"),
        ]});
        let pick = |title, artist, kind| pick_release_group(&data, title, artist, kind);
        assert_eq!(
            pick("Hounds Of Love", Some("Kate Bush"), Some("album")).as_deref(),
            Some(ALBUM)
        );
        assert_eq!(
            pick("Hounds Of Love [2018 Remaster]", None, None).as_deref(),
            Some(ALBUM)
        );
        assert_eq!(
            pick("Hounds Of Love", Some("Kate Bush"), Some("single")).as_deref(),
            Some(SINGLE)
        );
        assert_eq!(
            pick("Hounds Of Love", Some("Someone Else"), Some("album")),
            None
        );
        assert_eq!(pick("Hounds Of Love", Some("Kate Bush"), Some("ep")), None);
        assert_eq!(pick("The Dreaming", Some("Kate Bush"), Some("album")), None);
        // A compilation must be marked one; a studio album of the same name doesn't count.
        assert_eq!(
            pick("Hounds of Love / Lionheart", None, Some("compilation")).as_deref(),
            Some(BEST_OF)
        );
        assert_eq!(pick("Hounds Of Love", None, Some("compilation")), None);
    }

    #[test]
    fn several_equal_matches_or_only_a_live_one_are_handled() {
        let two = serde_json::json!({ "release-groups": [
            group(ALBUM, "Greatest Hits", "Album", &[], "Eurythmics"),
            group(SINGLE, "Greatest Hits", "Album", &[], "Eurythmics"),
        ]});
        assert_eq!(
            pick_release_group(&two, "Greatest Hits", None, Some("album")),
            None
        );
        let live_only = serde_json::json!({ "release-groups": [
            group(LIVE, "Live at Wembley", "Album", &["Live"], "Queen"),
        ]});
        assert_eq!(
            pick_release_group(&live_only, "Live At Wembley", Some("Queen"), Some("album"))
                .as_deref(),
            Some(LIVE)
        );
        assert_eq!(
            pick_release_group(&serde_json::json!({}), "Anything", None, None),
            None
        );
        let bad_id = serde_json::json!({ "release-groups": [group("../x", "Dummy", "Album", &[], "Portishead")] });
        assert_eq!(pick_release_group(&bad_id, "Dummy", None, None), None);
    }

    #[test]
    fn a_trailing_ep_is_not_part_of_the_title() {
        const EP: &str = "0b6c4f5e-1d2a-4c3b-9e8f-7a6b5c4d3e2f";
        let data = serde_json::json!({ "release-groups": [group(EP, "Rainmaker", "EP", &[], "Recondite")] });
        assert_eq!(
            pick_release_group(&data, "Rainmaker Ep", None, Some("ep")).as_deref(),
            Some(EP)
        );
        assert_eq!(
            pick_release_group(&data, "Rainmaker E.P.", None, Some("ep")).as_deref(),
            Some(EP)
        );
        assert_eq!(
            album_title_key("Ep"),
            album_title_key("EP"),
            "a title that is only EP stays"
        );
        assert_ne!(album_title_key("Deep"), album_title_key("De"));
    }

    #[test]
    fn theaudiodb_english_text_is_read_with_or_without_the_en_suffix() {
        let now = serde_json::json!({ "strBiography": "English.", "strBiographyFR": "Français." });
        assert_eq!(
            english_text(&now, "strBiography").as_deref(),
            Some("English.")
        );
        let old =
            serde_json::json!({ "strDescriptionEN": "Old field.", "strDescription": "New field." });
        assert_eq!(
            english_text(&old, "strDescription").as_deref(),
            Some("Old field.")
        );
        let empty = serde_json::json!({ "strBiographyEN": "", "strBiography": "  " });
        assert_eq!(english_text(&empty, "strBiography"), None);
    }

    #[test]
    fn bracketed_extras_leave_the_title() {
        assert_eq!(
            title_without_brackets("Brotherhood [Collector's Edition]").trim(),
            "Brotherhood"
        );
        assert_eq!(
            title_without_brackets("Substance (2023 Expanded / Reissue)").trim(),
            "Substance"
        );
        assert_eq!(title_without_brackets("Dummy"), "Dummy");
    }

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
