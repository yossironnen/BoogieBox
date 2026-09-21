//! Artist Radio v2 ranking and selection (`wip/artist-radio-v2-plan.md` §4).
//!
//! The ranking core is pure: candidate pools come in, an ordered, explained
//! queue comes out. All randomness is injected as a `FnMut() -> f64` in
//! `(0, 1)` so tests are deterministic. [`load_inputs`] and [`radio_options`]
//! at the bottom are the only parts that read the database (never the network).

use crate::{
    similar_artists::{resolve_local_similar_artists_with_listenbrainz, RelatedSet},
    tag_taxonomy::{mood_bucket_of, mood_similarity, tags_for_bucket, MOOD_BUCKETS},
};
use boogiebox_db::{
    music::EntityId,
    radio::{
        count_tracks_with_any_tag, load_radio_tracks, radio_ids_for_artists,
        radio_ids_matching_tags, recent_play_ages, seed_audio_profile, seed_tag_vector,
        track_tag_progress, RadioTrack, SeedAudio, TagLayer, TagWeight, SOURCE_LASTFM,
    },
};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Slider position that yields exactly the documented 30/45/25 mix.
pub const DEFAULT_VARIETY: f64 = 0.45;
/// Recent plays (days) considered when penalising repeats.
pub const RECENT_PLAY_WINDOW_DAYS: i64 = 14;

/// Very broad tags say little about similarity; they count half.
const GENERIC_TAGS: &[&str] = &[
    "rock",
    "pop",
    "electronic",
    "alternative",
    "indie",
    "alternative rock",
    "indie rock",
];

// Scoring weights (relative; positive terms are renormalised over the terms
// that actually apply to a candidate, so missing data never zeroes a track).
const W_ARTIST: f64 = 0.35;
const W_TAGS: f64 = 0.30;
const W_MOOD: f64 = 0.20;
const W_AUDIO: f64 = 0.10;
const W_RATING: f64 = 0.15;
const W_RECENT: f64 = 0.45;
/// Floor so nothing is ever impossible to draw.
const MIN_SAMPLE_WEIGHT: f64 = 0.02;

/// What the user asked the radio to emphasise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// The artist and artists like it (default).
    Similar,
    /// Lean on mood matches.
    Mood,
}

impl Focus {
    pub fn parse(value: &str) -> Option<Focus> {
        match value {
            "similar" => Some(Focus::Similar),
            "mood" => Some(Focus::Mood),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RadioOptions {
    pub limit: usize,
    pub focus: Focus,
    /// Requested mood buckets; empty = auto-pick from the seed artist.
    pub moods: Vec<String>,
    /// 0 = familiar … 1 = adventurous.
    pub variety: f64,
}

/// Why a track is in the queue, shown as a chip in the client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RadioReason {
    /// `seed` | `similar` | `mood` | `style`.
    pub kind: &'static str,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pool {
    Seed,
    Similar,
    Mood,
}

const POOLS: [Pool; 3] = [Pool::Seed, Pool::Similar, Pool::Mood];

impl Pool {
    fn index(self) -> usize {
        match self {
            Pool::Seed => 0,
            Pool::Similar => 1,
            Pool::Mood => 2,
        }
    }
}

/// Everything the engine needs, already fetched.
pub struct RadioInputs {
    pub seed_artist_id: EntityId,
    pub seed_artist_name: String,
    pub seed_tags: Vec<TagWeight>,
    pub seed_audio: SeedAudio,
    pub seed_pool: Vec<RadioTrack>,
    pub similar_pool: Vec<RadioTrack>,
    /// Provider similarity score (0..1) per similar artist.
    pub similar_artists: HashMap<EntityId, f64>,
    pub mood_pool: Vec<RadioTrack>,
    /// Days since this user last played a track (recent plays only).
    pub recent_ages: HashMap<EntityId, f64>,
}

pub struct PlannedTrack {
    pub track: RadioTrack,
    pub reason: RadioReason,
}

/// Achieved share of each source in the final queue.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct MixShares {
    pub seed: f64,
    pub similar: f64,
    pub mood: f64,
}

/// How much of the queue is backed by track-level tags (vs fallback layers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Coverage {
    pub tagged: usize,
    pub candidates: usize,
}

pub struct RadioPlan {
    pub tracks: Vec<PlannedTrack>,
    /// Mood buckets that were in effect (requested, or auto-picked).
    pub moods: Vec<String>,
    pub mix: MixShares,
    pub coverage: Coverage,
    pub degraded: Option<String>,
}

// ── Moods and tags ──────────────────────────────────────────────────────────

/// The seed's dominant mood buckets (up to two), from its tag vector.
pub fn auto_moods(seed_tags: &[TagWeight]) -> Vec<String> {
    let mut totals: HashMap<&'static str, f64> = HashMap::new();
    for tag in seed_tags {
        if let Some(bucket) = mood_bucket_of(&tag.tag) {
            *totals.entry(bucket).or_default() += tag.weight;
        }
    }
    let mut ranked: Vec<(&str, f64)> = totals.into_iter().filter(|(_, sum)| *sum >= 0.15).collect();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(b.0))
    });
    ranked
        .into_iter()
        .take(2)
        .map(|(b, _)| b.to_owned())
        .collect()
}

/// Requested buckets (unknown names dropped), or the auto-picked ones.
pub fn effective_moods(requested: &[String], seed_tags: &[TagWeight]) -> Vec<String> {
    let mut moods: Vec<String> = Vec::new();
    for name in requested {
        let name = name.trim().to_lowercase();
        if MOOD_BUCKETS.contains(&name.as_str()) && !moods.contains(&name) {
            moods.push(name);
        }
    }
    if moods.is_empty() {
        auto_moods(seed_tags)
    } else {
        moods
    }
}

/// Tags used to pull the mood/style candidate pool: the seed's leading tags
/// plus every raw tag of the chosen moods. Mood focus keeps only a few genre
/// tags so the moods drive the result.
pub fn pool_tags(seed_tags: &[TagWeight], moods: &[String], focus: Focus) -> Vec<String> {
    let genre_count = match focus {
        Focus::Similar => 12,
        Focus::Mood => 3,
    };
    let mut tags: Vec<String> = Vec::new();
    for tag in seed_tags
        .iter()
        .filter(|t| mood_bucket_of(&t.tag).is_none())
        .take(genre_count)
    {
        tags.push(tag.tag.clone());
    }
    for mood in moods {
        for tag in tags_for_bucket(mood) {
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
    }
    tags
}

// ── Mix shares ──────────────────────────────────────────────────────────────

/// `(seed, similar, mood)` target shares for a focus and variety slider.
pub fn target_shares(focus: Focus, variety: f64) -> [f64; 3] {
    let base = match focus {
        Focus::Similar => [0.30, 0.45, 0.25],
        Focus::Mood => [0.10, 0.25, 0.65],
    };
    let shift = (variety.clamp(0.0, 1.0) - DEFAULT_VARIETY) * 0.3;
    let seed = (base[0] - shift).clamp(0.05, 0.45);
    let moved = base[0] - seed;
    let shares = [seed, base[1] + moved / 2.0, base[2] + moved / 2.0];
    let total: f64 = shares.iter().sum();
    [shares[0] / total, shares[1] / total, shares[2] / total]
}

/// Splits `limit` into per-pool quotas: empty pools give their share away,
/// quotas never exceed what a pool can supply, and leftovers go to whichever
/// pool still has spare tracks (largest share first).
fn allocate(limit: usize, shares: [f64; 3], available: [usize; 3]) -> [usize; 3] {
    let mut weights = shares;
    for i in 0..3 {
        if available[i] == 0 {
            weights[i] = 0.0;
        }
    }
    let total: f64 = weights.iter().sum();
    if total <= 0.0 {
        return [0; 3];
    }
    let mut quota = [0usize; 3];
    let mut remainders = [(0usize, 0.0f64); 3];
    let mut assigned = 0;
    for i in 0..3 {
        let exact = limit as f64 * weights[i] / total;
        quota[i] = (exact.floor() as usize).min(available[i]);
        remainders[i] = (i, exact - exact.floor());
        assigned += quota[i];
    }
    remainders.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut order: Vec<usize> = remainders.iter().map(|(i, _)| *i).collect();
    // Then by share so a spare slot lands where it is most wanted.
    order.sort_by(|a, b| {
        weights[*b]
            .partial_cmp(&weights[*a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    while assigned < limit {
        let Some(&i) = order.iter().find(|&&i| quota[i] < available[i]) else {
            break;
        };
        quota[i] += 1;
        assigned += 1;
    }
    quota
}

// ── Scoring ─────────────────────────────────────────────────────────────────

/// Sorted so float sums accumulate in a fixed order (deterministic scores).
fn tag_map(tags: &[TagWeight]) -> BTreeMap<&str, f64> {
    tags.iter()
        .map(|t| {
            let generic = if GENERIC_TAGS.contains(&t.tag.as_str()) {
                0.5
            } else {
                1.0
            };
            (t.tag.as_str(), t.weight * generic)
        })
        .collect()
}

/// Weighted cosine similarity between two tag vectors (0 when either is empty).
pub fn tag_similarity(a: &[TagWeight], b: &[TagWeight]) -> f64 {
    let (am, bm) = (tag_map(a), tag_map(b));
    let dot: f64 = am
        .iter()
        .filter_map(|(tag, wa)| bm.get(tag).map(|wb| wa * wb))
        .sum();
    let na: f64 = am.values().map(|w| w * w).sum::<f64>().sqrt();
    let nb: f64 = bm.values().map(|w| w * w).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        (dot / (na * nb)).clamp(0.0, 1.0)
    }
}

/// Best `(target mood, weight-scaled similarity, candidate bucket)` between a
/// candidate's mood tags and the moods in effect.
fn best_mood_match(candidate: &[TagWeight], moods: &[String]) -> Option<(f64, &'static str)> {
    let mut best: Option<(f64, &'static str)> = None;
    for tag in candidate {
        let Some(bucket) = mood_bucket_of(&tag.tag) else {
            continue;
        };
        for target in moods {
            let score = mood_similarity(target, bucket) * tag.weight.max(0.3);
            if best.is_none_or(|(current, _)| score > current) {
                best = Some((score, bucket));
            }
        }
    }
    best
}

fn audio_proximity(seed: &SeedAudio, candidate: &RadioTrack) -> Option<f64> {
    let mut parts: Vec<f64> = Vec::new();
    let bpm = candidate
        .track
        .bpm
        .or(candidate.track.bpm_detected)
        .filter(|b| *b > 0.0);
    if let (Some(seed_bpm), Some(bpm)) = (seed.bpm, bpm) {
        parts.push(1.0 - ((seed_bpm - bpm).abs() / 40.0).min(1.0));
    }
    if let (Some(seed_energy), Some(energy)) = (seed.energy, candidate.energy) {
        parts.push(1.0 - (seed_energy - energy).abs().min(1.0));
    }
    (!parts.is_empty()).then(|| parts.iter().sum::<f64>() / parts.len() as f64)
}

fn recency_penalty(age_days: Option<f64>) -> f64 {
    match age_days {
        None => 0.0,
        Some(d) if d < 1.0 => 1.0,
        Some(d) if d < 3.0 => 0.7,
        Some(d) if d < 7.0 => 0.4,
        Some(d) if d < RECENT_PLAY_WINDOW_DAYS as f64 => 0.2,
        Some(_) => 0.0,
    }
}

struct ScoreContext<'a> {
    seed_tags: &'a [TagWeight],
    seed_audio: &'a SeedAudio,
    moods: &'a [String],
    similar_artists: &'a HashMap<EntityId, f64>,
    recent_ages: &'a HashMap<EntityId, f64>,
}

/// 0..~1 usefulness of a candidate for this radio. Terms without data drop out
/// and the rest are renormalised, so a track with no tags or BPM is judged on
/// what is known rather than scored zero.
fn score_track(ctx: &ScoreContext<'_>, pool: Pool, candidate: &RadioTrack) -> f64 {
    let mut weighted = 0.0;
    let mut weight_sum = 0.0;
    let mut add = |weight: f64, term: f64| {
        weighted += weight * term.clamp(0.0, 1.0);
        weight_sum += weight;
    };
    let affinity = match pool {
        Pool::Seed => 1.0,
        Pool::Similar => candidate
            .artist_id
            .as_ref()
            .and_then(|id| ctx.similar_artists.get(id).copied())
            .unwrap_or(0.5),
        Pool::Mood => 0.0,
    };
    add(W_ARTIST, affinity);
    if candidate.tag_layer != TagLayer::None && !ctx.seed_tags.is_empty() {
        add(W_TAGS, tag_similarity(ctx.seed_tags, &candidate.tags));
    }
    if !ctx.moods.is_empty() {
        add(
            W_MOOD,
            best_mood_match(&candidate.tags, ctx.moods).map_or(0.0, |(score, _)| score),
        );
    }
    if let Some(closeness) = audio_proximity(ctx.seed_audio, candidate) {
        add(W_AUDIO, closeness);
    }
    let base = if weight_sum > 0.0 {
        weighted / weight_sum
    } else {
        0.0
    };
    let rating = candidate
        .rating
        .map_or(0.0, |r| ((r - 3.0) / 2.0).clamp(-1.0, 1.0));
    let recent = recency_penalty(ctx.recent_ages.get(&candidate.track.id).copied());
    base + W_RATING * rating - W_RECENT * recent
}

// ── Selection ───────────────────────────────────────────────────────────────

struct Picked {
    track: RadioTrack,
    pool: Pool,
    reason: RadioReason,
    score: f64,
}

fn artist_key(track: &RadioTrack) -> String {
    match &track.artist_id {
        Some(id) => id.to_string(),
        None => format!(
            "name:{}",
            track.track.artist.as_deref().unwrap_or("").to_lowercase()
        ),
    }
}

fn reason_for(
    inputs: &RadioInputs,
    moods: &[String],
    pool: Pool,
    candidate: &RadioTrack,
) -> RadioReason {
    match pool {
        Pool::Seed => RadioReason {
            kind: "seed",
            label: inputs.seed_artist_name.clone(),
        },
        Pool::Similar => RadioReason {
            kind: "similar",
            label: inputs.seed_artist_name.clone(),
        },
        Pool::Mood => {
            if let Some((score, bucket)) = best_mood_match(&candidate.tags, moods) {
                if score >= 0.3 {
                    return RadioReason {
                        kind: "mood",
                        label: bucket.to_owned(),
                    };
                }
            }
            let shared = inputs
                .seed_tags
                .iter()
                .find(|s| candidate.tags.iter().any(|c| c.tag == s.tag))
                .map(|s| s.tag.clone());
            RadioReason {
                kind: "style",
                label: shared.unwrap_or_else(|| "style match".to_owned()),
            }
        }
    }
}

/// Efraimidis–Spirakis weighted sampling without replacement: each candidate
/// gets key `u^(1/w)` and the largest keys win, so high scores are favoured
/// but never guaranteed (that is what makes the radio feel random).
fn draw(
    candidates: Vec<(RadioTrack, f64)>,
    exponent: f64,
    rng: &mut dyn FnMut() -> f64,
) -> Vec<(RadioTrack, f64)> {
    let mut keyed: Vec<(f64, RadioTrack, f64)> = candidates
        .into_iter()
        .map(|(track, score)| {
            let weight = score.max(MIN_SAMPLE_WEIGHT).powf(exponent).max(1e-9);
            let key = rng().clamp(1e-12, 1.0 - 1e-12).powf(1.0 / weight);
            (key, track, score)
        })
        .collect();
    keyed.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    keyed.into_iter().map(|(_, t, s)| (t, s)).collect()
}

/// Builds the queue: score → sample per source → sequence.
pub fn plan_radio(
    inputs: RadioInputs,
    options: &RadioOptions,
    rng: &mut dyn FnMut() -> f64,
) -> RadioPlan {
    let limit = options.limit.max(1);
    let moods = effective_moods(&options.moods, &inputs.seed_tags);
    let ctx = ScoreContext {
        seed_tags: &inputs.seed_tags,
        seed_audio: &inputs.seed_audio,
        moods: &moods,
        similar_artists: &inputs.similar_artists,
        recent_ages: &inputs.recent_ages,
    };
    // Sharper preference when the user wants familiar; flatter when adventurous.
    let exponent = 1.0 + (1.0 - options.variety.clamp(0.0, 1.0)) * 2.0;

    let mut seen: HashSet<EntityId> = HashSet::new();
    let mut scored: Vec<Vec<(RadioTrack, f64)>> = Vec::new();
    let mut all_candidates: HashMap<EntityId, bool> = HashMap::new();
    let pools_in = [&inputs.seed_pool, &inputs.similar_pool, &inputs.mood_pool];
    for (index, pool_tracks) in pools_in.iter().enumerate() {
        let pool = POOLS[index];
        let mut list: Vec<(RadioTrack, f64)> = Vec::new();
        for candidate in pool_tracks.iter() {
            // The seed artist only ever comes from its own pool.
            let is_seed_artist = candidate.artist_id.as_ref() == Some(&inputs.seed_artist_id);
            if pool != Pool::Seed && is_seed_artist {
                continue;
            }
            if !seen.insert(candidate.track.id.clone()) {
                continue;
            }
            all_candidates.insert(
                candidate.track.id.clone(),
                candidate.tag_layer == TagLayer::Track,
            );
            let score = score_track(&ctx, pool, candidate);
            list.push((candidate.clone(), score));
        }
        scored.push(list);
    }
    let coverage = Coverage {
        tagged: all_candidates.values().filter(|tagged| **tagged).count(),
        candidates: all_candidates.len(),
    };

    let available = [scored[0].len(), scored[1].len(), scored[2].len()];
    let shares = target_shares(options.focus, options.variety);
    let quota = allocate(limit, shares, available);

    let cap = limit.div_ceil(12).clamp(2, 8);
    let mut per_artist: HashMap<String, usize> = HashMap::new();
    let mut picks: Vec<Vec<Picked>> = vec![Vec::new(), Vec::new(), Vec::new()];
    let mut leftovers: Vec<Picked> = Vec::new();
    for index in 0..3 {
        let pool = POOLS[index];
        let drawn = draw(std::mem::take(&mut scored[index]), exponent, rng);
        for (track, score) in drawn {
            let key = artist_key(&track);
            let is_seed = pool == Pool::Seed;
            let full = picks[index].len() >= quota[index];
            let over_cap = !is_seed && per_artist.get(&key).copied().unwrap_or(0) >= cap;
            let reason = reason_for(&inputs, &moods, pool, &track);
            let picked = Picked {
                track,
                pool,
                reason,
                score,
            };
            if full || over_cap {
                leftovers.push(picked);
            } else {
                *per_artist.entry(key).or_default() += 1;
                picks[index].push(picked);
            }
        }
    }
    // Pools that ran short (after de-duplication and the artist cap) are
    // topped up from the best remaining candidates of any source.
    let mut total: usize = picks.iter().map(Vec::len).sum();
    if total < limit {
        leftovers.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for picked in leftovers {
            if total >= limit {
                break;
            }
            let key = artist_key(&picked.track);
            let is_seed = picked.pool == Pool::Seed;
            if !is_seed && per_artist.get(&key).copied().unwrap_or(0) >= cap {
                continue;
            }
            *per_artist.entry(key).or_default() += 1;
            picks[picked.pool.index()].push(picked);
            total += 1;
        }
    }

    let ordered = sequence(picks, shares, &inputs.seed_artist_id);
    let n = ordered.len().max(1) as f64;
    let count = |pool: Pool| ordered.iter().filter(|p| p.pool == pool).count() as f64 / n;
    let mix = MixShares {
        seed: count(Pool::Seed),
        similar: count(Pool::Similar),
        mood: count(Pool::Mood),
    };
    let degraded = degraded_message(&ordered, available, &inputs);
    RadioPlan {
        tracks: ordered
            .into_iter()
            .map(|p| PlannedTrack {
                track: p.track,
                reason: p.reason,
            })
            .collect(),
        moods,
        mix,
        coverage,
        degraded,
    }
}

fn degraded_message(
    ordered: &[Picked],
    available: [usize; 3],
    inputs: &RadioInputs,
) -> Option<String> {
    if ordered.is_empty() {
        return Some("No tracks found for this artist.".to_owned());
    }
    if available[1] == 0 && available[2] == 0 {
        return Some(if inputs.seed_tags.is_empty() {
            "Not enough tag data yet — playing this artist's tracks. Better matches appear as metadata is collected."
                .to_owned()
        } else {
            "No similar artists or matching tracks found yet — playing this artist's tracks."
                .to_owned()
        });
    }
    if available[1] == 0 {
        return Some("No similar-artist data yet — filled with mood and style matches.".to_owned());
    }
    None
}

/// Orders picks so sources interleave in proportion to their shares, the same
/// artist never repeats within three tracks (the seed artist just never
/// back-to-back), albums don't repeat back-to-back, and energy doesn't jump
/// between neighbours when it is known.
fn sequence(
    mut picks: Vec<Vec<Picked>>,
    shares: [f64; 3],
    seed_artist_id: &EntityId,
) -> Vec<Picked> {
    let seed_key = seed_artist_id.to_string();
    let total: usize = picks.iter().map(Vec::len).sum();
    let mut out: Vec<Picked> = Vec::with_capacity(total);
    let mut placed = [0usize; 3];

    // Open with the seed artist's best-scoring pick.
    if let Some(best) = picks[0]
        .iter()
        .enumerate()
        .max_by(|a, b| {
            a.1.score
                .partial_cmp(&b.1.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
    {
        placed[0] += 1;
        out.push(picks[0].remove(best));
    }

    while out.len() < total {
        let position = out.len() as f64 + 1.0;
        let mut order: Vec<usize> = (0..3).filter(|i| !picks[*i].is_empty()).collect();
        order.sort_by(|a, b| {
            let da = shares[*a] * position - placed[*a] as f64;
            let db = shares[*b] * position - placed[*b] as f64;
            db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
        });
        let chosen = choose_next(&picks, &order, &out, &seed_key, Strictness::Full)
            .or_else(|| choose_next(&picks, &order, &out, &seed_key, Strictness::NoAlbumRule))
            .or_else(|| choose_next(&picks, &order, &out, &seed_key, Strictness::NotSameAsLast))
            .or_else(|| order.first().map(|&pool| (pool, 0)));
        let Some((pool, index)) = chosen else { break };
        placed[pool] += 1;
        out.push(picks[pool].remove(index));
    }
    out
}

#[derive(Clone, Copy)]
enum Strictness {
    Full,
    NoAlbumRule,
    NotSameAsLast,
}

fn choose_next(
    picks: &[Vec<Picked>],
    order: &[usize],
    out: &[Picked],
    seed_key: &str,
    strictness: Strictness,
) -> Option<(usize, usize)> {
    let last_album = out.last().and_then(|p| p.track.track.album_id.clone());
    let prev_energy = out.last().and_then(|p| p.track.energy);
    for &pool in order {
        let mut eligible: Vec<usize> = Vec::new();
        for (index, candidate) in picks[pool].iter().enumerate().take(6) {
            let key = artist_key(&candidate.track);
            let gap = match strictness {
                Strictness::NotSameAsLast => 1,
                _ if key == seed_key => 1,
                _ => 3,
            };
            let recent_clash = out
                .iter()
                .rev()
                .take(gap)
                .any(|placed| artist_key(&placed.track) == key);
            let album_clash = matches!(strictness, Strictness::Full)
                && last_album.is_some()
                && candidate.track.track.album_id == last_album;
            if !recent_clash && !album_clash {
                eligible.push(index);
            }
            if eligible.len() >= 3 {
                break;
            }
        }
        if eligible.is_empty() {
            continue;
        }
        let best = match prev_energy {
            Some(previous) => eligible
                .iter()
                .copied()
                .filter(|i| picks[pool][*i].track.energy.is_some())
                .min_by(|a, b| {
                    let da = (picks[pool][*a].track.energy.unwrap_or(previous) - previous).abs();
                    let db = (picks[pool][*b].track.energy.unwrap_or(previous) - previous).abs();
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap_or(eligible[0]),
            None => eligible[0],
        };
        return Some((pool, best));
    }
    None
}

// ── Data loading ────────────────────────────────────────────────────────────

/// Similar artists kept from the provider graphs.
const SIMILAR_ARTIST_LIMIT: usize = 40;
const SEED_POOL_LIMIT: i64 = 400;
const SIMILAR_TRACKS_PER_ARTIST: i64 = 8;
const MOOD_POOL_LIMIT: i64 = 500;

/// Fetches the three candidate pools (seed / similar / mood) and everything
/// needed to score them. Reads only the local database and provider caches.
pub fn load_inputs(
    conn: &Connection,
    user_id: &str,
    artist_id: &EntityId,
    artist_name: String,
    related: &RelatedSet,
    options: &RadioOptions,
) -> rusqlite::Result<RadioInputs> {
    let seed_tags = seed_tag_vector(conn, artist_id)?;
    let seed_audio = seed_audio_profile(conn, artist_id)?;
    let moods = effective_moods(&options.moods, &seed_tags);
    let similar = resolve_local_similar_artists_with_listenbrainz(
        conn,
        user_id,
        artist_id,
        &related.lastfm,
        &related.deezer,
        &related.listenbrainz,
        SIMILAR_ARTIST_LIMIT,
    )?;
    let similar_ids: Vec<EntityId> = similar.iter().map(|s| s.id.clone()).collect();
    let similar_artists: HashMap<EntityId, f64> =
        similar.iter().map(|s| (s.id.clone(), s.score)).collect();

    let seed_ids = radio_ids_for_artists(conn, std::slice::from_ref(artist_id), SEED_POOL_LIMIT)?;
    let similar_track_ids = radio_ids_for_artists(conn, &similar_ids, SIMILAR_TRACKS_PER_ARTIST)?;
    let tags = pool_tags(&seed_tags, &moods, options.focus);
    let mood_ids = radio_ids_matching_tags(conn, &tags, artist_id, MOOD_POOL_LIMIT)?;

    let mut unique: Vec<EntityId> = Vec::new();
    let mut seen: HashSet<EntityId> = HashSet::new();
    for id in seed_ids.iter().chain(&similar_track_ids).chain(&mood_ids) {
        if seen.insert(id.clone()) {
            unique.push(id.clone());
        }
    }
    let loaded: HashMap<EntityId, RadioTrack> = load_radio_tracks(conn, user_id, &unique)?
        .into_iter()
        .map(|t| (t.track.id.clone(), t))
        .collect();
    let pick = |ids: &[EntityId]| -> Vec<RadioTrack> {
        ids.iter()
            .filter_map(|id| loaded.get(id).cloned())
            .collect()
    };
    Ok(RadioInputs {
        seed_artist_id: artist_id.clone(),
        seed_artist_name: artist_name,
        seed_tags,
        seed_audio,
        seed_pool: pick(&seed_ids),
        similar_pool: pick(&similar_track_ids),
        similar_artists,
        mood_pool: pick(&mood_ids),
        recent_ages: recent_play_ages(conn, user_id, RECENT_PLAY_WINDOW_DAYS)?,
    })
}

/// One mood the popover can offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MoodOption {
    pub bucket: &'static str,
    /// Enough tagged tracks exist to build a mood radio around it.
    pub available: bool,
    /// Auto-picked from this artist.
    pub auto: bool,
}

/// Tracks that must carry a mood for it to be offered.
const MOOD_AVAILABLE_MIN_TRACKS: i64 = 20;

/// Everything the options popover shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioOptionsSnapshot {
    pub tags: Vec<String>,
    pub auto_moods: Vec<String>,
    pub moods: Vec<MoodOption>,
    /// Library-wide background tagging progress (`checked` of `total` tracks).
    pub library_tag_progress: Coverage,
}

pub fn radio_options(
    conn: &Connection,
    artist_id: &EntityId,
) -> rusqlite::Result<RadioOptionsSnapshot> {
    let seed_tags = seed_tag_vector(conn, artist_id)?;
    let auto = auto_moods(&seed_tags);
    let mut moods = Vec::with_capacity(MOOD_BUCKETS.len());
    for bucket in MOOD_BUCKETS {
        let count =
            count_tracks_with_any_tag(conn, &tags_for_bucket(bucket), MOOD_AVAILABLE_MIN_TRACKS)?;
        moods.push(MoodOption {
            bucket,
            available: count >= MOOD_AVAILABLE_MIN_TRACKS,
            auto: auto.iter().any(|a| a == bucket),
        });
    }
    let (checked, total) = track_tag_progress(conn, SOURCE_LASTFM)?;
    Ok(RadioOptionsSnapshot {
        tags: seed_tags.iter().take(12).map(|t| t.tag.clone()).collect(),
        auto_moods: auto,
        moods,
        library_tag_progress: Coverage {
            tagged: checked as usize,
            candidates: total as usize,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use boogiebox_db::music::TrackRow;

    fn tw(tag: &str, weight: f64) -> TagWeight {
        TagWeight {
            tag: tag.to_owned(),
            weight,
        }
    }

    fn track(id: &str, artist: &str, tags: &[(&str, f64)]) -> RadioTrack {
        RadioTrack {
            track: TrackRow {
                id: EntityId::Str(id.to_owned()),
                file_name: None,
                file_size: None,
                format: None,
                duration: Some(200.0),
                bitrate: None,
                sample_rate: None,
                channels: None,
                title: Some(id.to_owned()),
                track_number: None,
                disc_number: None,
                year: None,
                genre: None,
                composer: None,
                comment: None,
                bpm: None,
                bpm_detected: None,
                bpm_source: None,
                bpm_confidence: None,
                scanned_at: None,
                last_played_at: None,
                play_count: None,
                album_id: Some(EntityId::Str(format!("album-{id}"))),
                artist: Some(artist.to_owned()),
                album: None,
                library_name: None,
                rating: None,
                has_deep_analysis: false,
                file_path: None,
            },
            artist_id: Some(EntityId::Str(artist.to_owned())),
            tags: tags.iter().map(|(t, w)| tw(t, *w)).collect(),
            tag_layer: if tags.is_empty() {
                TagLayer::None
            } else {
                TagLayer::Track
            },
            rating: None,
            energy: None,
        }
    }

    /// Deterministic uniform (0,1) source (SplitMix64).
    fn rng(seed: u64) -> impl FnMut() -> f64 {
        let mut state = seed;
        move || {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            ((z >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        }
    }

    fn options(limit: usize) -> RadioOptions {
        RadioOptions {
            limit,
            focus: Focus::Similar,
            moods: vec![],
            variety: DEFAULT_VARIETY,
        }
    }

    /// 40 seed tracks, 20 similar artists × 5 tracks, 15 mood artists × 5 tracks.
    fn big_inputs() -> RadioInputs {
        let seed_tags = vec![
            tw("trip-hop", 1.0),
            tw("melancholy", 0.8),
            tw("electronic", 0.6),
        ];
        let mut similar_pool = Vec::new();
        let mut similar_artists = HashMap::new();
        for a in 0..20 {
            let name = format!("sim{a}");
            similar_artists.insert(EntityId::Str(name.clone()), 1.0 - a as f64 * 0.03);
            for t in 0..5 {
                similar_pool.push(track(&format!("{name}-{t}"), &name, &[("trip-hop", 0.9)]));
            }
        }
        let mut mood_pool = Vec::new();
        for a in 0..15 {
            let name = format!("mood{a}");
            for t in 0..5 {
                mood_pool.push(track(&format!("{name}-{t}"), &name, &[("melancholy", 0.8)]));
            }
        }
        RadioInputs {
            seed_artist_id: EntityId::Str("seed".into()),
            seed_artist_name: "Seed".into(),
            seed_tags,
            seed_audio: SeedAudio::default(),
            seed_pool: (0..40)
                .map(|i| track(&format!("seed-{i}"), "seed", &[("trip-hop", 1.0)]))
                .collect(),
            similar_pool,
            similar_artists,
            mood_pool,
            recent_ages: HashMap::new(),
        }
    }

    fn ids(plan: &RadioPlan) -> Vec<String> {
        plan.tracks
            .iter()
            .map(|p| p.track.track.id.to_string())
            .collect()
    }

    #[test]
    fn shares_default_to_30_45_25_and_respond_to_focus_and_variety() {
        let s = target_shares(Focus::Similar, DEFAULT_VARIETY);
        assert!(
            (s[0] - 0.30).abs() < 1e-9 && (s[1] - 0.45).abs() < 1e-9 && (s[2] - 0.25).abs() < 1e-9
        );
        let m = target_shares(Focus::Mood, DEFAULT_VARIETY);
        assert!((m[0] - 0.10).abs() < 1e-9 && (m[2] - 0.65).abs() < 1e-9);
        let adventurous = target_shares(Focus::Similar, 1.0);
        let familiar = target_shares(Focus::Similar, 0.0);
        assert!(adventurous[0] < s[0] && familiar[0] > s[0]);
        for shares in [s, m, adventurous, familiar, target_shares(Focus::Mood, 1.0)] {
            assert!((shares.iter().sum::<f64>() - 1.0).abs() < 1e-9);
            assert!(shares[0] >= 0.05 - 1e-9 && shares[0] <= 0.45 + 1e-9);
        }
        // Out-of-range sliders are clamped rather than producing negative shares.
        assert!(target_shares(Focus::Similar, 9.0)[0] > 0.0);
        assert_eq!(Focus::parse("similar"), Some(Focus::Similar));
        assert_eq!(Focus::parse("mood"), Some(Focus::Mood));
        assert_eq!(Focus::parse("x"), None);
    }

    #[test]
    fn allocate_redistributes_from_empty_and_short_pools() {
        assert_eq!(allocate(100, [0.3, 0.45, 0.25], [50, 50, 50]), [30, 45, 25]);
        assert_eq!(
            allocate(100, [0.3, 0.45, 0.25], [50, 0, 50])
                .iter()
                .sum::<usize>(),
            100
        );
        assert_eq!(allocate(100, [0.3, 0.45, 0.25], [50, 0, 50])[1], 0);
        // A tiny seed pool hands its unused quota to the others.
        let a = allocate(100, [0.3, 0.45, 0.25], [5, 80, 80]);
        assert_eq!(a[0], 5);
        assert_eq!(a.iter().sum::<usize>(), 100);
        // Not enough tracks anywhere: take everything, never over-allocate.
        assert_eq!(allocate(100, [0.3, 0.45, 0.25], [2, 3, 4]), [2, 3, 4]);
        assert_eq!(allocate(10, [0.3, 0.45, 0.25], [0, 0, 0]), [0, 0, 0]);
    }

    #[test]
    fn auto_moods_pick_the_dominant_buckets_and_requested_moods_override() {
        let tags = vec![
            tw("trip-hop", 1.0),
            tw("melancholy", 0.8),
            tw("sad", 0.4),
            tw("atmospheric", 0.5),
            tw("chill", 0.1),
        ];
        assert_eq!(auto_moods(&tags), vec!["melancholic", "dreamy"]);
        assert!(auto_moods(&[tw("rock", 1.0)]).is_empty());
        assert_eq!(
            effective_moods(
                &["Dark".to_owned(), "bogus".to_owned(), "dark".to_owned()],
                &tags
            ),
            vec!["dark"]
        );
        assert_eq!(
            effective_moods(&["bogus".to_owned()], &tags),
            auto_moods(&tags)
        );
    }

    #[test]
    fn pool_tags_combine_seed_genres_and_mood_words_by_focus() {
        let seed = vec![
            tw("trip-hop", 1.0),
            tw("melancholy", 0.9),
            tw("downtempo", 0.8),
            tw("ambient", 0.7),
            tw("british", 0.6),
        ];
        let moods = vec!["chill".to_owned()];
        let similar = pool_tags(&seed, &moods, Focus::Similar);
        assert!(
            similar.contains(&"trip-hop".to_owned()) && similar.contains(&"british".to_owned())
        );
        assert!(!similar.contains(&"melancholy".to_owned())); // moods come only from the chosen buckets
        assert!(similar.contains(&"relaxing".to_owned()));
        let mood = pool_tags(&seed, &moods, Focus::Mood);
        assert!(mood.contains(&"downtempo".to_owned()));
        assert!(!mood.contains(&"british".to_owned()));
    }

    #[test]
    fn tag_similarity_is_weighted_cosine_with_generic_tags_discounted() {
        let a = vec![tw("trip-hop", 1.0), tw("rock", 1.0)];
        assert!((tag_similarity(&a, &a) - 1.0).abs() < 1e-9);
        assert_eq!(tag_similarity(&a, &[]), 0.0);
        assert_eq!(tag_similarity(&a, &[tw("jazz", 1.0)]), 0.0);
        // Sharing only a broad tag counts for less than sharing a specific one.
        let generic_only = tag_similarity(&a, &[tw("rock", 1.0), tw("x-tag", 1.0)]);
        let specific = tag_similarity(&a, &[tw("trip-hop", 1.0), tw("x-tag", 1.0)]);
        assert!(specific > generic_only);
    }

    #[test]
    fn plan_matches_default_mix_and_is_deterministic_per_seed() {
        let plan = plan_radio(big_inputs(), &options(100), &mut rng(7));
        assert_eq!(plan.tracks.len(), 100);
        assert!((plan.mix.seed - 0.30).abs() <= 0.03, "{:?}", plan.mix);
        assert!((plan.mix.similar - 0.45).abs() <= 0.03, "{:?}", plan.mix);
        assert!((plan.mix.mood - 0.25).abs() <= 0.03, "{:?}", plan.mix);
        assert_eq!(
            plan.tracks[0].reason.kind, "seed",
            "opens with the seed artist"
        );
        assert_eq!(plan.degraded, None);
        assert_eq!(plan.moods, vec!["melancholic"]);

        let again = plan_radio(big_inputs(), &options(100), &mut rng(7));
        assert_eq!(ids(&plan), ids(&again));
        let other = plan_radio(big_inputs(), &options(100), &mut rng(8));
        assert_ne!(ids(&plan), ids(&other));
        let unique: HashSet<String> = ids(&plan).into_iter().collect();
        assert_eq!(unique.len(), 100, "no track twice");
    }

    #[test]
    fn sequencing_spaces_artists_and_never_puts_the_seed_back_to_back() {
        for seed in 1..6 {
            let plan = plan_radio(big_inputs(), &options(100), &mut rng(seed));
            let artists: Vec<String> = plan
                .tracks
                .iter()
                .map(|p| p.track.track.artist.clone().unwrap())
                .collect();
            for (i, artist) in artists.iter().enumerate() {
                if artist == "Seed" || artist == "seed" {
                    if i > 0 {
                        assert_ne!(&artists[i - 1], artist, "seed back-to-back at {i}");
                    }
                    continue;
                }
                for back in 1..=3 {
                    if i >= back {
                        assert_ne!(
                            &artists[i - back],
                            artist,
                            "{artist} repeats within 3 at {i}"
                        );
                    }
                }
            }
            // Per-artist cap (ceil(100/12) = 9 → clamped to 8) for non-seed artists.
            let mut counts: HashMap<&String, usize> = HashMap::new();
            for a in &artists {
                *counts.entry(a).or_default() += 1;
            }
            assert!(counts
                .iter()
                .filter(|(a, _)| a.as_str() != "seed")
                .all(|(_, c)| *c <= 8));
        }
    }

    #[test]
    fn empty_pools_hand_their_share_over_and_never_fail() {
        // No similar artists: mood picks up the slack, with an explanation.
        let mut inputs = big_inputs();
        inputs.similar_pool.clear();
        let plan = plan_radio(inputs, &options(60), &mut rng(3));
        assert_eq!(plan.tracks.len(), 60);
        assert_eq!(plan.mix.similar, 0.0);
        assert!(plan
            .degraded
            .as_deref()
            .unwrap()
            .contains("No similar-artist data"));

        // No similar and no mood candidates and no tags: the seed's own tracks.
        let mut inputs = big_inputs();
        inputs.similar_pool.clear();
        inputs.mood_pool.clear();
        inputs.seed_tags.clear();
        let plan = plan_radio(inputs, &options(30), &mut rng(3));
        assert_eq!(plan.tracks.len(), 30);
        assert!(plan.tracks.iter().all(|p| p.reason.kind == "seed"));
        assert!(plan
            .degraded
            .as_deref()
            .unwrap()
            .contains("Not enough tag data"));

        // With tags but nothing to match them against.
        let mut inputs = big_inputs();
        inputs.similar_pool.clear();
        inputs.mood_pool.clear();
        let plan = plan_radio(inputs, &options(10), &mut rng(3));
        assert!(plan
            .degraded
            .as_deref()
            .unwrap()
            .contains("No similar artists or matching"));

        // Nothing at all.
        let mut inputs = big_inputs();
        inputs.seed_pool.clear();
        inputs.similar_pool.clear();
        inputs.mood_pool.clear();
        let plan = plan_radio(inputs, &options(10), &mut rng(3));
        assert!(plan.tracks.is_empty());
        assert!(plan.degraded.is_some());
    }

    #[test]
    fn a_short_seed_pool_is_topped_up_and_the_queue_still_fills() {
        let mut inputs = big_inputs();
        inputs.seed_pool.truncate(3);
        let plan = plan_radio(inputs, &options(100), &mut rng(11));
        assert_eq!(plan.tracks.len(), 100);
        assert_eq!(
            plan.tracks
                .iter()
                .filter(|p| p.reason.kind == "seed")
                .count(),
            3
        );
    }

    #[test]
    fn the_seed_artist_never_leaks_in_through_other_pools() {
        let mut inputs = big_inputs();
        inputs
            .similar_pool
            .push(track("leak-1", "seed", &[("trip-hop", 1.0)]));
        inputs
            .mood_pool
            .push(track("leak-2", "seed", &[("melancholy", 1.0)]));
        let plan = plan_radio(inputs, &options(100), &mut rng(5));
        for planned in &plan.tracks {
            if planned.track.track.id.to_string().starts_with("leak-") {
                panic!("seed-artist track leaked from another pool");
            }
        }
    }

    #[test]
    fn a_track_in_two_pools_is_only_drawn_once() {
        let mut inputs = big_inputs();
        let dup = inputs.similar_pool[0].clone();
        inputs.mood_pool.push(dup);
        let plan = plan_radio(inputs, &options(100), &mut rng(5));
        let unique: HashSet<String> = ids(&plan).into_iter().collect();
        assert_eq!(unique.len(), plan.tracks.len());
    }

    #[test]
    fn recently_played_tracks_are_drawn_less_often() {
        let mut inputs = big_inputs();
        let played: HashMap<EntityId, f64> = inputs
            .mood_pool
            .iter()
            .filter(|t| {
                t.track
                    .artist
                    .as_deref()
                    .unwrap()
                    .ends_with(|c: char| "02468".contains(c))
            })
            .map(|t| (t.track.id.clone(), 0.2))
            .collect();
        inputs.recent_ages = played.clone();
        let (mut with_penalty, mut without) = (0usize, 0usize);
        for seed in 1..=12 {
            let plan = plan_radio(inputs_clone(&inputs), &options(60), &mut rng(seed));
            with_penalty += plan
                .tracks
                .iter()
                .filter(|p| played.contains_key(&p.track.track.id))
                .count();
            let mut clean = inputs_clone(&inputs);
            clean.recent_ages.clear();
            let plan = plan_radio(clean, &options(60), &mut rng(seed));
            without += plan
                .tracks
                .iter()
                .filter(|p| played.contains_key(&p.track.track.id))
                .count();
        }
        assert!(
            with_penalty < without,
            "penalised {with_penalty} vs {without}"
        );
    }

    fn inputs_clone(i: &RadioInputs) -> RadioInputs {
        RadioInputs {
            seed_artist_id: i.seed_artist_id.clone(),
            seed_artist_name: i.seed_artist_name.clone(),
            seed_tags: i.seed_tags.clone(),
            seed_audio: i.seed_audio,
            seed_pool: i.seed_pool.clone(),
            similar_pool: i.similar_pool.clone(),
            similar_artists: i.similar_artists.clone(),
            mood_pool: i.mood_pool.clone(),
            recent_ages: i.recent_ages.clone(),
        }
    }

    fn ctx_for<'a>(
        seed_tags: &'a [TagWeight],
        audio: &'a SeedAudio,
        moods: &'a [String],
        sim: &'a HashMap<EntityId, f64>,
        recent: &'a HashMap<EntityId, f64>,
    ) -> ScoreContext<'a> {
        ScoreContext {
            seed_tags,
            seed_audio: audio,
            moods,
            similar_artists: sim,
            recent_ages: recent,
        }
    }

    #[test]
    fn scoring_uses_available_signals_and_drops_missing_ones() {
        let seed_tags = vec![tw("trip-hop", 1.0)];
        let (sim, recent) = (HashMap::new(), HashMap::new());
        let no_audio = SeedAudio::default();
        let moods: Vec<String> = vec![];
        let ctx = ctx_for(&seed_tags, &no_audio, &moods, &sim, &recent);

        let tagged = track("a", "x", &[("trip-hop", 1.0)]);
        let untagged = track("b", "x", &[]);
        assert!(score_track(&ctx, Pool::Mood, &tagged) > score_track(&ctx, Pool::Mood, &untagged));

        // Rating boost and penalty.
        let mut loved = tagged.clone();
        loved.rating = Some(5.0);
        let mut disliked = tagged.clone();
        disliked.rating = Some(1.0);
        assert!(score_track(&ctx, Pool::Mood, &loved) > score_track(&ctx, Pool::Mood, &tagged));
        assert!(score_track(&ctx, Pool::Mood, &disliked) < score_track(&ctx, Pool::Mood, &tagged));

        // BPM closeness only matters when both sides have a BPM.
        let seed_audio = SeedAudio {
            bpm: Some(100.0),
            energy: None,
        };
        let ctx_audio = ctx_for(&seed_tags, &seed_audio, &moods, &sim, &recent);
        let mut near = tagged.clone();
        near.track.bpm = Some(102.0);
        let mut far = tagged.clone();
        far.track.bpm = Some(160.0);
        assert!(
            score_track(&ctx_audio, Pool::Mood, &near) > score_track(&ctx_audio, Pool::Mood, &far)
        );
        assert_eq!(
            score_track(&ctx, Pool::Mood, &near),
            score_track(&ctx, Pool::Mood, &far),
            "no seed BPM → the BPM term is omitted"
        );
        // Energy contributes too when both are known.
        let energetic_seed = SeedAudio {
            bpm: None,
            energy: Some(0.9),
        };
        let ctx_energy = ctx_for(&seed_tags, &energetic_seed, &moods, &sim, &recent);
        let mut hot = tagged.clone();
        hot.energy = Some(0.85);
        let mut calm = tagged.clone();
        calm.energy = Some(0.1);
        assert!(
            score_track(&ctx_energy, Pool::Mood, &hot)
                > score_track(&ctx_energy, Pool::Mood, &calm)
        );
    }

    #[test]
    fn mood_match_and_similar_affinity_raise_the_score() {
        let seed_tags = vec![tw("trip-hop", 1.0)];
        let mut sim = HashMap::new();
        sim.insert(EntityId::Str("close".into()), 0.95);
        sim.insert(EntityId::Str("far".into()), 0.1);
        let recent = HashMap::new();
        let audio = SeedAudio::default();
        let moods = vec!["melancholic".to_owned()];
        let ctx = ctx_for(&seed_tags, &audio, &moods, &sim, &recent);
        let close = track("t1", "close", &[("trip-hop", 1.0)]);
        let far = track("t2", "far", &[("trip-hop", 1.0)]);
        assert!(score_track(&ctx, Pool::Similar, &close) > score_track(&ctx, Pool::Similar, &far));

        let sad = track("t3", "z", &[("sad", 0.9)]);
        let happy = track("t4", "z", &[("happy", 0.9)]);
        let plain = track("t5", "z", &[("jazz", 0.9)]);
        let s = |t: &RadioTrack| score_track(&ctx, Pool::Mood, t);
        assert!(s(&sad) > s(&happy) && s(&happy) >= s(&plain));

        // Recency: played today is heavily penalised, an old play is not.
        let mut today = HashMap::new();
        today.insert(sad.track.id.clone(), 0.2);
        let mut old = HashMap::new();
        old.insert(sad.track.id.clone(), 30.0);
        let (ctx_today, ctx_old) = (
            ctx_for(&seed_tags, &audio, &moods, &sim, &today),
            ctx_for(&seed_tags, &audio, &moods, &sim, &old),
        );
        assert!(
            score_track(&ctx_today, Pool::Mood, &sad) < score_track(&ctx_old, Pool::Mood, &sad)
        );
        for (age, penalty) in [(0.5, 1.0), (2.0, 0.7), (5.0, 0.4), (10.0, 0.2), (20.0, 0.0)] {
            assert_eq!(recency_penalty(Some(age)), penalty);
        }
        assert_eq!(recency_penalty(None), 0.0);
    }

    #[test]
    fn reasons_name_the_source_mood_or_shared_style() {
        let inputs = big_inputs();
        let moods = vec!["melancholic".to_owned()];
        let mood_hit = track("m1", "z", &[("sad", 0.9)]);
        let reason = reason_for(&inputs, &moods, Pool::Mood, &mood_hit);
        assert_eq!(
            (reason.kind, reason.label.as_str()),
            ("mood", "melancholic")
        );
        let style_hit = track("m2", "z", &[("trip-hop", 0.9)]);
        let reason = reason_for(&inputs, &moods, Pool::Mood, &style_hit);
        assert_eq!((reason.kind, reason.label.as_str()), ("style", "trip-hop"));
        let unknown = track("m3", "z", &[("jazz", 0.9)]);
        assert_eq!(
            reason_for(&inputs, &moods, Pool::Mood, &unknown).label,
            "style match"
        );
        let similar = reason_for(&inputs, &moods, Pool::Similar, &unknown);
        assert_eq!((similar.kind, similar.label.as_str()), ("similar", "Seed"));
        assert_eq!(
            reason_for(&inputs, &moods, Pool::Seed, &unknown).kind,
            "seed"
        );
    }

    #[test]
    fn coverage_counts_only_track_level_tags() {
        let mut inputs = big_inputs();
        for t in inputs.mood_pool.iter_mut().take(10) {
            t.tag_layer = TagLayer::Artist;
        }
        let plan = plan_radio(inputs, &options(100), &mut rng(2));
        // 40 seed + 100 similar + 75 mood candidates, 10 of them only artist-tagged.
        assert_eq!(plan.coverage.candidates, 215);
        assert_eq!(plan.coverage.tagged, 205);
    }

    #[test]
    fn energy_smoothing_prefers_the_closest_energy_neighbour() {
        let mut inputs = big_inputs();
        inputs.seed_pool.clear();
        inputs.similar_pool.clear();
        inputs.mood_pool = (0..12)
            .map(|i| {
                let mut t = track(
                    &format!("e{i}"),
                    &format!("artist{i}"),
                    &[("melancholy", 0.8)],
                );
                t.energy = Some(if i % 2 == 0 { 0.9 } else { 0.1 });
                t
            })
            .collect();
        let plan = plan_radio(inputs, &options(12), &mut rng(4));
        let energies: Vec<f64> = plan.tracks.iter().filter_map(|p| p.track.energy).collect();
        assert_eq!(energies.len(), 12);
        let jumps = energies
            .windows(2)
            .filter(|w| (w[0] - w[1]).abs() > 0.5)
            .count();
        assert!(
            jumps < 10,
            "should avoid always swinging high/low, got {jumps} big jumps"
        );
    }

    #[test]
    fn mood_focus_leans_on_mood_matches() {
        let mut opts = options(100);
        opts.focus = Focus::Mood;
        let plan = plan_radio(big_inputs(), &opts, &mut rng(9));
        assert!(plan.mix.mood > plan.mix.similar);
        assert!(plan.mix.seed < 0.2);
        opts.moods = vec!["dreamy".to_owned()];
        let plan = plan_radio(big_inputs(), &opts, &mut rng(9));
        assert_eq!(plan.moods, vec!["dreamy"]);
    }

    fn fixture_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        boogiebox_db::initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO users(id, username) VALUES('u1', 'user');
             INSERT INTO libraries(id, path, name) VALUES('lib', '/m', 'Music');
             INSERT INTO artists(id, name, lastfm_mbid) VALUES
               ('seed', 'Seed', 'seed-mbid'), ('sim', 'Similar', 'sim-mbid'), ('other', 'Other', NULL);
             INSERT INTO albums(id, title, album_artist, artist_id) VALUES
               ('a-seed', 'S', 'Seed', 'seed'), ('a-sim', 'X', 'Similar', 'sim'), ('a-other', 'O', 'Other', 'other');
             INSERT INTO artist_styles(artist_id, style, kind, bucket, weight) VALUES
               ('seed', 'trip-hop', 'genre', NULL, 1.0), ('seed', 'melancholy', 'mood', 'melancholic', 0.8),
               ('other', 'trip-hop', 'genre', NULL, 0.9);",
        )
        .unwrap();
        for (id, artist, album) in [
            ("s1", "seed", "a-seed"),
            ("s2", "seed", "a-seed"),
            ("s3", "seed", "a-seed"),
            ("x1", "sim", "a-sim"),
            ("x2", "sim", "a-sim"),
            ("o1", "other", "a-other"),
            ("o2", "other", "a-other"),
        ] {
            conn.execute(
                "INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path)
                 VALUES(?1, 'lib', ?2, ?3, ?1, '/m/' || ?1 || '.mp3')",
                rusqlite::params![id, artist, album],
            )
            .unwrap();
        }
        conn
    }

    fn related_similar() -> RelatedSet {
        RelatedSet {
            lastfm: vec![boogiebox_server_related("sim-mbid", "Similar", 0.9)],
            ..RelatedSet::default()
        }
    }

    fn boogiebox_server_related(
        id: &str,
        name: &str,
        score: f64,
    ) -> crate::providers::RelatedArtistCandidate {
        crate::providers::RelatedArtistCandidate {
            external_id: Some(id.to_owned()),
            name: name.to_owned(),
            url: None,
            image_url: None,
            match_score: Some(score),
            rank: 1,
        }
    }

    #[test]
    fn load_inputs_builds_the_three_pools_and_a_full_plan_from_the_database() {
        let conn = fixture_db();
        let seed = EntityId::Str("seed".into());
        let inputs = load_inputs(
            &conn,
            "u1",
            &seed,
            "Seed".into(),
            &related_similar(),
            &options(100),
        )
        .unwrap();
        let names = |pool: &[RadioTrack]| {
            let mut v: Vec<String> = pool.iter().map(|t| t.track.id.to_string()).collect();
            v.sort();
            v
        };
        assert_eq!(names(&inputs.seed_pool), vec!["s1", "s2", "s3"]);
        assert_eq!(names(&inputs.similar_pool), vec!["x1", "x2"]);
        // "other" shares the trip-hop tag; the seed artist itself is excluded from the mood pool.
        assert_eq!(names(&inputs.mood_pool), vec!["o1", "o2"]);
        assert_eq!(inputs.seed_tags[0].tag, "trip-hop");
        assert!(inputs
            .similar_artists
            .contains_key(&EntityId::Str("sim".into())));

        let plan = plan_radio(inputs, &options(100), &mut rng(1));
        assert_eq!(plan.tracks.len(), 7);
        let kinds: HashSet<&str> = plan.tracks.iter().map(|p| p.reason.kind).collect();
        assert!(kinds.contains("seed") && kinds.contains("similar"));
        assert_eq!(plan.moods, vec!["melancholic"]);
    }

    #[test]
    fn load_inputs_without_any_similarity_data_still_returns_the_seed_artist() {
        let conn = fixture_db();
        conn.execute("DELETE FROM artist_styles", []).unwrap();
        let seed = EntityId::Str("seed".into());
        let inputs = load_inputs(
            &conn,
            "u1",
            &seed,
            "Seed".into(),
            &RelatedSet::default(),
            &options(50),
        )
        .unwrap();
        assert!(inputs.similar_pool.is_empty() && inputs.mood_pool.is_empty());
        let plan = plan_radio(inputs, &options(50), &mut rng(1));
        assert_eq!(plan.tracks.len(), 3);
        assert!(plan.degraded.is_some());
    }

    #[test]
    fn radio_options_lists_all_moods_with_availability_and_auto_picks() {
        let conn = fixture_db();
        let opts = radio_options(&conn, &EntityId::Str("seed".into())).unwrap();
        assert_eq!(opts.moods.len(), MOOD_BUCKETS.len());
        assert_eq!(opts.auto_moods, vec!["melancholic"]);
        let melancholic = opts
            .moods
            .iter()
            .find(|m| m.bucket == "melancholic")
            .unwrap();
        assert!(melancholic.auto);
        assert!(
            !melancholic.available,
            "3 tracks is under the availability floor"
        );
        assert!(opts.tags.contains(&"trip-hop".to_owned()));
        assert_eq!(opts.library_tag_progress.candidates, 7);
        assert_eq!(opts.library_tag_progress.tagged, 0);
    }
}
