//! Chooses which steering pills to offer for a smart playlist. Pills only appear when they can do something:
//! the rules must match more tracks than fit, and enough of those tracks must back the pill. Plan:
//! `wip/smart-playlist-steering-plan.md` §2a, §5.3 and worked examples W1–W8.

use crate::smart_playlists::{
    evaluate_with_budget, leaned_positions, matching_pool, EvalError, ValidDefinition,
    DEFAULT_QUERY_BUDGET,
};
use crate::smart_steer::{
    self, bpm_sql, canonical_tag, deeper_factor, is_child_tag, less_factor, more_factor,
    signal_factors, steer_weights, CSteer, Signal, Steer, ALBUM_AFFINITY, ARTIST_AFFINITY_SCALE,
    DEFAULT_ARTIST_WEIGHT, ENERGY_SQL, NEW_TRACK_DAYS,
};
use rusqlite::{types::Value, Connection};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet};

/// Most pills shown at once, counting the ones already on.
const MAX_PILLS: usize = 8;
/// Most style and mood pills; the other places go to favourites, popular, new and sound.
const MAX_TAG_PILLS: usize = 5;
/// A style in at least this share of the playlist is dominant: "Go deeper" and "Less" instead of "More".
const DOMINANT_SHARE: f64 = 0.35;
/// Below this share of the playlist a style is too rare to bother with.
const MIN_SHARE: f64 = 0.05;
/// A style in this share of the pool or more can't be reduced much: "Less" barely moved it on real data.
const MAX_LESS_POOL_SHARE: f64 = 0.80;
/// Tracks so weakly tied to a style barely move when it is boosted.
const MIN_AVERAGE_AFFINITY: f64 = 0.4;
/// Fewest tracks behind a "More" pill, also at least a tenth of the playlist.
const MIN_SUPPORT: usize = 10;
/// "Go deeper" needs this many narrower kinds, each with this many tracks.
const MIN_CHILDREN: usize = 2;
const MIN_CHILD_SUPPORT: usize = 8;
/// Two styles sharing this much of their tracks (intersection over union) are the same pill twice.
const DUPLICATE_OVERLAP: f64 = 0.8;
/// Under this share of tagged tracks, style and mood pills are not offered at all.
const MIN_TAGGED_SHARE: f64 = 0.2;
/// Favourites, popular and new need this many tracks to do anything noticeable.
const MIN_SIGNAL_TRACKS: usize = 10;
/// BPM and energy pills need a value on this share of the pool.
const MIN_SOUND_SHARE: f64 = 0.5;
/// A pill must change at least this share of the playlist (and at least [`MIN_CHANGED_TRACKS`]), checked with a
/// dry run. On a real library a "More techno" pill changed nothing: every techno track that could get in
/// already had.
const MIN_CHANGED_SHARE: usize = 20;
const MIN_CHANGED_TRACKS: usize = 2;
/// Most untagged tracks handed back for an on-demand tag lookup.
const MAX_UNTAGGED: usize = 200;

/// Places and collection names that Last.fm and Discogs tag artists with. Not kinds of music.
const NON_STYLE_TAGS: &[&str] = &[
    "american",
    "australian",
    "austrian",
    "belgian",
    "brazilian",
    "british",
    "canadian",
    "danish",
    "dutch",
    "england",
    "english",
    "finnish",
    "french",
    "german",
    "germany",
    "greek",
    "home collection",
    "icelandic",
    "irish",
    "italian",
    "japanese",
    "nederlandstalig",
    "norwegian",
    "scottish",
    "spanish",
    "swedish",
    "uk",
    "united kingdom",
    "united states",
    "usa",
    "welsh",
];

/// One pill to offer: the exact value to send back in `steer`, and how many tracks are behind it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pill {
    #[serde(flatten)]
    pub steer: Steer,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestions {
    pub pills: Vec<Pill>,
    /// Tracks the rules match (up to the steering pool limit).
    pub pool: usize,
    /// How many of those have a style tag. Not counted when every match already fits.
    pub tagged: usize,
    /// Too few tracks have style tags to offer style pills.
    pub few_tags: bool,
    /// Which count the popular pill uses, which also decides its wording.
    pub popular_source: &'static str,
    /// Tracks worth an on-demand tag lookup; only filled when `few_tags` is set.
    #[serde(skip)]
    pub untagged_ids: Vec<String>,
}

fn style_steer(tag: &str, direction: &str) -> Steer {
    Steer {
        kind: "style".to_owned(),
        tag: Some(tag.to_owned()),
        bucket: None,
        direction: Some(direction.to_owned()),
        signal: None,
    }
}

fn mood_steer(bucket: &str, direction: &str) -> Steer {
    Steer {
        kind: "mood".to_owned(),
        tag: None,
        bucket: Some(bucket.to_owned()),
        direction: Some(direction.to_owned()),
        signal: None,
    }
}

fn signal_steer(signal: &str) -> Steer {
    Steer {
        kind: "signal".to_owned(),
        tag: None,
        bucket: None,
        direction: None,
        signal: Some(signal.to_owned()),
    }
}

/// tag -> pool position -> strongest evidence.
type TagMap = HashMap<String, HashMap<usize, f64>>;

fn note(map: &mut TagMap, tag: String, track: usize, affinity: f64) {
    let slot = map.entry(tag).or_default().entry(track).or_insert(0.0);
    *slot = slot.max(affinity.clamp(0.0, 1.0));
}

fn style_key(raw: &str) -> Option<String> {
    let tag = raw.trim().to_lowercase();
    let tag = canonical_tag(&tag).to_owned();
    (!NON_STYLE_TAGS.contains(&tag.as_str())).then_some(tag)
}

/// Style tags on the pool's tracks, using the same evidence the pills are weighed with.
fn read_styles(conn: &Connection, index: &HashMap<&str, usize>) -> rusqlite::Result<TagMap> {
    let mut map = TagMap::new();
    smart_steer::scan(
        conn,
        "SELECT track_id, tag, weight FROM track_tags WHERE kind IN ('genre', 'style')",
        Vec::new(),
        |id, row| {
            if let Some(&i) = index.get(id) {
                if let Some(tag) = style_key(&row.get::<_, String>(1)?) {
                    note(&mut map, tag, i, row.get(2)?);
                }
            }
            Ok(())
        },
    )?;
    smart_steer::scan(
        conn,
        "SELECT t.id, s.tag FROM album_styles s JOIN tracks t ON t.album_id = s.album_id
         WHERE s.kind IN ('genre', 'style')",
        Vec::new(),
        |id, row| {
            if let Some(&i) = index.get(id) {
                if let Some(tag) = style_key(&row.get::<_, String>(1)?) {
                    note(&mut map, tag, i, ALBUM_AFFINITY);
                }
            }
            Ok(())
        },
    )?;
    smart_steer::scan(
        conn,
        "SELECT t.id, s.style, s.weight FROM artist_styles s JOIN tracks t ON t.artist_id = s.artist_id
         WHERE s.kind IN ('genre', 'style')",
        Vec::new(),
        |id, row| {
            if let Some(&i) = index.get(id) {
                if let Some(tag) = style_key(&row.get::<_, String>(1)?) {
                    let weight = row
                        .get::<_, Option<f64>>(2)?
                        .unwrap_or(DEFAULT_ARTIST_WEIGHT);
                    note(&mut map, tag, i, weight.min(1.0) * ARTIST_AFFINITY_SCALE);
                }
            }
            Ok(())
        },
    )?;
    Ok(map)
}

fn read_moods(conn: &Connection, index: &HashMap<&str, usize>) -> rusqlite::Result<TagMap> {
    let mut map = TagMap::new();
    smart_steer::scan(
        conn,
        "SELECT track_id, bucket, weight FROM track_tags WHERE bucket IS NOT NULL",
        Vec::new(),
        |id, row| {
            if let Some(&i) = index.get(id) {
                note(&mut map, row.get(1)?, i, row.get(2)?);
            }
            Ok(())
        },
    )?;
    smart_steer::scan(
        conn,
        "SELECT t.id, s.bucket, s.weight FROM artist_styles s JOIN tracks t ON t.artist_id = s.artist_id
         WHERE s.bucket IS NOT NULL",
        Vec::new(),
        |id, row| {
            if let Some(&i) = index.get(id) {
                let weight = row
                    .get::<_, Option<f64>>(2)?
                    .unwrap_or(DEFAULT_ARTIST_WEIGHT);
                note(
                    &mut map,
                    row.get(1)?,
                    i,
                    weight.min(1.0) * ARTIST_AFFINITY_SCALE,
                );
            }
            Ok(())
        },
    )?;
    Ok(map)
}

/// Dry-runs pills against what the playlist holds now.
struct DryRun<'a> {
    def: &'a ValidDefinition,
    ids: &'a [String],
    weights: Vec<f64>,
    current: HashSet<usize>,
    places: usize,
}

impl<'a> DryRun<'a> {
    fn new(def: &'a ValidDefinition, ids: &'a [String], weights: Vec<f64>, places: usize) -> Self {
        let current = leaned_positions(def, ids, &weights, places);
        Self {
            def,
            ids,
            weights,
            current,
            places,
        }
    }

    /// Whether these factors would change enough of the playlist to notice.
    fn moves(&self, factors: &[f64]) -> bool {
        let weights: Vec<f64> = self
            .weights
            .iter()
            .zip(factors)
            .map(|(weight, factor)| weight * factor)
            .collect();
        let changed = leaned_positions(self.def, self.ids, &weights, self.places)
            .difference(&self.current)
            .count();
        changed >= MIN_CHANGED_TRACKS.max(self.places / MIN_CHANGED_SHARE)
    }
}

/// How one style or mood sits in this playlist.
struct Profile {
    name: String,
    /// Pool position -> strongest evidence.
    tracks: HashMap<usize, f64>,
    /// Share of the current playlist carrying it.
    share: f64,
    /// Share of everything the rules match.
    pool_share: f64,
    average_affinity: f64,
}

fn profiles(map: TagMap, members: &[usize], pool: usize) -> Vec<Profile> {
    let mut out: Vec<Profile> = map
        .into_iter()
        .filter(|(_, tracks)| !tracks.is_empty())
        .map(|(name, tracks)| {
            let in_playlist = members.iter().filter(|m| tracks.contains_key(m)).count();
            let support = tracks.len();
            Profile {
                name,
                share: in_playlist as f64 / members.len().max(1) as f64,
                pool_share: support as f64 / pool.max(1) as f64,
                average_affinity: tracks.values().sum::<f64>() / support as f64,
                tracks,
            }
        })
        .collect();
    // Most tracks first; the name settles ties, so the same library always gets the same pills.
    out.sort_by(|a, b| {
        b.tracks
            .len()
            .cmp(&a.tracks.len())
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

fn more_eligible(p: &Profile, min_support: usize) -> bool {
    (MIN_SHARE..DOMINANT_SHARE).contains(&p.share)
        && p.tracks.len() >= min_support
        && p.average_affinity >= MIN_AVERAGE_AFFINITY
}

fn less_eligible(p: &Profile) -> bool {
    p.share >= DOMINANT_SHARE
        && p.pool_share < MAX_LESS_POOL_SHARE
        && p.average_affinity >= MIN_AVERAGE_AFFINITY
}

/// The narrower kinds behind "Go deeper into `parent`", if there are enough of them.
fn deeper_tracks(parent: &Profile, all: &[Profile]) -> Option<usize> {
    let children: Vec<&Profile> = all
        .iter()
        .filter(|c| c.tracks.len() >= MIN_CHILD_SUPPORT && is_child_tag(&c.name, &parent.name))
        .collect();
    (parent.share >= DOMINANT_SHARE && children.len() >= MIN_CHILDREN).then(|| {
        children
            .iter()
            .flat_map(|c| c.tracks.keys())
            .collect::<HashSet<_>>()
            .len()
    })
}

/// A factor per pool position from one profile's evidence.
fn factors_from(
    tracks: &HashMap<usize, f64>,
    pool: usize,
    factor: impl Fn(f64) -> f64,
) -> Vec<f64> {
    (0..pool)
        .map(|i| factor(tracks.get(&i).copied().unwrap_or(0.0)))
        .collect()
}

/// The same factors the engine uses for "Go deeper into `parent`", without going back to the database.
fn deeper_factors(parent: &Profile, all: &[Profile], pool: usize) -> Vec<f64> {
    let mut narrower = vec![0.0_f64; pool];
    for child in all.iter().filter(|c| is_child_tag(&c.name, &parent.name)) {
        for (&track, &affinity) in &child.tracks {
            narrower[track] = narrower[track].max(affinity);
        }
    }
    narrower
        .into_iter()
        .enumerate()
        .map(|(i, narrower)| deeper_factor(parent.tracks.get(&i).copied().unwrap_or(0.0), narrower))
        .collect()
}

fn overlap(a: &HashMap<usize, f64>, b: &HashMap<usize, f64>) -> f64 {
    let shared = a.keys().filter(|track| b.contains_key(track)).count();
    let union = a.len() + b.len() - shared;
    if union == 0 {
        0.0
    } else {
        shared as f64 / union as f64
    }
}

/// Style and mood pills, at most [`MAX_TAG_PILLS`]: one "deeper", one "less" and one mood first so a large
/// "More" list can't crowd them out, then "More" by support.
fn tag_pills(
    styles: &[Profile],
    moods: &[Profile],
    hide_energy_moods: bool,
    excluded: &HashSet<String>,
    active: &HashSet<String>,
    min_support: usize,
    dry_run: &DryRun,
) -> Vec<Pill> {
    let pool = dry_run.ids.len();
    let is_active = |key: String| active.contains(&key);
    let usable = |p: &Profile| {
        !excluded.contains(&p.name)
            && (more_eligible(p, min_support)
                || less_eligible(p)
                || deeper_tracks(p, styles).is_some())
    };
    let mut kept: Vec<&Profile> = Vec::new();
    for profile in styles.iter().filter(|p| usable(p)) {
        if !kept
            .iter()
            .any(|k| overlap(&k.tracks, &profile.tracks) >= DUPLICATE_OVERLAP)
        {
            kept.push(profile);
        }
    }
    let usable_style =
        |p: &&&Profile| !is_active(CSteer::style_target(&p.name)) && !excluded.contains(&p.name);
    let deeper: Vec<Pill> = kept
        .iter()
        .filter(usable_style)
        .filter_map(|p| {
            deeper_tracks(p, styles)
                .filter(|_| dry_run.moves(&deeper_factors(p, styles, pool)))
                .map(|count| Pill {
                    steer: style_steer(&p.name, "deeper"),
                    count,
                })
        })
        .collect();
    let less: Vec<Pill> = kept
        .iter()
        .filter(usable_style)
        .filter(|p| less_eligible(p))
        .filter(|p| dry_run.moves(&factors_from(&p.tracks, pool, less_factor)))
        .map(|p| Pill {
            steer: style_steer(&p.name, "less"),
            count: p.tracks.len(),
        })
        .collect();
    let more: Vec<Pill> = kept
        .iter()
        .filter(usable_style)
        .filter(|p| more_eligible(p, min_support))
        .filter(|p| dry_run.moves(&factors_from(&p.tracks, pool, more_factor)))
        .map(|p| Pill {
            steer: style_steer(&p.name, "more"),
            count: p.tracks.len(),
        })
        .collect();
    let mood: Vec<Pill> = moods
        .iter()
        .filter(|p| !is_active(CSteer::mood_target(&p.name)))
        .filter(|p| !(hide_energy_moods && matches!(p.name.as_str(), "energetic" | "chill")))
        .filter_map(|p| {
            let (direction, factors) = if more_eligible(p, min_support) {
                ("more", factors_from(&p.tracks, pool, more_factor))
            } else if less_eligible(p) {
                ("less", factors_from(&p.tracks, pool, less_factor))
            } else {
                return None;
            };
            dry_run.moves(&factors).then(|| Pill {
                steer: mood_steer(&p.name, direction),
                count: p.tracks.len(),
            })
        })
        .collect();

    let mut deeper = deeper.into_iter();
    let mut less = less.into_iter();
    let mut mood = mood.into_iter();
    let mut more = more.into_iter();
    let first_deeper = deeper.next();
    let first_less = less.next();
    let first_mood = mood.next();
    let reserved = [&first_deeper, &first_less, &first_mood]
        .iter()
        .filter(|p| p.is_some())
        .count();
    let more_room = MAX_TAG_PILLS - reserved;
    let mut out: Vec<Pill> = first_deeper.into_iter().collect();
    out.extend(more.by_ref().take(more_room));
    out.extend(first_less);
    out.extend(first_mood);
    // Short of "More" pills: the next best of the others fill the rest.
    let room = MAX_TAG_PILLS.saturating_sub(out.len());
    out.extend(deeper.chain(less).chain(mood).take(room));
    out
}

/// A non-style pill that has enough tracks or values behind it.
struct SignalChoice {
    name: &'static str,
    signal: Signal,
    tracks: usize,
}

/// The other pills with enough behind them, best first, plus whether the energy pills are among them.
fn signal_choices(
    conn: &Connection,
    index: &HashMap<&str, usize>,
    owner: &str,
    now: &str,
    active: &HashSet<String>,
) -> rusqlite::Result<(Vec<SignalChoice>, bool)> {
    let pool = index.len();
    let owner_param = || vec![Value::Text(owner.to_owned())];

    let starred = smart_steer::pool_flags(
        conn,
        index,
        "SELECT item_id FROM subsonic_stars WHERE user_id = ?1 AND item_type = 'track'",
        owner_param(),
    )?;
    let rated = smart_steer::pool_flags(
        conn,
        index,
        "SELECT track_id FROM track_ratings WHERE user_id = ?1 AND rating >= 4",
        owner_param(),
    )?;
    let favourites = starred
        .iter()
        .zip(&rated)
        .filter(|(starred, rated)| **starred || **rated)
        .count();

    let any_plays = smart_steer::pool_flags(
        conn,
        index,
        "SELECT track_id FROM user_track_stats WHERE play_count > 0",
        Vec::new(),
    )?;
    let played = any_plays.iter().filter(|p| **p).count();

    let own_plays = smart_steer::pool_flags(
        conn,
        index,
        "SELECT track_id FROM user_track_stats WHERE user_id = ?1 AND play_count > 0",
        owner_param(),
    )?;
    let recent = smart_steer::pool_flags(
        conn,
        index,
        &format!("SELECT id FROM tracks WHERE added_at >= datetime(?1, '-{NEW_TRACK_DAYS} days')"),
        vec![Value::Text(now.to_owned())],
    )?;
    let fresh = own_plays
        .iter()
        .zip(&recent)
        .filter(|(played, recent)| !**played || **recent)
        .count();

    let sound = |sql: &str| -> rusqlite::Result<(usize, bool)> {
        let values = smart_steer::pool_values(conn, index, sql, None)?;
        let known: Vec<f64> = values.iter().flatten().copied().collect();
        let varied = known.windows(2).any(|pair| pair[0] != pair[1]);
        let enough = known.len() as f64 >= MIN_SOUND_SHARE * pool as f64;
        Ok((known.len(), enough && varied))
    };
    let (bpm_known, bpm_ok) = sound(&bpm_sql())?;
    let (energy_known, energy_ok) = sound(ENERGY_SQL)?;

    let useful = |tracks: usize| tracks >= MIN_SIGNAL_TRACKS && tracks < pool;
    let choices = [
        (
            "favourites",
            Signal::Favourites,
            useful(favourites),
            favourites,
        ),
        ("popular", Signal::Popular, useful(played), played),
        ("energetic", Signal::Energetic, energy_ok, energy_known),
        ("new", Signal::New, useful(fresh), fresh),
        ("calmer", Signal::Calmer, energy_ok, energy_known),
        ("faster", Signal::Faster, bpm_ok, bpm_known),
        ("slower", Signal::Slower, bpm_ok, bpm_known),
    ]
    .into_iter()
    .filter(|(_, signal, eligible, _)| {
        *eligible && !active.contains(&CSteer::Signal(*signal).target())
    })
    .map(|(name, signal, _, tracks)| SignalChoice {
        name,
        signal,
        tracks,
    })
    .collect();
    Ok((choices, energy_ok))
}

/// The rule's own genres, which can't be steered towards: every track already has them.
fn own_genres(def: &ValidDefinition) -> HashSet<String> {
    def.positive_genres()
        .iter()
        .flat_map(|label| label.split([',', '/', ';']))
        .map(|part| canonical_tag(&part.trim().to_lowercase()).to_owned())
        .filter(|part| !part.is_empty())
        .collect()
}

/// The pills to offer for `def` right now. Pills already on are never offered again; the caller shows those
/// from the definition. See the module docs for when nothing is offered.
pub fn suggest_pills(
    conn: &Connection,
    owner: &str,
    def: &ValidDefinition,
    now: &str,
) -> Result<Suggestions, EvalError> {
    let started = std::time::Instant::now();
    let budget = || DEFAULT_QUERY_BUDGET.saturating_sub(started.elapsed());
    let ids = matching_pool(conn, owner, def, now, budget())?;
    let selection = evaluate_with_budget(conn, owner, def, now, budget())?;
    let popular_source = "local";
    if selection.track_ids.len() >= ids.len() {
        // Every match already gets in, so no pill could change what's in the playlist.
        return Ok(Suggestions {
            pills: Vec::new(),
            pool: ids.len(),
            tagged: 0,
            few_tags: false,
            popular_source,
            untagged_ids: Vec::new(),
        });
    }

    let index: HashMap<&str, usize> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    let members: Vec<usize> = selection
        .track_ids
        .iter()
        .filter_map(|id| index.get(id.as_str()).copied())
        .collect();
    let active: HashSet<String> = def.steers().iter().map(CSteer::target).collect();
    let pool = ids.len();

    crate::smart_playlists::with_query_budget(conn, budget(), || {
        let styles_map = read_styles(conn, &index)?;
        let tagged_tracks: BTreeSet<usize> = styles_map
            .values()
            .flat_map(|tracks| tracks.keys().copied())
            .collect();
        let few_tags = (tagged_tracks.len() as f64) < MIN_TAGGED_SHARE * pool as f64;
        let (signals, energy_offered) = signal_choices(conn, &index, owner, now, &active)?;
        let current_weights = if def.steers().is_empty() {
            vec![1.0; pool]
        } else {
            steer_weights(conn, owner, now, def.steers(), &ids)?
        };
        let dry_run = DryRun::new(def, &ids, current_weights, members.len());

        let mut pills = if few_tags {
            Vec::new()
        } else {
            let min_support = MIN_SUPPORT.max(def.max_tracks() as usize / 10);
            let styles = profiles(styles_map, &members, pool);
            let moods = profiles(read_moods(conn, &index)?, &members, pool);
            tag_pills(
                &styles,
                &moods,
                energy_offered,
                &own_genres(def),
                &active,
                min_support,
                &dry_run,
            )
        };
        let room = MAX_PILLS.saturating_sub(def.steers().len());
        pills.truncate(room);
        for choice in signals {
            if pills.len() >= room {
                break;
            }
            let factors = signal_factors(conn, &index, owner, now, choice.signal)?;
            if dry_run.moves(&factors) {
                pills.push(Pill {
                    steer: signal_steer(choice.name),
                    count: choice.tracks,
                });
            }
        }

        let untagged_ids = if few_tags {
            ids.iter()
                .enumerate()
                .filter(|(i, _)| !tagged_tracks.contains(i))
                .take(MAX_UNTAGGED)
                .map(|(_, id)| id.clone())
                .collect()
        } else {
            Vec::new()
        };
        Ok(Suggestions {
            pills,
            pool,
            tagged: tagged_tracks.len(),
            few_tags,
            popular_source,
            untagged_ids,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smart_playlists::parse_definition;
    use serde_json::{json, Value as Json};

    const NOW: &str = "2026-11-01 12:00:00";

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO users(id, username) VALUES ('alice', 'alice'), ('bob', 'bob');
             INSERT INTO libraries(id, path, name) VALUES ('lib1', '/m1', 'Music');",
        )
        .unwrap();
        conn
    }

    /// Adds an artist with `tracks` Metal tracks and the given artist tags (weight 1 gives affinity 0.6).
    fn artist(conn: &Connection, id: &str, tracks: usize, tags: &[(&str, f64)]) {
        conn.execute(
            "INSERT INTO artists(id, name) VALUES (?1, ?1)",
            rusqlite::params![id],
        )
        .unwrap();
        for (tag, weight) in tags {
            conn.execute(
                "INSERT INTO artist_styles(artist_id, style, kind, weight) VALUES (?1, ?2, 'genre', ?3)",
                rusqlite::params![id, tag, weight],
            )
            .unwrap();
        }
        for n in 0..tracks {
            conn.execute(
                "INSERT INTO tracks(id, library_id, artist_id, title, file_path, genre, duration, added_at)
                 VALUES (?1, 'lib1', ?2, ?1, ?1, 'Metal', 200, '2025-01-01 00:00:00')",
                rusqlite::params![format!("{id}-{n:03}"), id],
            )
            .unwrap();
        }
    }

    fn definition(steer: Json, max_tracks: u32) -> ValidDefinition {
        parse_definition(
            &json!({
                "schemaVersion": 1,
                "root": { "mode": "all", "children": [
                    { "field": "genre", "operator": "is", "value": "Metal" }
                ] },
                "sort": [{ "field": "shuffle", "seed": 7 }],
                "limit": { "maxTracks": max_tracks },
                "steer": steer,
            })
            .to_string(),
        )
        .unwrap()
    }

    /// Pills as readable words: "deeper black metal", "more sludge", "signal new".
    fn words(s: &Suggestions) -> Vec<String> {
        s.pills
            .iter()
            .map(|p| {
                let steer = &p.steer;
                match steer.kind.as_str() {
                    "signal" => format!("signal {}", steer.signal.as_deref().unwrap()),
                    "mood" => format!(
                        "mood {} {}",
                        steer.direction.as_deref().unwrap(),
                        steer.bucket.as_deref().unwrap()
                    ),
                    _ => format!(
                        "{} {}",
                        steer.direction.as_deref().unwrap(),
                        steer.tag.as_deref().unwrap()
                    ),
                }
            })
            .collect()
    }

    fn tag_words(s: &Suggestions) -> Vec<String> {
        words(s)
            .into_iter()
            .filter(|w| !w.starts_with("signal"))
            .collect()
    }

    fn suggest(conn: &Connection, def: &ValidDefinition) -> Suggestions {
        suggest_pills(conn, "alice", def, NOW).unwrap()
    }

    #[test]
    fn w1_dominant_style_goes_deeper_or_less_and_a_rarer_one_gets_more() {
        let conn = db();
        artist(&conn, "bm", 240, &[("black metal", 1.0)]);
        artist(&conn, "sl", 80, &[("sludge", 1.0)]);
        artist(&conn, "th", 80, &[("thrash", 1.0)]);
        for (i, child) in [
            (0, "atmospheric black metal"),
            (1, "depressive black metal"),
        ] {
            for n in 0..30 {
                conn.execute(
                    "INSERT INTO track_tags(track_id, tag, kind, weight, source)
                     VALUES (?1, ?2, 'genre', 0.8, 'lastfm')",
                    rusqlite::params![format!("bm-{:03}", n + i * 40), child],
                )
                .unwrap();
            }
        }
        let got = suggest(&conn, &definition(json!([]), 100));
        let tags = tag_words(&got);
        for expected in ["deeper black metal", "less black metal", "more sludge"] {
            assert!(tags.contains(&expected.to_owned()), "{expected}: {tags:?}");
        }
        assert_eq!(got.pool, 400);
        let deeper = got
            .pills
            .iter()
            .find(|p| p.steer.direction.as_deref() == Some("deeper"))
            .unwrap();
        assert_eq!(deeper.count, 60, "tracks in the narrower kinds");
    }

    #[test]
    fn w2_nothing_for_the_rules_own_genre_but_its_children_still_appear() {
        let conn = db();
        artist(&conn, "a", 150, &[("metal", 1.0), ("doom metal", 1.0)]);
        artist(&conn, "b", 40, &[("metal", 1.0), ("thrash metal", 1.0)]);
        artist(&conn, "c", 10, &[("other", 1.0)]);
        let tags = tag_words(&suggest(&conn, &definition(json!([]), 50)));
        for own in ["more metal", "less metal", "deeper metal"] {
            assert!(!tags.contains(&own.to_owned()), "{tags:?}");
        }
        assert!(tags.contains(&"more thrash metal".to_owned()), "{tags:?}");
    }

    #[test]
    fn w2_a_style_filling_the_pool_gets_no_less_pill() {
        let conn = db();
        artist(&conn, "a", 170, &[("doom metal", 1.0)]);
        artist(&conn, "b", 30, &[("thrash metal", 1.0)]);
        let tags = tag_words(&suggest(&conn, &definition(json!([]), 50)));
        assert!(!tags.contains(&"less doom metal".to_owned()), "{tags:?}");
    }

    #[test]
    fn w3_weakly_tied_styles_get_no_pill() {
        let conn = db();
        artist(&conn, "weak", 60, &[("punk", 0.4)]);
        artist(&conn, "firm", 60, &[("sludge", 1.0)]);
        artist(&conn, "rest", 80, &[("doom metal", 1.0)]);
        let tags = tag_words(&suggest(&conn, &definition(json!([]), 50)));
        assert!(!tags.contains(&"more punk".to_owned()), "{tags:?}");
        assert!(tags.contains(&"more sludge".to_owned()), "{tags:?}");
    }

    #[test]
    fn w4_aliases_fold_and_styles_on_the_same_tracks_collapse() {
        let conn = db();
        artist(&conn, "g1", 30, &[("gothic rock", 1.0)]);
        artist(&conn, "g2", 15, &[("goth rock", 1.0)]);
        artist(
            &conn,
            "alt",
            30,
            &[("indie rock", 1.0), ("alternative rock", 1.0)],
        );
        artist(&conn, "rest", 125, &[("doom metal", 1.0)]);
        let got = suggest(&conn, &definition(json!([]), 50));
        let tags = tag_words(&got);
        assert!(tags.contains(&"more gothic rock".to_owned()), "{tags:?}");
        assert!(!tags.iter().any(|t| t.contains("goth rock")), "{tags:?}");
        let gothic = got
            .pills
            .iter()
            .find(|p| p.steer.tag.as_deref() == Some("gothic rock"))
            .unwrap();
        assert_eq!(gothic.count, 45, "both spellings count");
        assert!(
            tags.contains(&"more alternative rock".to_owned()),
            "{tags:?}"
        );
        assert!(!tags.contains(&"more indie rock".to_owned()), "{tags:?}");
    }

    #[test]
    fn w5_places_and_eras_are_not_styles() {
        let conn = db();
        artist(
            &conn,
            "a",
            60,
            &[("british", 1.0), ("german", 1.0), ("sludge", 1.0)],
        );
        artist(&conn, "b", 140, &[("doom metal", 1.0)]);
        conn.execute(
            "INSERT INTO artist_styles(artist_id, style, kind, weight) VALUES ('b', '80s', 'era', 1.0)",
            [],
        )
        .unwrap();
        let tags = tag_words(&suggest(&conn, &definition(json!([]), 50)));
        for absent in ["british", "german", "80s"] {
            assert!(!tags.iter().any(|t| t.contains(absent)), "{tags:?}");
        }
        assert!(tags.contains(&"more sludge".to_owned()), "{tags:?}");
    }

    #[test]
    fn w6_few_tags_offers_only_signals_and_names_tracks_to_look_up() {
        let conn = db();
        artist(&conn, "tagged", 30, &[("sludge", 1.0)]);
        artist(&conn, "bare", 270, &[]);
        conn.execute_batch(
            "INSERT INTO play_history(id, user_id, track_id, played_at)
             SELECT 'h' || id, 'alice', id, '2026-10-01 10:00:00' FROM tracks WHERE id LIKE 'bare-00%';",
        )
        .unwrap();
        let got = suggest(&conn, &definition(json!([]), 100));
        assert!(got.few_tags);
        assert_eq!((got.tagged, got.pool), (30, 300));
        assert!(tag_words(&got).is_empty(), "{:?}", words(&got));
        assert!(words(&got).contains(&"signal popular".to_owned()));
        assert_eq!(got.untagged_ids.len(), 200);
        assert!(got.untagged_ids.iter().all(|id| id.starts_with("bare-")));
    }

    #[test]
    fn w7_no_pills_when_every_match_already_fits() {
        let conn = db();
        artist(&conn, "a", 80, &[("sludge", 1.0)]);
        let got = suggest(&conn, &definition(json!([]), 100));
        assert!(got.pills.is_empty());
        assert!(!got.few_tags, "no message either");
        assert_eq!(got.pool, 80);
    }

    #[test]
    fn w8_signals_need_enough_tracks_or_values() {
        let conn = db();
        artist(&conn, "a", 200, &[]);
        // 3 favourites, 25 played, BPM on everything, energy on most.
        conn.execute_batch(
            "INSERT INTO track_ratings(user_id, track_id, rating)
             SELECT 'alice', id, 5 FROM tracks WHERE id IN ('a-000', 'a-001', 'a-002');
             INSERT INTO play_history(id, user_id, track_id, played_at)
             SELECT 'h' || id, 'alice', id, '2026-10-01 10:00:00' FROM tracks ORDER BY id LIMIT 25;
             UPDATE tracks SET bpm_detected = 60 + rowid % 90;
             INSERT INTO track_deep_analysis(track_id, energy_score_refined, confidence)
             SELECT id, (rowid % 10) / 10.0, 0.9 FROM tracks ORDER BY id LIMIT 164;",
        )
        .unwrap();
        let got = words(&suggest(&conn, &definition(json!([]), 50)));
        assert!(!got.contains(&"signal favourites".to_owned()), "{got:?}");
        for expected in ["popular", "new", "energetic", "calmer", "faster", "slower"] {
            assert!(got.contains(&format!("signal {expected}")), "{got:?}");
        }

        // Too little known: the sound pills go away.
        conn.execute_batch(
            "UPDATE tracks SET bpm_detected = NULL WHERE rowid % 4 <> 0;
             DELETE FROM track_deep_analysis WHERE rowid % 4 <> 0;",
        )
        .unwrap();
        let got = words(&suggest(&conn, &definition(json!([]), 50)));
        for absent in ["energetic", "calmer", "faster", "slower"] {
            assert!(!got.contains(&format!("signal {absent}")), "{got:?}");
        }
    }

    #[test]
    fn signals_that_every_track_qualifies_for_are_not_offered() {
        let conn = db();
        artist(&conn, "a", 200, &[]);
        let got = words(&suggest(&conn, &definition(json!([]), 50)));
        assert!(!got.contains(&"signal new".to_owned()), "{got:?}");
        assert!(!got.contains(&"signal faster".to_owned()), "{got:?}");
    }

    #[test]
    fn at_most_eight_with_room_for_every_kind_and_no_repeats() {
        let conn = db();
        artist(&conn, "main", 200, &[("black metal", 1.0)]);
        for (n, name) in ["atmospheric", "depressive", "raw"].iter().enumerate() {
            for t in 0..12 {
                conn.execute(
                    "INSERT INTO track_tags(track_id, tag, kind, weight, source)
                     VALUES (?1, ?2, 'genre', 0.9, 'lastfm')",
                    rusqlite::params![
                        format!("main-{:03}", n * 12 + t),
                        format!("{name} black metal")
                    ],
                )
                .unwrap();
            }
        }
        for name in [
            "sludge", "thrash", "grind", "stoner", "drone", "speed", "folk",
        ] {
            artist(&conn, name, 30, &[(name, 1.0)]);
        }
        conn.execute_batch(
            "INSERT INTO play_history(id, user_id, track_id, played_at)
             SELECT 'h' || id, 'alice', id, '2026-10-01 10:00:00' FROM tracks ORDER BY id LIMIT 25;
             INSERT INTO track_ratings(user_id, track_id, rating)
             SELECT 'alice', id, 5 FROM tracks ORDER BY id LIMIT 20;
             UPDATE tracks SET bpm_detected = 60 + rowid % 90;",
        )
        .unwrap();
        let got = suggest(&conn, &definition(json!([]), 100));
        let all = words(&got);
        assert_eq!(all.len(), MAX_PILLS, "{all:?}");
        let tags = tag_words(&got);
        assert_eq!(tags.len(), MAX_TAG_PILLS, "{all:?}");
        assert_eq!(tags[0], "deeper black metal", "{all:?}");
        assert!(tags.contains(&"less black metal".to_owned()), "{all:?}");
        assert_eq!(
            all.iter().collect::<HashSet<_>>().len(),
            all.len(),
            "{all:?}"
        );
    }

    #[test]
    fn pills_already_on_are_not_offered_and_shares_follow_the_lean() {
        let conn = db();
        artist(&conn, "sl", 40, &[("sludge", 1.0)]);
        artist(&conn, "dm", 160, &[("doom metal", 1.0)]);
        conn.execute_batch("UPDATE tracks SET bpm_detected = 60 + rowid % 90;")
            .unwrap();
        let steer = json!([
            { "kind": "style", "tag": "sludge", "direction": "more" },
            { "kind": "signal", "signal": "faster" },
        ]);
        let got = words(&suggest(&conn, &definition(steer, 50)));
        assert!(!got.iter().any(|w| w.contains("sludge")), "{got:?}");
        assert!(!got.contains(&"signal faster".to_owned()), "{got:?}");
        assert!(
            !got.contains(&"signal slower".to_owned()),
            "the opposite would be refused: {got:?}"
        );
    }

    #[test]
    fn pills_already_on_count_towards_the_eight() {
        let conn = db();
        for name in [
            "sludge", "thrash", "grind", "stoner", "drone", "speed", "folk", "crust",
        ] {
            artist(&conn, name, 30, &[(name, 1.0)]);
        }
        conn.execute_batch(
            "INSERT INTO play_history(id, user_id, track_id, played_at)
             SELECT 'h' || id, 'alice', id, '2026-10-01 10:00:00' FROM tracks ORDER BY id LIMIT 25;
             INSERT INTO track_ratings(user_id, track_id, rating)
             SELECT 'alice', id, 5 FROM tracks ORDER BY id LIMIT 20;
             UPDATE tracks SET bpm_detected = 60 + rowid % 90;",
        )
        .unwrap();
        let steer = json!([
            { "kind": "style", "tag": "sludge", "direction": "more" },
            { "kind": "style", "tag": "thrash", "direction": "more" },
        ]);
        let got = suggest(&conn, &definition(steer, 60));
        assert_eq!(got.pills.len(), MAX_PILLS - 2, "{:?}", words(&got));
    }

    #[test]
    fn mood_pills_appear_when_backed_and_step_aside_for_energy() {
        let conn = db();
        artist(&conn, "a", 60, &[("sludge", 1.0)]);
        artist(&conn, "b", 140, &[("doom metal", 1.0)]);
        conn.execute_batch(
            "INSERT INTO artist_styles(artist_id, style, kind, bucket, weight) VALUES
               ('a', 'melancholic', 'mood', 'melancholic', 1.0), ('a', 'energetic', 'mood', 'energetic', 1.0);",
        )
        .unwrap();
        let got = tag_words(&suggest(&conn, &definition(json!([]), 50)));
        assert!(
            got.contains(&"mood more melancholic".to_owned())
                || got.contains(&"mood more energetic".to_owned()),
            "{got:?}"
        );

        conn.execute_batch(
            "INSERT INTO track_deep_analysis(track_id, energy_score_refined, confidence)
             SELECT id, (rowid % 10) / 10.0, 0.9 FROM tracks;",
        )
        .unwrap();
        let got = tag_words(&suggest(&conn, &definition(json!([]), 50)));
        assert!(!got.contains(&"mood more energetic".to_owned()), "{got:?}");
        assert!(got.contains(&"mood more melancholic".to_owned()), "{got:?}");
    }

    #[test]
    fn a_pill_that_would_change_almost_nothing_is_not_offered() {
        let conn = db();
        // In title order the "aaa" artist fills the first places; "zzz" is too far down for a lean to reach.
        artist(&conn, "aaa", 60, &[("sludge", 1.0)]);
        artist(&conn, "mmm", 200, &[("doom metal", 1.0)]);
        artist(&conn, "zzz", 40, &[("thrash metal", 1.0)]);
        let by_title = parse_definition(
            &json!({
                "schemaVersion": 1,
                "root": { "mode": "all", "children": [
                    { "field": "genre", "operator": "is", "value": "Metal" }
                ] },
                "sort": [{ "field": "title" }],
                "limit": { "maxTracks": 50 },
            })
            .to_string(),
        )
        .unwrap();
        let tags = tag_words(&suggest(&conn, &by_title));
        assert!(!tags.contains(&"more thrash metal".to_owned()), "{tags:?}");
        assert!(
            !tags.contains(&"more sludge".to_owned()),
            "already in: {tags:?}"
        );
    }

    mod saved {
        use super::*;
        use crate::music::{coerce_entity_id, EntityId};
        use crate::smart_playlists::{
            create_smart_playlist, playlist_snapshot, set_steer, smart_definition, steer_view,
            SmartError,
        };

        fn saved_playlist(conn: &Connection) -> EntityId {
            artist(conn, "sl", 60, &[("sludge", 1.0)]);
            artist(conn, "dm", 140, &[("doom metal", 1.0)]);
            create_smart_playlist(conn, "Metal", "", "alice", &definition(json!([]), 50))
                .unwrap()
                .id
        }

        fn in_playlist(conn: &Connection, id: &EntityId) -> Vec<String> {
            playlist_snapshot(conn, id, "alice", false)
                .unwrap()
                .unwrap()
                .tracks
                .into_iter()
                .map(|t| t.id.to_string())
                .collect()
        }

        #[test]
        fn tapping_a_pill_changes_the_tracks_and_turning_it_off_restores_them() {
            let conn = db();
            let id = saved_playlist(&conn);
            let before = in_playlist(&conn, &id);
            let start = steer_view(&conn, &id, "alice").unwrap();
            assert!(start.active.is_empty());
            assert!(start.added_track_ids.is_empty());
            let pill = start
                .pills
                .iter()
                .find(|p| p.steer.tag.as_deref() == Some("sludge"))
                .expect("offers sludge")
                .steer
                .clone();

            let on = set_steer(&conn, &id, "alice", &json!([pill])).unwrap();
            assert_eq!(on.active, std::slice::from_ref(&pill));
            assert_eq!(on.edit_revision, start.edit_revision + 1);
            assert!(!on
                .pills
                .iter()
                .any(|p| p.steer.tag.as_deref() == Some("sludge")));
            let during = in_playlist(&conn, &id);
            assert_ne!(during, before);
            assert_eq!(during.len(), 50);
            assert!(!on.added_track_ids.is_empty());
            for added in &on.added_track_ids {
                assert!(during.contains(added), "{added} is in the playlist");
                assert!(!before.contains(added), "{added} wasn't before");
            }

            // Stored with the rules, so it survives a reload.
            let rules = smart_definition(&conn, &id, "alice").unwrap().unwrap();
            assert_eq!(rules.definition["steer"][0]["tag"], "sludge");

            let off = set_steer(&conn, &id, "alice", &json!([])).unwrap();
            assert!(off.active.is_empty());
            assert_eq!(in_playlist(&conn, &id), before, "the earlier list exactly");
            let rules = smart_definition(&conn, &id, "alice").unwrap().unwrap();
            assert!(rules.definition.get("steer").is_none());
        }

        #[test]
        fn a_bad_pill_changes_nothing() {
            let conn = db();
            let id = saved_playlist(&conn);
            let before = in_playlist(&conn, &id);
            let revision = steer_view(&conn, &id, "alice").unwrap().edit_revision;
            for bad in [
                json!([{ "kind": "weather" }]),
                json!("sludge"),
                json!([{ "kind": "signal", "signal": "new", "extra": 1 }]),
            ] {
                assert!(
                    matches!(
                        set_steer(&conn, &id, "alice", &bad),
                        Err(SmartError::Invalid(_))
                    ),
                    "{bad}"
                );
            }
            assert_eq!(in_playlist(&conn, &id), before);
            assert_eq!(
                steer_view(&conn, &id, "alice").unwrap().edit_revision,
                revision
            );
        }

        #[test]
        fn only_the_owners_smart_playlists_have_pills() {
            let conn = db();
            let id = saved_playlist(&conn);
            assert!(matches!(
                steer_view(&conn, &id, "bob"),
                Err(SmartError::NotFound)
            ));
            assert!(matches!(
                set_steer(&conn, &id, "bob", &json!([])),
                Err(SmartError::NotFound)
            ));
            assert!(matches!(
                steer_view(&conn, &coerce_entity_id("missing"), "alice"),
                Err(SmartError::NotFound)
            ));
        }

        #[test]
        fn rules_from_a_newer_version_are_left_alone() {
            let conn = db();
            let id = saved_playlist(&conn);
            conn.execute(
                "UPDATE smart_playlist_definitions SET schema_version = 99 WHERE playlist_id = ?1",
                [&id],
            )
            .unwrap();
            assert!(matches!(
                set_steer(&conn, &id, "alice", &json!([])),
                Err(SmartError::Unsupported)
            ));
            assert!(matches!(
                steer_view(&conn, &id, "alice"),
                Err(SmartError::Unsupported)
            ));
        }
    }

    #[test]
    fn suggestions_are_repeatable_and_can_be_sent_back_as_they_are() {
        let conn = db();
        artist(&conn, "sl", 60, &[("sludge", 1.0)]);
        artist(&conn, "dm", 140, &[("doom metal", 1.0)]);
        let first = suggest(&conn, &definition(json!([]), 50));
        assert_eq!(first, suggest(&conn, &definition(json!([]), 50)));
        assert!(!first.pills.is_empty());
        let json_form = serde_json::to_value(&first).unwrap();
        let pill = &json_form["pills"][0];
        assert!(pill["count"].is_number(), "{pill}");
        assert_eq!(json_form["popularSource"], "local");
        assert!(json_form.get("untaggedIds").is_none());
        let sent_back: Vec<Steer> = first.pills.iter().map(|p| p.steer.clone()).collect();
        for steer in sent_back {
            let as_json = serde_json::to_value(&steer).unwrap();
            let valid = definition(json!([as_json]), 50);
            assert_eq!(valid.steers().len(), 1);
        }
    }
}
