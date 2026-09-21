//! Artist Radio v2 storage and candidate queries (`wip/artist-radio-v2-plan.md` §2–§4).
//!
//! This module only stores provider tags and fetches candidate pools; all
//! ranking/selection lives in the server crate's `artist_radio` so it stays
//! pure and unit-testable.

use crate::music::{id_to_value, map_track, EntityId, TrackRow, TRACK_COLS};
use rusqlite::{params, params_from_iter, types::Value, Connection};
use std::collections::{HashMap, HashSet};

/// Provider name stored in `track_tags.source` / `track_tag_sync.source`.
pub const SOURCE_LASTFM: &str = "lastfm";
/// Provider name for keyless MusicBrainz artist tags.
pub const SOURCE_MUSICBRAINZ: &str = "musicbrainz";

/// Re-check a track that returned tags after this many days.
pub const TRACK_TAG_OK_TTL_DAYS: i64 = 365;
/// Re-check a track that returned no tags after this many days (negative cache).
pub const TRACK_TAG_EMPTY_TTL_DAYS: i64 = 90;

const IN_CHUNK: usize = 400;

/// One normalized, classified tag ready to persist.
#[derive(Debug, Clone, PartialEq)]
pub struct TagInput {
    pub tag: String,
    /// `genre` | `mood` | `era`.
    pub kind: String,
    /// Mood bucket, only for `kind == "mood"`.
    pub bucket: Option<String>,
    /// 0..1, relative to the strongest tag in the same provider response.
    pub weight: f64,
}

/// A tag with the weight it carries for a track/artist.
#[derive(Debug, Clone, PartialEq)]
pub struct TagWeight {
    pub tag: String,
    pub weight: f64,
}

/// The most specific metadata layer a candidate's tag vector was built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TagLayer {
    None,
    Artist,
    Album,
    Track,
}

/// A track eligible for provider tag lookup.
#[derive(Debug, Clone, PartialEq)]
pub struct TagCandidate {
    pub track_id: EntityId,
    pub artist_id: Option<EntityId>,
    pub artist: String,
    pub title: String,
}

/// A radio candidate with everything the ranking engine needs.
#[derive(Debug, Clone)]
pub struct RadioTrack {
    pub track: TrackRow,
    pub artist_id: Option<EntityId>,
    /// Effective tag vector: track tags, plus album styles and artist tags as fallback layers.
    pub tags: Vec<TagWeight>,
    pub tag_layer: TagLayer,
    /// The user's track > album > artist rating, 0.5..=5.
    pub rating: Option<f64>,
    /// `track_deep_analysis.energy_score_refined`, only for analyzed tracks.
    pub energy: Option<f64>,
}

/// Aggregate audio features of a seed artist's library.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SeedAudio {
    pub bpm: Option<f64>,
    pub energy: Option<f64>,
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

// ── Track tags ──────────────────────────────────────────────────────────────

/// Replaces one provider's tags for a track and stamps the sync cursor:
/// `ok` when tags were stored, `empty` (negative cache) when the provider had none.
pub fn replace_track_tags(
    conn: &Connection,
    track_id: &EntityId,
    source: &str,
    tags: &[TagInput],
) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM track_tags WHERE track_id=?1 AND source=?2",
        params![track_id, source],
    )?;
    for tag in tags {
        tx.execute(
            "INSERT OR REPLACE INTO track_tags(track_id, tag, kind, bucket, weight, source)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![track_id, tag.tag, tag.kind, tag.bucket, tag.weight, source],
        )?;
    }
    tx.execute(
        "INSERT INTO track_tag_sync(track_id, source, checked_at, status)
         VALUES(?1, ?2, datetime('now'), ?3)
         ON CONFLICT(track_id, source) DO UPDATE SET
           checked_at=excluded.checked_at, status=excluded.status",
        params![
            track_id,
            source,
            if tags.is_empty() { "empty" } else { "ok" }
        ],
    )?;
    tx.commit()
}

const NEEDS_TAGS_FILTER: &str = "(s.track_id IS NULL
       OR (s.status='ok' AND s.checked_at < datetime('now', ?2))
       OR (s.status<>'ok' AND s.checked_at < datetime('now', ?3)))";

fn map_tag_candidate(row: &rusqlite::Row<'_>) -> rusqlite::Result<TagCandidate> {
    Ok(TagCandidate {
        track_id: row.get(0)?,
        artist_id: row.get(1)?,
        artist: row.get(2)?,
        title: row.get(3)?,
    })
}

/// Tracks whose tags were never fetched (or whose cursor expired), most-played first.
pub fn list_tracks_needing_tags(
    conn: &Connection,
    source: &str,
    limit: i64,
) -> rusqlite::Result<Vec<TagCandidate>> {
    let sql = format!(
        "SELECT t.id, t.artist_id, ar.name, t.title
         FROM tracks t
         JOIN artists ar ON ar.id = t.artist_id
         LEFT JOIN track_tag_sync s ON s.track_id = t.id AND s.source = ?1
         WHERE TRIM(ar.name) <> '' AND TRIM(t.title) <> '' AND {NEEDS_TAGS_FILTER}
         ORDER BY (t.last_played_at IS NULL), t.last_played_at DESC, t.play_count DESC, t.id
         LIMIT ?4"
    );
    conn.prepare(&sql)?
        .query_map(
            params![
                source,
                format!("-{TRACK_TAG_OK_TTL_DAYS} days"),
                format!("-{TRACK_TAG_EMPTY_TTL_DAYS} days"),
                limit
            ],
            map_tag_candidate,
        )?
        .collect()
}

/// Same eligibility as [`list_tracks_needing_tags`], restricted to `track_ids`
/// (the lazy, radio-launch path).
pub fn list_untagged_among(
    conn: &Connection,
    source: &str,
    track_ids: &[EntityId],
    limit: usize,
) -> rusqlite::Result<Vec<TagCandidate>> {
    let mut out = Vec::new();
    for chunk in track_ids.chunks(IN_CHUNK) {
        if out.len() >= limit {
            break;
        }
        let sql = format!(
            "SELECT t.id, t.artist_id, ar.name, t.title
             FROM tracks t
             JOIN artists ar ON ar.id = t.artist_id
             LEFT JOIN track_tag_sync s ON s.track_id = t.id AND s.source = ?1
             WHERE TRIM(ar.name) <> '' AND TRIM(t.title) <> '' AND {NEEDS_TAGS_FILTER}
               AND t.id IN ({})",
            placeholders(chunk.len())
        );
        let mut values = vec![
            Value::Text(source.to_owned()),
            Value::Text(format!("-{TRACK_TAG_OK_TTL_DAYS} days")),
            Value::Text(format!("-{TRACK_TAG_EMPTY_TTL_DAYS} days")),
        ];
        values.extend(chunk.iter().map(id_to_value));
        let rows: Vec<TagCandidate> = conn
            .prepare(&sql)?
            .query_map(params_from_iter(values), map_tag_candidate)?
            .collect::<rusqlite::Result<_>>()?;
        out.extend(rows);
    }
    out.truncate(limit);
    Ok(out)
}

/// `(checked, total)` for the sidebar/progress line: tracks with a fresh sync
/// cursor for `source` versus all taggable tracks.
pub fn track_tag_progress(conn: &Connection, source: &str) -> rusqlite::Result<(i64, i64)> {
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tracks t JOIN artists ar ON ar.id = t.artist_id
         WHERE TRIM(ar.name) <> '' AND TRIM(t.title) <> ''",
        [],
        |row| row.get(0),
    )?;
    let checked: i64 = conn.query_row(
        "SELECT COUNT(*) FROM track_tag_sync s JOIN tracks t ON t.id = s.track_id
         WHERE s.source = ?1",
        [source],
        |row| row.get(0),
    )?;
    Ok((checked.min(total), total))
}

// ── Artist tags / identity ──────────────────────────────────────────────────

fn insert_artist_tag(
    conn: &Connection,
    artist_id: &EntityId,
    tag: &TagInput,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO artist_styles(artist_id, style, kind, bucket, weight, updated_at)
         VALUES(?1, ?2, ?3, ?4, ?5, datetime('now'))",
        params![artist_id, tag.tag, tag.kind, tag.bucket, tag.weight],
    )?;
    Ok(())
}

/// Replaces every tag of an artist (Last.fm is authoritative when it has data).
pub fn replace_artist_tags(
    conn: &Connection,
    artist_id: &EntityId,
    tags: &[TagInput],
) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM artist_styles WHERE artist_id=?1", [artist_id])?;
    for tag in tags {
        insert_artist_tag(&tx, artist_id, tag)?;
    }
    tx.commit()
}

/// Writes tags only when the artist has none yet (keyless MusicBrainz fill).
/// Returns whether anything was written.
pub fn insert_artist_tags_if_none(
    conn: &Connection,
    artist_id: &EntityId,
    tags: &[TagInput],
) -> rusqlite::Result<bool> {
    if tags.is_empty() {
        return Ok(false);
    }
    let existing: i64 = conn.query_row(
        "SELECT COUNT(*) FROM artist_styles WHERE artist_id=?1",
        [artist_id],
        |row| row.get(0),
    )?;
    if existing > 0 {
        return Ok(false);
    }
    let tx = conn.unchecked_transaction()?;
    for tag in tags {
        insert_artist_tag(&tx, artist_id, tag)?;
    }
    tx.commit()?;
    Ok(true)
}

/// Replaces an album's Discogs genre/style tags.
pub fn replace_album_styles(
    conn: &Connection,
    album_id: &EntityId,
    source: &str,
    styles: &[(String, String)],
) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM album_styles WHERE album_id=?1 AND source=?2",
        params![album_id, source],
    )?;
    for (tag, kind) in styles {
        tx.execute(
            "INSERT OR REPLACE INTO album_styles(album_id, tag, kind, source)
             VALUES(?1, ?2, ?3, ?4)",
            params![album_id, tag, kind, source],
        )?;
    }
    tx.commit()
}

/// Stores the resolved MusicBrainz artist id (or just stamps the check when
/// `mbid` is `None`, so an unresolvable artist isn't re-queried every pass).
pub fn set_musicbrainz_identity(
    conn: &Connection,
    artist_id: &EntityId,
    mbid: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE artists
         SET musicbrainz_artist_id = COALESCE(?1, musicbrainz_artist_id),
             musicbrainz_identity_checked_at = datetime('now')
         WHERE id = ?2",
        params![mbid.map(str::trim).filter(|v| !v.is_empty()), artist_id],
    )?;
    Ok(())
}

/// The artist's MusicBrainz id: the keyless-resolved one, else Last.fm's `mbid`
/// (which is the same namespace).
pub fn get_artist_musicbrainz_id(
    conn: &Connection,
    artist_id: &EntityId,
) -> rusqlite::Result<Option<String>> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT COALESCE(NULLIF(TRIM(musicbrainz_artist_id), ''), NULLIF(TRIM(lastfm_mbid), ''))
         FROM artists WHERE id = ?1",
        [artist_id],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .map(Option::flatten)
}

/// Artists with no MusicBrainz id and no fresh negative check, busiest first.
pub fn list_artists_needing_musicbrainz_id(
    conn: &Connection,
    stale_days: i64,
    limit: i64,
) -> rusqlite::Result<Vec<(EntityId, String, Option<String>)>> {
    conn.prepare(
        "SELECT ar.id, ar.name, ar.lastfm_mbid
         FROM artists ar
         WHERE ar.musicbrainz_artist_id IS NULL
           AND TRIM(ar.name) <> ''
           AND EXISTS (SELECT 1 FROM tracks t WHERE t.artist_id = ar.id)
           AND (ar.musicbrainz_identity_checked_at IS NULL
                OR ar.musicbrainz_identity_checked_at < datetime('now', ?1))
         ORDER BY ar.play_count DESC, ar.track_count DESC, ar.id
         LIMIT ?2",
    )?
    .query_map(params![format!("-{stale_days} days"), limit], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?
    .collect()
}

/// Artists that have a MusicBrainz id, no tags yet, and no fresh MB tag cache entry.
pub fn list_artists_needing_musicbrainz_tags(
    conn: &Connection,
    limit: i64,
) -> rusqlite::Result<Vec<(EntityId, String)>> {
    conn.prepare(
        "SELECT ar.id, ar.musicbrainz_artist_id
         FROM artists ar
         WHERE ar.musicbrainz_artist_id IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM artist_styles s WHERE s.artist_id = ar.id)
           AND NOT EXISTS (SELECT 1 FROM lastfm_cache c
                           WHERE c.cache_key = 'mb-artist-tags:' || ar.id
                             AND c.expires_at > datetime('now'))
         ORDER BY ar.play_count DESC, ar.track_count DESC, ar.id
         LIMIT ?1",
    )?
    .query_map([limit], |row| Ok((row.get(0)?, row.get(1)?)))?
    .collect()
}

// ── Seed profile ────────────────────────────────────────────────────────────

/// The seed artist's weighted tag vector: its artist tags merged (max) with
/// the average weight of its tracks' own tags. Strongest first, ≤ 30 entries.
pub fn seed_tag_vector(
    conn: &Connection,
    artist_id: &EntityId,
) -> rusqlite::Result<Vec<TagWeight>> {
    let mut merged: HashMap<String, f64> = HashMap::new();
    let styles: Vec<(String, Option<f64>)> = conn
        .prepare("SELECT style, weight FROM artist_styles WHERE artist_id = ?1")?
        .query_map([artist_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (style, weight) in styles {
        let w = weight.unwrap_or(0.6).clamp(0.05, 1.0);
        merged
            .entry(style.to_lowercase())
            .and_modify(|v| *v = v.max(w))
            .or_insert(w);
    }
    let tagged_tracks: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT tt.track_id) FROM track_tags tt
         JOIN tracks t ON t.id = tt.track_id WHERE t.artist_id = ?1",
        [artist_id],
        |row| row.get(0),
    )?;
    if tagged_tracks > 0 {
        let rows: Vec<(String, f64)> = conn
            .prepare(
                "SELECT tt.tag, SUM(tt.weight) FROM track_tags tt
                 JOIN tracks t ON t.id = tt.track_id
                 WHERE t.artist_id = ?1
                 GROUP BY tt.tag ORDER BY SUM(tt.weight) DESC LIMIT 30",
            )?
            .query_map([artist_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (tag, sum) in rows {
            let w = (sum / tagged_tracks as f64).clamp(0.0, 1.0);
            merged
                .entry(tag.to_lowercase())
                .and_modify(|v| *v = v.max(w))
                .or_insert(w);
        }
    }
    let mut out: Vec<TagWeight> = merged
        .into_iter()
        .map(|(tag, weight)| TagWeight { tag, weight })
        .collect();
    out.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.tag.cmp(&b.tag))
    });
    out.truncate(30);
    Ok(out)
}

/// Mean BPM and deep-analysis energy across an artist's tracks.
pub fn seed_audio_profile(conn: &Connection, artist_id: &EntityId) -> rusqlite::Result<SeedAudio> {
    conn.query_row(
        "SELECT AVG(NULLIF(t.bpm, 0)), AVG(da.energy_score_refined)
         FROM tracks t
         LEFT JOIN track_deep_analysis da ON da.track_id = t.id AND da.confidence > 0.25
         WHERE t.artist_id = ?1",
        [artist_id],
        |row| {
            Ok(SeedAudio {
                bpm: row.get(0)?,
                energy: row.get(1)?,
            })
        },
    )
}

// ── Candidate pools ─────────────────────────────────────────────────────────

/// Up to `per_artist` random track ids for each artist.
pub fn radio_ids_for_artists(
    conn: &Connection,
    artist_ids: &[EntityId],
    per_artist: i64,
) -> rusqlite::Result<Vec<EntityId>> {
    let mut out = Vec::new();
    for chunk in artist_ids.chunks(200) {
        let sql = format!(
            "WITH ranked AS (
               SELECT t.id AS id,
                      ROW_NUMBER() OVER (PARTITION BY t.artist_id ORDER BY RANDOM()) AS rn
               FROM tracks t WHERE t.artist_id IN ({})
             )
             SELECT id FROM ranked WHERE rn <= ?",
            placeholders(chunk.len())
        );
        let mut values: Vec<Value> = chunk.iter().map(id_to_value).collect();
        values.push(Value::Integer(per_artist));
        let ids: Vec<EntityId> = conn
            .prepare(&sql)?
            .query_map(params_from_iter(values), |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        out.extend(ids);
    }
    Ok(out)
}

/// Track ids whose track/album/artist tags overlap `tags`, best overlap first
/// (random among equals), excluding `exclude_artist`.
pub fn radio_ids_matching_tags(
    conn: &Connection,
    tags: &[String],
    exclude_artist: &EntityId,
    limit: i64,
) -> rusqlite::Result<Vec<EntityId>> {
    if tags.is_empty() {
        return Ok(Vec::new());
    }
    let tag_values = || -> Vec<Value> { tags.iter().map(|t| Value::Text(t.clone())).collect() };
    let in_tags = placeholders(tags.len());
    let mut seen: HashSet<EntityId> = HashSet::new();
    let mut out: Vec<EntityId> = Vec::new();
    let mut push = |ids: Vec<EntityId>, out: &mut Vec<EntityId>| {
        for id in ids {
            if seen.insert(id.clone()) {
                out.push(id);
            }
        }
    };

    // 1. Track-level tags — the most specific signal.
    let mut values = tag_values();
    values.push(id_to_value(exclude_artist));
    values.push(Value::Integer(limit));
    let ids: Vec<EntityId> = conn
        .prepare(&format!(
            "SELECT tt.track_id FROM track_tags tt JOIN tracks t ON t.id = tt.track_id
             WHERE tt.tag IN ({in_tags}) AND COALESCE(t.artist_id, '') <> ?
             GROUP BY tt.track_id ORDER BY COUNT(*) DESC, RANDOM() LIMIT ?"
        ))?
        .query_map(params_from_iter(values), |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    push(ids, &mut out);

    // 2. Album styles.
    let mut values = tag_values();
    values.push(id_to_value(exclude_artist));
    values.push(Value::Integer(limit));
    let ids: Vec<EntityId> = conn
        .prepare(&format!(
            "SELECT t.id FROM album_styles a JOIN tracks t ON t.album_id = a.album_id
             WHERE a.tag IN ({in_tags}) AND COALESCE(t.artist_id, '') <> ?
             GROUP BY t.id ORDER BY COUNT(*) DESC, RANDOM() LIMIT ?"
        ))?
        .query_map(params_from_iter(values), |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    push(ids, &mut out);

    // 3. Artist tags → a few random tracks from each best-overlapping artist.
    let mut values = tag_values();
    values.push(id_to_value(exclude_artist));
    let artist_ids: Vec<EntityId> = conn
        .prepare(&format!(
            "SELECT artist_id FROM artist_styles
             WHERE style IN ({in_tags}) AND artist_id <> ?
             GROUP BY artist_id ORDER BY COUNT(*) DESC, RANDOM() LIMIT 80"
        ))?
        .query_map(params_from_iter(values), |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let ids = radio_ids_for_artists(conn, &artist_ids, 6)?;
    push(ids, &mut out);

    Ok(out)
}

/// Number of distinct tracks carrying any of `tags` at track or artist level,
/// counted up to `cap` (enough to decide whether a mood is offerable).
pub fn count_tracks_with_any_tag(
    conn: &Connection,
    tags: &[String],
    cap: i64,
) -> rusqlite::Result<i64> {
    if tags.is_empty() {
        return Ok(0);
    }
    let in_tags = placeholders(tags.len());
    let sql = format!(
        "SELECT COUNT(*) FROM (
           SELECT track_id AS id FROM track_tags WHERE tag IN ({in_tags})
           UNION
           SELECT t.id FROM artist_styles s JOIN tracks t ON t.artist_id = s.artist_id
           WHERE s.style IN ({in_tags})
           LIMIT ?
         )"
    );
    let mut values: Vec<Value> = tags.iter().map(|t| Value::Text(t.clone())).collect();
    values.extend(tags.iter().map(|t| Value::Text(t.clone())));
    values.push(Value::Integer(cap));
    conn.query_row(&sql, params_from_iter(values), |row| row.get(0))
}

/// Days since this user last played each track, for plays within `days`.
pub fn recent_play_ages(
    conn: &Connection,
    user_id: &str,
    days: i64,
) -> rusqlite::Result<HashMap<EntityId, f64>> {
    let rows: Vec<(EntityId, f64)> = conn
        .prepare(
            "SELECT track_id, julianday('now') - julianday(MAX(played_at))
             FROM play_history
             WHERE user_id = ?1 AND played_at > datetime('now', ?2)
             GROUP BY track_id",
        )?
        .query_map(params![user_id, format!("-{days} days")], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows.into_iter().collect())
}

/// Loads full radio candidates (track row, rating, energy, effective tag vector).
pub fn load_radio_tracks(
    conn: &Connection,
    user_id: &str,
    ids: &[EntityId],
) -> rusqlite::Result<Vec<RadioTrack>> {
    let mut out: Vec<RadioTrack> = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(IN_CHUNK) {
        let sql = format!(
            "SELECT {TRACK_COLS},
                    COALESCE(trr.rating, alr.rating, arr.rating) AS rating,
                    t.artist_id, da.energy_score_refined
             FROM tracks t
             LEFT JOIN artists ar ON ar.id = t.artist_id
             LEFT JOIN albums al ON al.id = t.album_id
             LEFT JOIN libraries l ON l.id = t.library_id
             LEFT JOIN track_ratings trr ON trr.track_id = t.id AND trr.user_id = ?
             LEFT JOIN album_ratings alr ON alr.album_id = t.album_id AND alr.user_id = ?
             LEFT JOIN artist_ratings arr ON arr.artist_id = t.artist_id AND arr.user_id = ?
             LEFT JOIN track_deep_analysis da ON da.track_id = t.id AND da.confidence > 0.25
             WHERE t.id IN ({})",
            placeholders(chunk.len())
        );
        let mut values: Vec<Value> = vec![
            Value::Text(user_id.to_owned()),
            Value::Text(user_id.to_owned()),
            Value::Text(user_id.to_owned()),
        ];
        values.extend(chunk.iter().map(id_to_value));
        let rows: Vec<RadioTrack> = conn
            .prepare(&sql)?
            .query_map(params_from_iter(values), |row| {
                Ok(RadioTrack {
                    track: map_track(row)?,
                    rating: row.get(27)?,
                    artist_id: row.get(28)?,
                    energy: row.get(29)?,
                    tags: Vec::new(),
                    tag_layer: TagLayer::None,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        out.extend(rows);
    }
    attach_tags(conn, &mut out)?;
    Ok(out)
}

/// Fills each candidate's effective tag vector, most specific layer winning
/// per tag: track tags (1.0×) > album styles (0.6) > artist tags (0.7×).
fn attach_tags(conn: &Connection, tracks: &mut [RadioTrack]) -> rusqlite::Result<()> {
    let track_ids: Vec<EntityId> = tracks.iter().map(|t| t.track.id.clone()).collect();
    let mut album_ids: Vec<EntityId> = tracks
        .iter()
        .filter_map(|t| t.track.album_id.clone())
        .collect();
    album_ids.sort_by_key(|id| id.to_string());
    album_ids.dedup();
    let mut artist_ids: Vec<EntityId> = tracks.iter().filter_map(|t| t.artist_id.clone()).collect();
    artist_ids.sort_by_key(|id| id.to_string());
    artist_ids.dedup();

    let mut by_track: HashMap<EntityId, Vec<(String, f64)>> = HashMap::new();
    for chunk in track_ids.chunks(IN_CHUNK) {
        let sql = format!(
            "SELECT track_id, tag, weight FROM track_tags WHERE track_id IN ({})",
            placeholders(chunk.len())
        );
        let rows: Vec<(EntityId, String, f64)> = conn
            .prepare(&sql)?
            .query_map(params_from_iter(chunk.iter().map(id_to_value)), |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        for (id, tag, weight) in rows {
            by_track.entry(id).or_default().push((tag, weight));
        }
    }
    let mut by_album: HashMap<EntityId, Vec<String>> = HashMap::new();
    for chunk in album_ids.chunks(IN_CHUNK) {
        let sql = format!(
            "SELECT album_id, tag FROM album_styles WHERE album_id IN ({})",
            placeholders(chunk.len())
        );
        let rows: Vec<(EntityId, String)> = conn
            .prepare(&sql)?
            .query_map(params_from_iter(chunk.iter().map(id_to_value)), |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        for (id, tag) in rows {
            by_album.entry(id).or_default().push(tag);
        }
    }
    let mut by_artist: HashMap<EntityId, Vec<(String, f64)>> = HashMap::new();
    for chunk in artist_ids.chunks(IN_CHUNK) {
        let sql = format!(
            "SELECT artist_id, style, weight FROM artist_styles WHERE artist_id IN ({})",
            placeholders(chunk.len())
        );
        let rows: Vec<(EntityId, String, Option<f64>)> = conn
            .prepare(&sql)?
            .query_map(params_from_iter(chunk.iter().map(id_to_value)), |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        for (id, tag, weight) in rows {
            by_artist
                .entry(id)
                .or_default()
                .push((tag, weight.unwrap_or(0.6)));
        }
    }

    for candidate in tracks.iter_mut() {
        let mut merged: HashMap<String, f64> = HashMap::new();
        let mut layer = TagLayer::None;
        let mut put = |tag: &str, weight: f64| {
            let w = weight.clamp(0.0, 1.0);
            merged
                .entry(tag.to_lowercase())
                .and_modify(|v| *v = v.max(w))
                .or_insert(w);
        };
        if let Some(artist_tags) = candidate
            .artist_id
            .as_ref()
            .and_then(|id| by_artist.get(id))
        {
            for (tag, weight) in artist_tags {
                put(tag, weight * 0.7);
            }
            layer = TagLayer::Artist;
        }
        if let Some(album_tags) = candidate
            .track
            .album_id
            .as_ref()
            .and_then(|id| by_album.get(id))
        {
            for tag in album_tags {
                put(tag, 0.6);
            }
            layer = TagLayer::Album;
        }
        if let Some(track_tags) = by_track.get(&candidate.track.id) {
            for (tag, weight) in track_tags {
                put(tag, *weight);
            }
            layer = TagLayer::Track;
        }
        let mut vector: Vec<TagWeight> = merged
            .into_iter()
            .map(|(tag, weight)| TagWeight { tag, weight })
            .collect();
        vector.sort_by(|a, b| a.tag.cmp(&b.tag));
        candidate.tags = vector;
        candidate.tag_layer = layer;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::initialize_schema;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().expect("memory db");
        initialize_schema(&conn).expect("schema");
        conn.execute(
            "INSERT INTO libraries(id, path, name, library_type) VALUES('lib', 'D:/Music', 'Music', 'music')",
            [],
        )
        .expect("library");
        conn
    }

    fn artist(conn: &Connection, id: &str, name: &str) {
        conn.execute(
            "INSERT INTO artists(id, name) VALUES(?1, ?2)",
            params![id, name],
        )
        .expect("artist");
        conn.execute(
            "INSERT INTO albums(id, title, artist_id, album_artist) VALUES(?1, ?2, ?3, ?2)",
            params![format!("al-{id}"), format!("{name} LP"), id],
        )
        .expect("album");
    }

    fn track(conn: &Connection, id: &str, artist_id: &str, title: &str) {
        conn.execute(
            "INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_name, album_artist,
                                genre, composer, duration, file_path, file_size)
             VALUES(?1, 'lib', ?2, ?3, ?4, 'f.mp3', '', '', '', 200, ?5, 1)",
            params![id, artist_id, format!("al-{artist_id}"), title, format!("D:/Music/{id}.mp3")],
        )
        .expect("track");
    }

    fn tag(tag: &str, weight: f64) -> TagInput {
        TagInput {
            tag: tag.into(),
            kind: "genre".into(),
            bucket: None,
            weight,
        }
    }

    fn user(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT OR IGNORE INTO users(id, username, role) VALUES(?1, ?1, 'user')",
            [id],
        )
        .expect("user");
    }

    #[test]
    fn replace_track_tags_stamps_ok_and_empty_and_lists_only_stale_tracks() {
        let conn = db();
        artist(&conn, "a1", "Massive Attack");
        track(&conn, "t1", "a1", "Teardrop");
        track(&conn, "t2", "a1", "Angel");
        let id1 = EntityId::Str("t1".into());
        let id2 = EntityId::Str("t2".into());

        assert_eq!(
            list_tracks_needing_tags(&conn, SOURCE_LASTFM, 10)
                .unwrap()
                .len(),
            2
        );
        replace_track_tags(&conn, &id1, SOURCE_LASTFM, &[tag("trip-hop", 1.0)]).unwrap();
        replace_track_tags(&conn, &id2, SOURCE_LASTFM, &[]).unwrap();

        // Both are freshly stamped (ok / empty) so neither is due again.
        assert!(list_tracks_needing_tags(&conn, SOURCE_LASTFM, 10)
            .unwrap()
            .is_empty());
        let statuses: Vec<String> = conn
            .prepare("SELECT status FROM track_tag_sync ORDER BY track_id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(statuses, vec!["ok", "empty"]);
        // A different provider has its own cursor.
        assert_eq!(
            list_tracks_needing_tags(&conn, SOURCE_MUSICBRAINZ, 10)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(track_tag_progress(&conn, SOURCE_LASTFM).unwrap(), (2, 2));

        // Expired cursors come back: ok after 365 days, empty after 90.
        conn.execute(
            "UPDATE track_tag_sync SET checked_at = datetime('now','-100 days')",
            [],
        )
        .unwrap();
        let due = list_tracks_needing_tags(&conn, SOURCE_LASTFM, 10).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].track_id, id2);
    }

    #[test]
    fn list_tracks_needing_tags_prefers_recently_played_then_play_count() {
        let conn = db();
        artist(&conn, "a1", "Artist");
        for (id, plays) in [("t1", 1), ("t2", 9), ("t3", 5)] {
            track(&conn, id, "a1", id);
            conn.execute(
                "UPDATE tracks SET play_count=?1 WHERE id=?2",
                params![plays, id],
            )
            .unwrap();
        }
        conn.execute(
            "UPDATE tracks SET last_played_at = datetime('now') WHERE id='t3'",
            [],
        )
        .unwrap();
        let order: Vec<String> = list_tracks_needing_tags(&conn, SOURCE_LASTFM, 10)
            .unwrap()
            .into_iter()
            .map(|c| c.track_id.to_string())
            .collect();
        assert_eq!(order, vec!["t3", "t2", "t1"]);
        let among = list_untagged_among(
            &conn,
            SOURCE_LASTFM,
            &[EntityId::Str("t1".into()), EntityId::Str("t2".into())],
            10,
        )
        .unwrap();
        assert_eq!(among.len(), 2);
        assert_eq!(
            list_untagged_among(&conn, SOURCE_LASTFM, &[EntityId::Str("t1".into())], 0)
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn artist_tags_replace_and_musicbrainz_fill_only_when_empty() {
        let conn = db();
        artist(&conn, "a1", "Artist");
        let id = EntityId::Str("a1".into());
        assert!(!insert_artist_tags_if_none(&conn, &id, &[]).unwrap());
        assert!(insert_artist_tags_if_none(&conn, &id, &[tag("ambient", 0.5)]).unwrap());
        assert!(!insert_artist_tags_if_none(&conn, &id, &[tag("rock", 0.5)]).unwrap());
        replace_artist_tags(&conn, &id, &[tag("trip-hop", 0.9), tag("downtempo", 0.4)]).unwrap();
        let seed = seed_tag_vector(&conn, &id).unwrap();
        assert_eq!(seed[0].tag, "trip-hop");
        assert_eq!(seed.len(), 2);
    }

    #[test]
    fn musicbrainz_identity_and_tag_queue() {
        let conn = db();
        artist(&conn, "a1", "Artist One");
        artist(&conn, "a2", "Artist Two");
        track(&conn, "t1", "a1", "One");
        track(&conn, "t2", "a2", "Two");
        let a1 = EntityId::Str("a1".into());
        let a2 = EntityId::Str("a2".into());

        assert_eq!(
            list_artists_needing_musicbrainz_id(&conn, 30, 10)
                .unwrap()
                .len(),
            2
        );
        set_musicbrainz_identity(&conn, &a1, Some(" mbid-1 ")).unwrap();
        set_musicbrainz_identity(&conn, &a2, None).unwrap(); // stamped, unresolved
        assert_eq!(
            get_artist_musicbrainz_id(&conn, &a1).unwrap().as_deref(),
            Some("mbid-1")
        );
        assert_eq!(get_artist_musicbrainz_id(&conn, &a2).unwrap(), None);
        conn.execute(
            "UPDATE artists SET lastfm_mbid='lfm-mbid' WHERE id='a2'",
            [],
        )
        .unwrap();
        assert_eq!(
            get_artist_musicbrainz_id(&conn, &a2).unwrap().as_deref(),
            Some("lfm-mbid")
        );
        conn.execute("UPDATE artists SET lastfm_mbid=NULL WHERE id='a2'", [])
            .unwrap();
        assert_eq!(
            get_artist_musicbrainz_id(&conn, &EntityId::Str("missing".into())).unwrap(),
            None
        );
        assert!(list_artists_needing_musicbrainz_id(&conn, 30, 10)
            .unwrap()
            .is_empty());

        let needing = list_artists_needing_musicbrainz_tags(&conn, 10).unwrap();
        assert_eq!(needing, vec![(a1.clone(), "mbid-1".to_owned())]);
        crate::artwork::save_lastfm_cache(&conn, "mb-artist-tags:a1", "[]", 30);
        assert!(list_artists_needing_musicbrainz_tags(&conn, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn candidate_pools_respect_exclusion_and_tag_layers() {
        let conn = db();
        artist(&conn, "seed", "Seed");
        artist(&conn, "b", "Other B");
        artist(&conn, "c", "Other C");
        for (t, a) in [("s1", "seed"), ("b1", "b"), ("b2", "b"), ("c1", "c")] {
            track(&conn, t, a, t);
        }
        let seed = EntityId::Str("seed".into());
        replace_artist_tags(&conn, &EntityId::Str("b".into()), &[tag("trip-hop", 1.0)]).unwrap();
        replace_track_tags(
            &conn,
            &EntityId::Str("c1".into()),
            SOURCE_LASTFM,
            &[TagInput {
                tag: "melancholy".into(),
                kind: "mood".into(),
                bucket: Some("melancholic".into()),
                weight: 0.8,
            }],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO album_styles(album_id, tag, kind) VALUES('al-c', 'trip-hop', 'style')",
            [],
        )
        .unwrap();

        let ids = radio_ids_matching_tags(
            &conn,
            &["trip-hop".to_owned(), "melancholy".to_owned()],
            &seed,
            50,
        )
        .unwrap();
        let mut names: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
        names.sort();
        assert_eq!(names, vec!["b1", "b2", "c1"]);
        assert!(radio_ids_matching_tags(&conn, &[], &seed, 10)
            .unwrap()
            .is_empty());

        let seed_ids = radio_ids_for_artists(&conn, std::slice::from_ref(&seed), 5).unwrap();
        assert_eq!(seed_ids, vec![EntityId::Str("s1".into())]);
        let capped = radio_ids_for_artists(&conn, &[EntityId::Str("b".into())], 1).unwrap();
        assert_eq!(capped.len(), 1);

        let loaded = load_radio_tracks(&conn, "u1", &ids).unwrap();
        let by_id = |id: &str| {
            loaded
                .iter()
                .find(|c| c.track.id.to_string() == id)
                .expect("candidate")
        };
        assert_eq!(by_id("c1").tag_layer, TagLayer::Track);
        assert!(by_id("c1").tags.iter().any(|t| t.tag == "trip-hop")); // album fallback merged in
        assert_eq!(by_id("b1").tag_layer, TagLayer::Artist);
        assert!((by_id("b1").tags[0].weight - 0.7).abs() < 1e-9);

        assert!(count_tracks_with_any_tag(&conn, &["trip-hop".into()], 50).unwrap() >= 2);
        assert_eq!(count_tracks_with_any_tag(&conn, &[], 50).unwrap(), 0);
        assert_eq!(
            count_tracks_with_any_tag(&conn, &["nope".into()], 50).unwrap(),
            0
        );
    }

    #[test]
    fn ratings_energy_and_recency_are_per_user() {
        let conn = db();
        artist(&conn, "a1", "Artist");
        track(&conn, "t1", "a1", "One");
        user(&conn, "u1");
        user(&conn, "u2");
        conn.execute(
            "INSERT INTO track_ratings(user_id, track_id, rating) VALUES('u1', 't1', 4.5)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO play_history(id, user_id, track_id, played_at)
             VALUES('p1', 'u1', 't1', datetime('now', '-2 days'))",
            [],
        )
        .unwrap();
        let ids = [EntityId::Str("t1".into())];
        let u1 = load_radio_tracks(&conn, "u1", &ids).unwrap();
        let u2 = load_radio_tracks(&conn, "u2", &ids).unwrap();
        assert_eq!(u1[0].rating, Some(4.5));
        assert_eq!(u2[0].rating, None);
        let ages = recent_play_ages(&conn, "u1", 14).unwrap();
        assert!((ages[&ids[0]] - 2.0).abs() < 0.1);
        assert!(recent_play_ages(&conn, "u2", 14).unwrap().is_empty());
        assert_eq!(
            seed_audio_profile(&conn, &EntityId::Str("a1".into())).unwrap(),
            SeedAudio::default()
        );
    }

    #[test]
    fn album_styles_replace_per_source() {
        let conn = db();
        artist(&conn, "a1", "Artist");
        let album = EntityId::Str("al-a1".into());
        replace_album_styles(
            &conn,
            &album,
            "discogs",
            &[
                ("downtempo".into(), "style".into()),
                ("electronic".into(), "genre".into()),
            ],
        )
        .unwrap();
        replace_album_styles(&conn, &album, "discogs", &[("jazz".into(), "genre".into())]).unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM album_styles", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }
}
