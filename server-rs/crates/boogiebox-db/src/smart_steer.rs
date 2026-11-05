//! Steering pills on a smart playlist: validates a definition's optional `steer` list and turns it into one
//! weight per matching track. The rules still decide what may be in the playlist; these weights only decide
//! which of the matches get in. Plan: `wip/smart-playlist-steering-plan.md` §4–§5.

use crate::smart_playlists::{err, RuleError, RuleErrorCode, BPM};
use rusqlite::{params_from_iter, types::Value, Connection, Row};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Most pills on at once.
pub const MAX_STEER: usize = 3;
/// Most matches a steered evaluation weighs. Fixed orders read the first ones; a shuffle takes a seeded sample.
pub const STEER_POOL_MAX: usize = 5000;
/// Longest style tag, the same limit `tag_taxonomy::normalize_tag` keeps.
const MAX_TAG_CHARS: usize = 40;
/// The mood buckets stored in `track_tags.bucket` and `artist_styles.bucket`. A test in the server crate keeps
/// this in step with `tag_taxonomy::MOOD_BUCKETS`.
pub const MOOD_BUCKETS: [&str; 8] = [
    "chill",
    "melancholic",
    "uplifting",
    "energetic",
    "dark",
    "dreamy",
    "romantic",
    "aggressive",
];

/// Discogs-style album tags have no weight of their own.
const ALBUM_AFFINITY: f64 = 0.7;
/// Artist tags say less about one track than a track or album tag does.
const ARTIST_AFFINITY_SCALE: f64 = 0.6;
/// `artist_styles.weight` is empty on rows written before weights existed (same default as Artist Radio).
const DEFAULT_ARTIST_WEIGHT: f64 = 0.6;
/// A track "added recently" for the Something new pill.
const NEW_TRACK_DAYS: i64 = 30;

/// Kinds of music that are a narrower form of another without saying so in their name. Name-based children
/// ("atmospheric black metal" under "black metal") need no entry. Both sides are normalized tags.
const PARENTS: &[(&str, &str)] = &[
    ("sludge", "doom metal"),
    ("sludge metal", "doom metal"),
    ("stoner metal", "doom metal"),
    ("grindcore", "death metal"),
    ("deathcore", "death metal"),
    ("metalcore", "metal"),
    ("grunge", "rock"),
    ("shoegaze", "rock"),
    ("shoegaze", "dream pop"),
    ("post-punk", "rock"),
    ("punk", "rock"),
    ("glam", "rock"),
    ("metal", "rock"),
    ("new wave", "rock"),
    ("gothic", "post-punk"),
    ("darkwave", "post-punk"),
    ("new wave", "pop"),
    ("synthpop", "pop"),
    ("synthpop", "new wave"),
    ("europop", "pop"),
    ("britpop", "pop"),
    ("electropop", "pop"),
    ("eurodance", "dance"),
    ("disco", "dance"),
    ("house", "dance"),
    ("house", "electronic"),
    ("techno", "electronic"),
    ("trance", "electronic"),
    ("ambient", "electronic"),
    ("downtempo", "electronic"),
    ("trip-hop", "electronic"),
    ("idm", "electronic"),
    ("drum and bass", "electronic"),
    ("dubstep", "electronic"),
    ("electro", "electronic"),
    ("synthwave", "electronic"),
    ("minimal", "techno"),
    ("gangsta rap", "hip-hop"),
    ("trap", "hip-hop"),
    ("boom bap", "hip-hop"),
    ("bebop", "jazz"),
    ("swing", "jazz"),
    ("fusion", "jazz"),
    ("motown", "soul"),
    ("funk", "soul"),
    ("americana", "folk"),
    ("bluegrass", "country"),
    ("baroque", "classical"),
    ("opera", "classical"),
    ("dub", "reggae"),
    ("ska", "reggae"),
    ("dancehall", "reggae"),
];

/// One pill as sent by clients and stored in the definition. Fields stay strings so the validator can name
/// what is wrong.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Steer {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lean {
    More,
    Less,
    Deeper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Signal {
    Favourites,
    Popular,
    New,
    Faster,
    Slower,
    Energetic,
    Calmer,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CSteer {
    Style { tag: String, lean: Lean },
    Mood { bucket: String, lean: Lean },
    Signal(Signal),
}

impl CSteer {
    /// Reads the owner's ratings, plays or favourites, so a change to those makes the playlist stale.
    pub(crate) fn is_personal(&self) -> bool {
        matches!(
            self,
            CSteer::Signal(Signal::Favourites | Signal::Popular | Signal::New)
        )
    }

    /// Changes with time alone.
    pub(crate) fn is_time_based(&self) -> bool {
        matches!(self, CSteer::Signal(Signal::New))
    }

    /// Two pills sharing a target would fight or repeat each other.
    fn target(&self) -> String {
        match self {
            CSteer::Style { tag, .. } => format!("style:{tag}"),
            CSteer::Mood { bucket, .. } => format!("mood:{bucket}"),
            CSteer::Signal(Signal::Faster | Signal::Slower) => "signal:bpm".to_owned(),
            CSteer::Signal(Signal::Energetic | Signal::Calmer) => "signal:energy".to_owned(),
            CSteer::Signal(signal) => format!("signal:{signal:?}"),
        }
    }
}

/// Lowercases and tidies a tag the way `tag_taxonomy::normalize_tag` does, so it matches stored tags.
fn normalized_tag(raw: &str) -> Option<String> {
    let tag = raw
        .replace(['_', '\u{a0}'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    (1..=MAX_TAG_CHARS)
        .contains(&tag.chars().count())
        .then_some(tag)
}

fn check_one(steer: &mut Steer, path: &str, errors: &mut Vec<RuleError>) -> Option<CSteer> {
    let mut bad = |field: &str| {
        let at = if field.is_empty() {
            path.to_owned()
        } else {
            format!("{path}.{field}")
        };
        errors.push(err(at, RuleErrorCode::BadSteer));
        None
    };
    let lean = match steer.direction.as_deref() {
        Some("more") => Some(Lean::More),
        Some("less") => Some(Lean::Less),
        Some("deeper") => Some(Lean::Deeper),
        _ => None,
    };
    match steer.kind.as_str() {
        "style" => {
            if steer.bucket.is_some() || steer.signal.is_some() {
                return bad("");
            }
            let Some(tag) = steer.tag.as_deref().and_then(normalized_tag) else {
                return bad("tag");
            };
            let Some(lean) = lean else {
                return bad("direction");
            };
            steer.tag = Some(tag.clone());
            Some(CSteer::Style { tag, lean })
        }
        "mood" => {
            if steer.tag.is_some() || steer.signal.is_some() {
                return bad("");
            }
            let Some(bucket) = steer
                .bucket
                .as_deref()
                .filter(|b| MOOD_BUCKETS.contains(b))
                .map(str::to_owned)
            else {
                return bad("bucket");
            };
            let Some(lean) = lean.filter(|l| *l != Lean::Deeper) else {
                return bad("direction");
            };
            Some(CSteer::Mood { bucket, lean })
        }
        "signal" => {
            if steer.tag.is_some() || steer.bucket.is_some() || steer.direction.is_some() {
                return bad("");
            }
            let signal = match steer.signal.as_deref() {
                Some("favourites") => Signal::Favourites,
                Some("popular") => Signal::Popular,
                Some("new") => Signal::New,
                Some("faster") => Signal::Faster,
                Some("slower") => Signal::Slower,
                Some("energetic") => Signal::Energetic,
                Some("calmer") => Signal::Calmer,
                _ => return bad("signal"),
            };
            Some(CSteer::Signal(signal))
        }
        _ => bad("kind"),
    }
}

/// Validates a definition's pills, normalizing tags in place, and collects every problem.
pub(crate) fn check_steer(steers: &mut [Steer], errors: &mut Vec<RuleError>) -> Vec<CSteer> {
    if steers.len() > MAX_STEER {
        errors.push(err("steer", RuleErrorCode::BadSteer));
    }
    let mut out: Vec<CSteer> = Vec::with_capacity(steers.len());
    let mut targets = HashSet::new();
    for (i, steer) in steers.iter_mut().enumerate() {
        let path = format!("steer[{i}]");
        if let Some(compiled) = check_one(steer, &path, errors) {
            if targets.insert(compiled.target()) {
                out.push(compiled);
            } else {
                errors.push(err(path, RuleErrorCode::BadSteer));
            }
        }
    }
    out
}

/// Whether `child` is a narrower kind of `parent`: named after it ("atmospheric black metal", "synth-pop") or
/// listed in [`PARENTS`].
pub fn is_child_tag(child: &str, parent: &str) -> bool {
    if child == parent {
        return false;
    }
    let named = child.strip_suffix(parent).is_some_and(|rest| {
        rest.ends_with(' ') || (rest.ends_with('-') && rest.chars().count() > 1)
    });
    named || PARENTS.iter().any(|(c, p)| *c == child && *p == parent)
}

/// Tags in the library that are narrower kinds of `parent`. Only real tags, never invented ones.
pub fn children_in_library(conn: &Connection, parent: &str) -> rusqlite::Result<Vec<String>> {
    let mut found = HashSet::new();
    for sql in [
        "SELECT DISTINCT tag FROM track_tags",
        "SELECT DISTINCT tag FROM album_styles",
        "SELECT DISTINCT style FROM artist_styles",
    ] {
        let mut stmt = conn.prepare(sql)?;
        let tags = stmt.query_map([], |row| row.get::<_, String>(0))?;
        for tag in tags {
            let tag = tag?.to_lowercase();
            if is_child_tag(&tag, parent) {
                found.insert(tag);
            }
        }
    }
    let mut out: Vec<String> = found.into_iter().collect();
    out.sort();
    Ok(out)
}

fn id_text(value: Value) -> String {
    match value {
        Value::Integer(n) => n.to_string(),
        Value::Text(s) => s,
        _ => String::new(),
    }
}

/// Runs a query whose first column is a track id and hands each row to `visit`.
fn scan(
    conn: &Connection,
    sql: &str,
    params: Vec<Value>,
    mut visit: impl FnMut(&str, &Row<'_>) -> rusqlite::Result<()>,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    while let Some(row) = rows.next()? {
        let id = id_text(row.get(0)?);
        visit(&id, row)?;
    }
    Ok(())
}

/// The ids in `sql`'s first column that are in the pool, as flags per pool position.
fn pool_flags(
    conn: &Connection,
    index: &HashMap<&str, usize>,
    sql: &str,
    params: Vec<Value>,
) -> rusqlite::Result<Vec<bool>> {
    let mut flags = vec![false; index.len()];
    scan(conn, sql, params, |id, _| {
        if let Some(&i) = index.get(id) {
            flags[i] = true;
        }
        Ok(())
    })?;
    Ok(flags)
}

fn more_factor(affinity: f64) -> f64 {
    1.0 + 3.0 * affinity
}

fn less_factor(affinity: f64) -> f64 {
    (1.0 - 0.9 * affinity).max(0.1)
}

/// Each value's position among the known ones, 0 to 1. Equal values share their middle position, so a pool
/// where every value is the same sits at 0.5 and nothing changes.
fn percentiles(values: &[Option<f64>]) -> Vec<Option<f64>> {
    let mut known: Vec<f64> = values.iter().flatten().copied().collect();
    known.sort_by(f64::total_cmp);
    let top = known.len().saturating_sub(1) as f64;
    values
        .iter()
        .map(|value| {
            value.map(|v| {
                if top > 0.0 {
                    let below = known.partition_point(|k| *k < v);
                    let through = known.partition_point(|k| *k <= v);
                    (below + through - 1) as f64 / 2.0 / top
                } else {
                    0.5
                }
            })
        })
        .collect()
}

fn style_factors(
    conn: &Connection,
    index: &HashMap<&str, usize>,
    tag: &str,
    lean: Lean,
) -> rusqlite::Result<Vec<f64>> {
    let children = children_in_library(conn, tag)?;
    let mut tags = vec![tag.to_owned()];
    tags.extend(children);
    let marks = vec!["?"; tags.len()].join(",");
    let params: Vec<Value> = tags.iter().cloned().map(Value::Text).collect();

    // Strongest evidence for the tag itself and for its children, per pool track.
    let mut own = vec![0.0_f64; index.len()];
    let mut narrower = vec![0.0_f64; index.len()];
    let mut note = |id: &str, found: &str, affinity: f64| {
        if let Some(&i) = index.get(id) {
            let slot = if found.eq_ignore_ascii_case(tag) {
                &mut own[i]
            } else {
                &mut narrower[i]
            };
            *slot = slot.max(affinity.clamp(0.0, 1.0));
        }
    };
    scan(
        conn,
        &format!("SELECT track_id, tag, weight FROM track_tags WHERE tag IN ({marks})"),
        params.clone(),
        |id, row| {
            note(id, &row.get::<_, String>(1)?, row.get(2)?);
            Ok(())
        },
    )?;
    scan(
        conn,
        &format!(
            "SELECT t.id, s.tag FROM album_styles s JOIN tracks t ON t.album_id = s.album_id
             WHERE s.tag IN ({marks})"
        ),
        params.clone(),
        |id, row| {
            note(id, &row.get::<_, String>(1)?, ALBUM_AFFINITY);
            Ok(())
        },
    )?;
    scan(
        conn,
        &format!(
            "SELECT t.id, s.style, s.weight FROM artist_styles s JOIN tracks t ON t.artist_id = s.artist_id
             WHERE s.style IN ({marks})"
        ),
        params,
        |id, row| {
            let weight = row
                .get::<_, Option<f64>>(2)?
                .unwrap_or(DEFAULT_ARTIST_WEIGHT);
            note(
                id,
                &row.get::<_, String>(1)?,
                weight.min(1.0) * ARTIST_AFFINITY_SCALE,
            );
            Ok(())
        },
    )?;
    Ok(own
        .into_iter()
        .zip(narrower)
        .map(|(own, narrower)| match lean {
            Lean::More => more_factor(own.max(narrower)),
            Lean::Less => less_factor(own.max(narrower)),
            Lean::Deeper => 1.0 + 4.0 * narrower + own,
        })
        .collect())
}

fn mood_factors(
    conn: &Connection,
    index: &HashMap<&str, usize>,
    bucket: &str,
    lean: Lean,
) -> rusqlite::Result<Vec<f64>> {
    let mut affinity = vec![0.0_f64; index.len()];
    let param = || vec![Value::Text(bucket.to_owned())];
    scan(
        conn,
        "SELECT track_id, weight FROM track_tags WHERE bucket = ?1",
        param(),
        |id, row| {
            if let Some(&i) = index.get(id) {
                affinity[i] = affinity[i].max(row.get::<_, f64>(1)?.clamp(0.0, 1.0));
            }
            Ok(())
        },
    )?;
    scan(
        conn,
        "SELECT t.id, s.weight FROM artist_styles s JOIN tracks t ON t.artist_id = s.artist_id
         WHERE s.bucket = ?1",
        param(),
        |id, row| {
            if let Some(&i) = index.get(id) {
                let weight = row
                    .get::<_, Option<f64>>(1)?
                    .unwrap_or(DEFAULT_ARTIST_WEIGHT);
                affinity[i] = affinity[i].max(weight.clamp(0.0, 1.0) * ARTIST_AFFINITY_SCALE);
            }
            Ok(())
        },
    )?;
    Ok(affinity
        .into_iter()
        .map(|a| match lean {
            Lean::Less => less_factor(a),
            _ => more_factor(a),
        })
        .collect())
}

/// Values per pool track from a query of `(track id, number)`; tracks it doesn't list get `missing`.
fn pool_values(
    conn: &Connection,
    index: &HashMap<&str, usize>,
    sql: &str,
    missing: Option<f64>,
) -> rusqlite::Result<Vec<Option<f64>>> {
    let mut values = vec![missing; index.len()];
    scan(conn, sql, Vec::new(), |id, row| {
        if let Some(&i) = index.get(id) {
            if let Some(value) = row.get::<_, Option<f64>>(1)? {
                values[i] = Some(value);
            }
        }
        Ok(())
    })?;
    Ok(values)
}

/// A rising or falling lean from each track's position in the pool: from 1/2 to 2, so the middle of the pool
/// and tracks with no value at all sit at 1. Starting at 1 would rank a track with no value level with the
/// worst fit ("Calmer" would prefer a loud track to an unmeasured one).
fn rank_factors(values: &[Option<f64>], rising: bool) -> Vec<f64> {
    percentiles(values)
        .into_iter()
        .map(|position| match position {
            Some(p) => 4.0_f64.powf(if rising { p } else { 1.0 - p } - 0.5),
            None => 1.0,
        })
        .collect()
}

fn signal_factors(
    conn: &Connection,
    index: &HashMap<&str, usize>,
    owner: &str,
    now: &str,
    signal: Signal,
) -> rusqlite::Result<Vec<f64>> {
    let owner_param = || vec![Value::Text(owner.to_owned())];
    let bpm_sql = format!("SELECT t.id, {BPM} FROM tracks t");
    const ENERGY_SQL: &str = "SELECT track_id, energy_score_refined FROM track_deep_analysis
                              WHERE confidence > 0.25 AND energy_score_refined IS NOT NULL";
    Ok(match signal {
        Signal::Favourites => {
            let starred = pool_flags(
                conn,
                index,
                "SELECT item_id FROM subsonic_stars WHERE user_id = ?1 AND item_type = 'track'",
                owner_param(),
            )?;
            let rated = pool_flags(
                conn,
                index,
                "SELECT track_id FROM track_ratings WHERE user_id = ?1 AND rating >= 4",
                owner_param(),
            )?;
            starred
                .into_iter()
                .zip(rated)
                .map(|(s, r)| if s || r { 4.0 } else { 1.0 })
                .collect()
        }
        Signal::Popular => rank_factors(
            &pool_values(
                conn,
                index,
                "SELECT track_id, SUM(play_count) FROM user_track_stats GROUP BY track_id",
                Some(0.0),
            )?,
            true,
        ),
        Signal::New => {
            let played = pool_flags(
                conn,
                index,
                "SELECT track_id FROM user_track_stats WHERE user_id = ?1 AND play_count > 0",
                owner_param(),
            )?;
            let recent = pool_flags(
                conn,
                index,
                &format!("SELECT id FROM tracks WHERE added_at >= datetime(?1, '-{NEW_TRACK_DAYS} days')"),
                vec![Value::Text(now.to_owned())],
            )?;
            played
                .into_iter()
                .zip(recent)
                .map(|(p, r)| if !p || r { 3.0 } else { 1.0 })
                .collect()
        }
        Signal::Faster => rank_factors(&pool_values(conn, index, &bpm_sql, None)?, true),
        Signal::Slower => rank_factors(&pool_values(conn, index, &bpm_sql, None)?, false),
        Signal::Energetic => rank_factors(&pool_values(conn, index, ENERGY_SQL, None)?, true),
        Signal::Calmer => rank_factors(&pool_values(conn, index, ENERGY_SQL, None)?, false),
    })
}

/// One weight per id in `ids`: the product of every pill's factor, 1 when a pill says nothing about a track.
pub(crate) fn steer_weights(
    conn: &Connection,
    owner: &str,
    now: &str,
    steers: &[CSteer],
    ids: &[String],
) -> rusqlite::Result<Vec<f64>> {
    let index: HashMap<&str, usize> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    let mut weights = vec![1.0; ids.len()];
    for steer in steers {
        let factors = match steer {
            CSteer::Style { tag, lean } => style_factors(conn, &index, tag, *lean)?,
            CSteer::Mood { bucket, lean } => mood_factors(conn, &index, bucket, *lean)?,
            CSteer::Signal(signal) => signal_factors(conn, &index, owner, now, *signal)?,
        };
        for (weight, factor) in weights.iter_mut().zip(factors) {
            *weight *= factor;
        }
    }
    Ok(weights)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smart_playlists::{evaluate, parse_definition, RuleErrorCode, ValidDefinition};
    use serde_json::{json, Value as Json};

    const NOW: &str = "2026-11-01 12:00:00";

    /// 200 Metal tracks by five artists, 20 Pop tracks, and a second user.
    /// `sludge` 40 tracks, `doom metal` 60, `black metal` 40 + `atmospheric black metal` 20, `thrash metal` 40.
    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO users(id, username) VALUES ('alice', 'alice'), ('bob', 'bob');
             INSERT INTO libraries(id, path, name) VALUES ('lib1', '/m1', 'Music');
             INSERT INTO artists(id, name) VALUES ('sl', 'Sludge Band'), ('dm', 'Doom Band'), ('bm', 'Black Band'),
               ('ab', 'Atmos Band'), ('th', 'Thrash Band'), ('po', 'Pop Band');
             INSERT INTO artist_styles(artist_id, style, kind, bucket, weight) VALUES
               ('sl', 'sludge', 'genre', NULL, 1.0), ('dm', 'doom metal', 'genre', NULL, 1.0),
               ('bm', 'black metal', 'genre', NULL, 1.0), ('ab', 'atmospheric black metal', 'genre', NULL, 1.0),
               ('th', 'thrash metal', 'genre', NULL, 1.0), ('dm', 'dark', 'mood', 'dark', 1.0);",
        )
        .unwrap();
        let mut n = 0;
        for (artist, genre, count) in [
            ("sl", "Metal", 40),
            ("dm", "Metal", 60),
            ("bm", "Metal", 40),
            ("ab", "Metal", 20),
            ("th", "Metal", 40),
            ("po", "Pop", 20),
        ] {
            for _ in 0..count {
                conn.execute(
                    "INSERT INTO tracks(id, library_id, artist_id, title, file_path, genre, duration, added_at, bpm_detected)
                     VALUES (?1, 'lib1', ?2, ?3, ?4, ?5, 200, '2025-01-01 00:00:00', ?6)",
                    rusqlite::params![
                        format!("t{n:03}"),
                        artist,
                        format!("Song {n:03}"),
                        format!("/m1/{n}"),
                        genre,
                        60 + n,
                    ],
                )
                .unwrap();
                n += 1;
            }
        }
        conn
    }

    fn def(steer: Json, sort: Json, max_tracks: u32) -> ValidDefinition {
        parse_definition(
            &json!({
                "schemaVersion": 1,
                "root": { "mode": "all", "children": [{ "field": "genre", "operator": "is", "value": "Metal" }] },
                "sort": sort,
                "limit": { "maxTracks": max_tracks },
                "steer": steer,
            })
            .to_string(),
        )
        .expect("valid definition")
    }

    fn shuffled(steer: Json, max_tracks: u32) -> ValidDefinition {
        def(
            steer,
            json!([{ "field": "shuffle", "seed": 7 }]),
            max_tracks,
        )
    }

    fn pick(conn: &Connection, owner: &str, definition: &ValidDefinition) -> Vec<String> {
        evaluate(conn, owner, definition, NOW).unwrap().track_ids
    }

    /// How many of `ids` are by `artist`.
    fn by_artist(conn: &Connection, ids: &[String], artist: &str) -> usize {
        ids.iter()
            .filter(|id| {
                conn.query_row("SELECT artist_id FROM tracks WHERE id = ?1", [id], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap()
                    == artist
            })
            .count()
    }

    fn style(tag: &str, direction: &str) -> Json {
        json!({ "kind": "style", "tag": tag, "direction": direction })
    }

    fn signal(name: &str) -> Json {
        json!({ "kind": "signal", "signal": name })
    }

    fn steer_errors(steer: Json) -> Vec<(String, RuleErrorCode)> {
        parse_definition(
            &json!({
                "schemaVersion": 1,
                "root": { "mode": "all", "children": [{ "field": "genre", "operator": "is", "value": "Metal" }] },
                "steer": steer,
            })
            .to_string(),
        )
        .expect_err("invalid steer")
        .into_iter()
        .map(|e| (e.path, e.code))
        .collect()
    }

    #[test]
    fn validation_names_what_is_wrong() {
        let bad = |path: &str| vec![(path.to_owned(), RuleErrorCode::BadSteer)];
        assert_eq!(
            steer_errors(json!([{ "kind": "weather" }])),
            bad("steer[0].kind")
        );
        assert_eq!(
            steer_errors(json!([{ "kind": "style", "direction": "more" }])),
            bad("steer[0].tag")
        );
        assert_eq!(
            steer_errors(json!([{ "kind": "style", "tag": "sludge", "direction": "sideways" }])),
            bad("steer[0].direction")
        );
        assert_eq!(
            steer_errors(json!([{ "kind": "style", "tag": "x".repeat(41), "direction": "more" }])),
            bad("steer[0].tag")
        );
        assert_eq!(
            steer_errors(
                json!([{ "kind": "style", "tag": "sludge", "direction": "more", "signal": "new" }])
            ),
            bad("steer[0]")
        );
        assert_eq!(
            steer_errors(json!([{ "kind": "mood", "bucket": "sleepy", "direction": "more" }])),
            bad("steer[0].bucket")
        );
        assert_eq!(
            steer_errors(json!([{ "kind": "mood", "bucket": "dark", "direction": "deeper" }])),
            bad("steer[0].direction")
        );
        assert_eq!(
            steer_errors(json!([{ "kind": "signal", "signal": "louder" }])),
            bad("steer[0].signal")
        );
        assert_eq!(
            steer_errors(json!([{ "kind": "signal", "signal": "new", "direction": "more" }])),
            bad("steer[0]")
        );
    }

    #[test]
    fn at_most_three_pills_and_no_repeats_or_opposites() {
        let four = json!([
            style("a1", "more"),
            style("a2", "more"),
            style("a3", "more"),
            style("a4", "more")
        ]);
        assert_eq!(
            steer_errors(four),
            vec![("steer".to_owned(), RuleErrorCode::BadSteer)]
        );
        for repeat in [
            json!([style("sludge", "more"), style("Sludge", "less")]),
            json!([signal("faster"), signal("slower")]),
            json!([signal("energetic"), signal("calmer")]),
            json!([signal("new"), signal("new")]),
        ] {
            assert_eq!(
                steer_errors(repeat),
                vec![("steer[1]".to_owned(), RuleErrorCode::BadSteer)]
            );
        }
    }

    #[test]
    fn stored_form_is_normalized_and_unchanged_without_pills() {
        let with = def(
            json!([style("  Black_Metal ", "deeper")]),
            json!([{ "field": "title" }]),
            10,
        );
        assert_eq!(
            with.definition().steer[0].tag.as_deref(),
            Some("black metal")
        );

        let without = parse_definition(
            &json!({
                "schemaVersion": 1,
                "root": { "mode": "all", "children": [{ "field": "genre", "operator": "is", "value": "Metal" }] },
            })
            .to_string(),
        )
        .unwrap();
        assert!(
            !without.canonical_json().contains("steer"),
            "definitions without pills must store exactly what they stored before"
        );
    }

    #[test]
    fn children_come_from_the_name_or_the_list() {
        assert!(is_child_tag("atmospheric black metal", "black metal"));
        assert!(is_child_tag("classic rock", "rock"));
        assert!(is_child_tag("synth-pop", "pop"));
        assert!(is_child_tag("sludge", "doom metal"));
        assert!(!is_child_tag("rock", "rock"));
        assert!(!is_child_tag("krock", "rock"));
        assert!(!is_child_tag("black metal", "atmospheric black metal"));
        assert!(!is_child_tag("-pop", "pop"));

        let conn = fixture();
        assert_eq!(
            children_in_library(&conn, "black metal").unwrap(),
            vec!["atmospheric black metal"],
            "only tags the library has"
        );
        assert_eq!(
            children_in_library(&conn, "doom metal").unwrap(),
            vec!["sludge"]
        );
    }

    #[test]
    fn parent_list_uses_normalized_tags_and_has_no_self_or_repeats() {
        let mut seen = HashSet::new();
        for (child, parent) in PARENTS {
            assert_ne!(child, parent);
            assert_eq!(normalized_tag(child).as_deref(), Some(*child));
            assert_eq!(normalized_tag(parent).as_deref(), Some(*parent));
            assert!(seen.insert((child, parent)), "{child} under {parent} twice");
        }
    }

    #[test]
    fn a_pill_that_says_nothing_changes_nothing() {
        let conn = fixture();
        let shuffle_sort = json!([{ "field": "shuffle", "seed": 7 }]);
        let title_sort = json!([{ "field": "title" }]);
        for sort in [shuffle_sort, title_sort] {
            let base = pick(&conn, "alice", &def(json!([]), sort.clone(), 50));
            for silent in [
                json!([style("no such tag", "more")]),
                json!([style("no such tag", "deeper")]),
                json!([signal("energetic")]),
            ] {
                assert_eq!(
                    pick(&conn, "alice", &def(silent.clone(), sort.clone(), 50)),
                    base,
                    "{silent}"
                );
            }
        }
    }

    #[test]
    fn more_raises_a_style_and_less_lowers_it_inside_the_rules() {
        let conn = fixture();
        let base = pick(&conn, "alice", &shuffled(json!([]), 50));
        let more = pick(
            &conn,
            "alice",
            &shuffled(json!([style("sludge", "more")]), 50),
        );
        let less = pick(
            &conn,
            "alice",
            &shuffled(json!([style("sludge", "less")]), 50),
        );
        let share = |ids: &[String]| by_artist(&conn, ids, "sl");
        assert!(
            share(&more) > share(&base),
            "{} !> {}",
            share(&more),
            share(&base)
        );
        assert!(
            share(&less) < share(&base),
            "{} !< {}",
            share(&less),
            share(&base)
        );
        assert!(share(&more) < 40, "a lean, not a filter: {}", share(&more));
        for ids in [&base, &more, &less] {
            assert_eq!(ids.len(), 50);
            assert_eq!(by_artist(&conn, ids, "po"), 0, "stays inside the rules");
        }
    }

    #[test]
    fn deeper_prefers_narrower_kinds_over_the_style_itself() {
        let conn = fixture();
        let more = pick(
            &conn,
            "alice",
            &shuffled(json!([style("black metal", "more")]), 40),
        );
        let deeper = pick(
            &conn,
            "alice",
            &shuffled(json!([style("black metal", "deeper")]), 40),
        );
        assert!(by_artist(&conn, &deeper, "ab") > by_artist(&conn, &more, "ab"));
        assert!(by_artist(&conn, &deeper, "bm") < by_artist(&conn, &more, "bm"));
    }

    #[test]
    fn steered_results_repeat_and_respect_both_limits() {
        let conn = fixture();
        let steer = json!([style("sludge", "more"), signal("faster")]);
        let first = pick(&conn, "alice", &shuffled(steer.clone(), 30));
        assert_eq!(first, pick(&conn, "alice", &shuffled(steer.clone(), 30)));
        assert_eq!(first.len(), 30);

        let bounded = parse_definition(
            &json!({
                "schemaVersion": 1,
                "root": { "mode": "all", "children": [{ "field": "genre", "operator": "is", "value": "Metal" }] },
                "sort": [{ "field": "shuffle", "seed": 7 }],
                "limit": { "maxTracks": 100, "maxDurationSeconds": 1000 },
                "steer": steer,
            })
            .to_string(),
        )
        .unwrap();
        let selection = evaluate(&conn, "alice", &bounded, NOW).unwrap();
        assert_eq!(selection.track_ids.len(), 5);
        assert!(selection.total_duration <= 1000.0);
    }

    #[test]
    fn a_fixed_order_leans_without_becoming_a_filter_and_keeps_its_order() {
        let conn = fixture();
        let sort = json!([{ "field": "title", "direction": "desc" }]);
        let base = pick(&conn, "alice", &def(json!([]), sort.clone(), 50));
        let more = pick(
            &conn,
            "alice",
            &def(json!([style("doom metal", "more")]), sort.clone(), 50),
        );
        let thrash_before = by_artist(&conn, &base, "th");
        assert!(thrash_before > 0, "the fixture's top titles are thrash");
        assert!(by_artist(&conn, &more, "dm") > by_artist(&conn, &base, "dm"));
        assert!(
            by_artist(&conn, &more, "th") > 0,
            "other tracks keep their place: a lean, not a filter"
        );
        assert_eq!(more.len(), 50);
        let mut in_order = more.clone();
        in_order.sort();
        in_order.reverse();
        assert_eq!(more, in_order, "still in the user's order");
    }

    #[test]
    fn mood_pills_read_the_stored_bucket() {
        let conn = fixture();
        let more = pick(
            &conn,
            "alice",
            &shuffled(
                json!([{ "kind": "mood", "bucket": "dark", "direction": "more" }]),
                50,
            ),
        );
        let base = pick(&conn, "alice", &shuffled(json!([]), 50));
        assert!(by_artist(&conn, &more, "dm") > by_artist(&conn, &base, "dm"));
    }

    #[test]
    fn personal_pills_differ_per_user() {
        let conn = fixture();
        for i in 0..40 {
            conn.execute(
                "INSERT INTO track_ratings(user_id, track_id, rating) VALUES ('bob', ?1, 5)",
                [format!("t{i:03}")],
            )
            .unwrap();
        }
        let definition = shuffled(json!([signal("favourites")]), 50);
        let alice = pick(&conn, "alice", &definition);
        let bob = pick(&conn, "bob", &definition);
        assert_eq!(
            alice,
            pick(&conn, "alice", &shuffled(json!([]), 50)),
            "alice has none"
        );
        assert!(by_artist(&conn, &bob, "sl") > by_artist(&conn, &alice, "sl"));
    }

    #[test]
    fn popular_counts_plays_from_every_user() {
        let conn = fixture();
        for (i, user) in (0..40).map(|i| (i, if i % 2 == 0 { "alice" } else { "bob" })) {
            conn.execute(
                "INSERT INTO play_history(id, user_id, track_id, played_at)
                 VALUES (?1, ?2, ?3, '2026-10-01 10:00:00')",
                rusqlite::params![format!("h{i}"), user, format!("t{i:03}")],
            )
            .unwrap();
        }
        let popular = pick(&conn, "alice", &shuffled(json!([signal("popular")]), 50));
        let base = pick(&conn, "alice", &shuffled(json!([]), 50));
        assert!(by_artist(&conn, &popular, "sl") > by_artist(&conn, &base, "sl"));
    }

    #[test]
    fn something_new_prefers_unplayed_and_recently_added() {
        let conn = fixture();
        // Alice has played everything except the thrash tracks.
        conn.execute_batch(
            "INSERT INTO play_history(id, user_id, track_id, played_at)
             SELECT 'h' || t.id, 'alice', t.id, '2026-10-01 10:00:00' FROM tracks t WHERE t.artist_id <> 'th';",
        )
        .unwrap();
        let base = pick(&conn, "alice", &shuffled(json!([]), 50));
        let new = pick(&conn, "alice", &shuffled(json!([signal("new")]), 50));
        assert!(by_artist(&conn, &new, "th") > by_artist(&conn, &base, "th"));

        // A played track added recently still counts.
        conn.execute(
            "UPDATE tracks SET added_at = '2026-10-20 00:00:00' WHERE artist_id = 'bm'",
            [],
        )
        .unwrap();
        let recent = pick(&conn, "alice", &shuffled(json!([signal("new")]), 50));
        assert!(by_artist(&conn, &recent, "bm") > by_artist(&conn, &new, "bm"));
    }

    #[test]
    fn bpm_pills_use_the_detected_value() {
        let conn = fixture();
        // Fixture BPM rises with the track number, so the last Metal artist (thrash) is fastest.
        let fast = pick(&conn, "alice", &shuffled(json!([signal("faster")]), 50));
        let slow = pick(&conn, "alice", &shuffled(json!([signal("slower")]), 50));
        assert!(by_artist(&conn, &fast, "th") > by_artist(&conn, &slow, "th"));
        assert!(by_artist(&conn, &slow, "sl") > by_artist(&conn, &fast, "sl"));

        let values = percentiles(&[Some(10.0), None, Some(30.0), Some(20.0), Some(10.0)]);
        assert_eq!(
            values,
            [
                Some(1.0 / 6.0),
                None,
                Some(1.0),
                Some(2.0 / 3.0),
                Some(1.0 / 6.0)
            ]
        );
        assert_eq!(
            rank_factors(&[Some(1.0), None, Some(2.0)], false),
            [2.0, 1.0, 0.5]
        );
        assert_eq!(
            rank_factors(&[Some(5.0), Some(5.0)], true),
            [1.0, 1.0],
            "all equal: nothing to lean on"
        );
    }

    #[test]
    fn energy_pills_read_confident_deep_analysis_and_leave_the_rest_neutral() {
        let conn = fixture();
        // Sludge is loud, doom is quiet, and thrash has a reading too unsure to use.
        conn.execute_batch(
            "INSERT INTO track_deep_analysis(track_id, energy_score_refined, confidence)
             SELECT t.id,
                    CASE t.artist_id WHEN 'sl' THEN 0.9 WHEN 'dm' THEN 0.1 ELSE 0.99 END,
                    CASE t.artist_id WHEN 'th' THEN 0.1 ELSE 0.9 END
             FROM tracks t WHERE t.artist_id IN ('sl', 'dm', 'th');",
        )
        .unwrap();
        let base = pick(&conn, "alice", &shuffled(json!([]), 50));
        let energetic = pick(&conn, "alice", &shuffled(json!([signal("energetic")]), 50));
        let calmer = pick(&conn, "alice", &shuffled(json!([signal("calmer")]), 50));
        assert!(by_artist(&conn, &energetic, "sl") > by_artist(&conn, &base, "sl"));
        assert!(by_artist(&conn, &energetic, "dm") < by_artist(&conn, &base, "dm"));
        assert!(by_artist(&conn, &calmer, "dm") > by_artist(&conn, &base, "dm"));
        assert!(by_artist(&conn, &calmer, "sl") < by_artist(&conn, &base, "sl"));
    }

    #[test]
    fn freshness_knows_which_pills_are_personal_or_time_based() {
        for (name, personal, timed) in [
            ("favourites", true, false),
            ("popular", true, false),
            ("new", true, true),
            ("faster", false, false),
            ("energetic", false, false),
        ] {
            let definition = shuffled(json!([signal(name)]), 50);
            assert_eq!(definition.uses_personal(), personal, "{name}");
            assert_eq!(definition.has_relative_dates(), timed, "{name}");
        }
        let style_pill = shuffled(json!([style("sludge", "more")]), 50);
        assert!(!style_pill.uses_personal());
        assert!(!style_pill.has_relative_dates());
    }
}
