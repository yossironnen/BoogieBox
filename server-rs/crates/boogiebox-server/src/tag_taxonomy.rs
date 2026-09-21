//! Provider tag normalization and classification for Artist Radio
//! (`wip/artist-radio-v2-plan.md` §2.1): folds spelling variants, drops junk,
//! and sorts every tag into genre / mood / era, with moods mapped onto a closed
//! set of buckets so the UI and the ranking engine stay bounded.

use boogiebox_db::radio::TagInput;

/// The closed set of mood buckets offered in the UI, in display order.
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

/// Raw tags (already normalized) that map to each mood bucket.
const MOOD_TAGS: &[(&str, &[&str])] = &[
    (
        "chill",
        &[
            "chill",
            "chillout",
            "chill out",
            "relaxing",
            "relax",
            "relaxed",
            "mellow",
            "calm",
            "calming",
            "soothing",
            "laid back",
            "laid-back",
            "easy listening",
            "lounge",
            "smooth",
            "peaceful",
            "sleep",
            "cozy",
        ],
    ),
    (
        "melancholic",
        &[
            "melancholy",
            "melancholic",
            "sad",
            "sadness",
            "depressing",
            "depressive",
            "sorrow",
            "bittersweet",
            "heartbreak",
            "lonely",
            "gloomy",
            "somber",
            "nostalgic",
            "nostalgia",
            "emotional",
            "wistful",
            "mournful",
        ],
    ),
    (
        "uplifting",
        &[
            "happy",
            "uplifting",
            "feel good",
            "feel-good",
            "feelgood",
            "joyful",
            "cheerful",
            "upbeat",
            "positive",
            "sunny",
            "summer",
            "fun",
            "euphoric",
            "optimistic",
            "hopeful",
            "inspiring",
            "motivational",
        ],
    ),
    (
        "energetic",
        &[
            "energetic",
            "energy",
            "party",
            "danceable",
            "driving",
            "workout",
            "running",
            "pump up",
            "hype",
            "powerful",
            "lively",
        ],
    ),
    (
        "dark",
        &[
            "dark",
            "darkness",
            "brooding",
            "sinister",
            "eerie",
            "creepy",
            "haunting",
            "ominous",
            "nocturnal",
            "night",
            "noir",
            "menacing",
        ],
    ),
    (
        "dreamy",
        &[
            "dreamy",
            "dreamlike",
            "ethereal",
            "atmospheric",
            "hypnotic",
            "spacey",
            "trippy",
            "lush",
            "cinematic",
            "airy",
            "floaty",
        ],
    ),
    (
        "romantic",
        &[
            "romantic",
            "love song",
            "love songs",
            "sensual",
            "sexy",
            "seductive",
            "intimate",
            "passionate",
            "tender",
        ],
    ),
    (
        "aggressive",
        &[
            "aggressive",
            "angry",
            "rage",
            "intense",
            "heavy",
            "brutal",
            "fierce",
            "violent",
        ],
    ),
];

/// `(valence, energy)` on a 0..1 plane for each bucket, used for partial
/// credit between related moods (e.g. dreamy ≈ chill, aggressive ≈ energetic).
fn mood_coordinates(bucket: &str) -> Option<(f64, f64)> {
    Some(match bucket {
        "chill" => (0.55, 0.2),
        "melancholic" => (0.15, 0.25),
        "uplifting" => (0.9, 0.7),
        "energetic" => (0.7, 0.95),
        "dark" => (0.15, 0.5),
        "dreamy" => (0.5, 0.25),
        "romantic" => (0.7, 0.35),
        "aggressive" => (0.2, 0.95),
        _ => return None,
    })
}

/// Tags that carry no musical information (listening habits, praise, formats).
const JUNK_TAGS: &[&str] = &[
    "seen live",
    "favorites",
    "favourites",
    "favorite",
    "favourite",
    "my favorites",
    "my favourites",
    "female vocalists",
    "female vocalist",
    "male vocalists",
    "male vocalist",
    "albums i own",
    "awesome",
    "good",
    "best",
    "love",
    "loved",
    "spotify",
    "all",
    "check out",
    "under 2000 listeners",
    "cool",
    "great",
    "amazing",
];

/// Spelling/alias folds applied after lowercasing.
const ALIASES: &[(&str, &str)] = &[
    ("trip hop", "trip-hop"),
    ("triphop", "trip-hop"),
    ("hip hop", "hip-hop"),
    ("hiphop", "hip-hop"),
    ("rnb", "r&b"),
    ("r and b", "r&b"),
    ("r'n'b", "r&b"),
    ("electronica", "electronic"),
    ("drum and bass", "drum-and-bass"),
    ("drum n bass", "drum-and-bass"),
    ("dnb", "drum-and-bass"),
    ("synth pop", "synthpop"),
    ("synth-pop", "synthpop"),
    ("post rock", "post-rock"),
    ("post punk", "post-punk"),
    ("indie rock", "indie rock"),
    ("chill-out", "chill out"),
    ("melancholia", "melancholy"),
];

/// Coarse classification of a normalized tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagKind {
    Genre,
    Mood,
    Era,
}

impl TagKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TagKind::Genre => "genre",
            TagKind::Mood => "mood",
            TagKind::Era => "era",
        }
    }
}

/// A classified tag: its kind and, for moods, the bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Classified {
    pub kind: TagKind,
    pub bucket: Option<&'static str>,
}

fn is_year_or_decade(tag: &str) -> bool {
    let digits = tag.trim_end_matches('s');
    let plain_year = tag.len() == 4 && tag.chars().all(|c| c.is_ascii_digit());
    let decade = tag.ends_with('s')
        && !digits.is_empty()
        && digits.len() <= 4
        && digits.chars().all(|c| c.is_ascii_digit());
    plain_year || decade
}

/// Lowercases, trims, folds aliases, and drops junk. `None` means "discard".
/// `exclude` lists names (artist/title) that Last.fm users sometimes tag with.
pub fn normalize_tag(raw: &str, exclude: &[&str]) -> Option<String> {
    let mut tag = raw
        .replace(['_', '\u{a0}'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if let Some((_, folded)) = ALIASES.iter().find(|(from, _)| *from == tag) {
        tag = (*folded).to_owned();
    }
    if tag.chars().count() < 3 || tag.chars().count() > 40 {
        return None;
    }
    if JUNK_TAGS.contains(&tag.as_str())
        || tag.contains("seen live")
        || tag.starts_with("under ")
        || tag.contains("my ")
        || tag.contains("albums i")
        // MusicBrainz editors' internal to-do tags ("fixme", "fixme recordings merge").
        || tag.contains("fixme")
    {
        return None;
    }
    if exclude
        .iter()
        .any(|name| !name.trim().is_empty() && name.trim().to_lowercase() == tag)
    {
        return None;
    }
    Some(tag)
}

/// Sorts a normalized tag into genre / mood / era.
pub fn classify(tag: &str) -> Classified {
    let tag = tag.trim().to_lowercase();
    for (bucket, words) in MOOD_TAGS {
        if words.contains(&tag.as_str()) {
            return Classified {
                kind: TagKind::Mood,
                bucket: MOOD_BUCKETS.iter().copied().find(|b| b == bucket),
            };
        }
    }
    if is_year_or_decade(&tag) {
        return Classified {
            kind: TagKind::Era,
            bucket: None,
        };
    }
    Classified {
        kind: TagKind::Genre,
        bucket: None,
    }
}

/// The mood bucket a (normalized) tag belongs to, if any.
pub fn mood_bucket_of(tag: &str) -> Option<&'static str> {
    classify(tag).bucket
}

/// Every raw tag that maps to `bucket` (used to count how many tracks can
/// serve a mood).
pub fn tags_for_bucket(bucket: &str) -> Vec<String> {
    MOOD_TAGS
        .iter()
        .find(|(name, _)| *name == bucket)
        .map(|(_, words)| words.iter().map(|w| (*w).to_owned()).collect())
        .unwrap_or_default()
}

/// 0..1 closeness of two mood buckets on the valence/energy plane (1 = same).
pub fn mood_similarity(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    match (mood_coordinates(a), mood_coordinates(b)) {
        (Some((av, ae)), Some((bv, be))) => {
            let dist = ((av - bv).powi(2) + (ae - be).powi(2)).sqrt();
            // Max plane distance is sqrt(2); squash so unrelated moods score ~0.
            (1.0 - dist / 0.9).clamp(0.0, 1.0) * 0.6
        }
        _ => 0.0,
    }
}

/// Turns a provider's raw `(tag, count)` list into persistable tags:
/// normalized, classified, deduplicated, weighted relative to the strongest
/// kept tag, and capped at `max_tags`.
pub fn to_tag_inputs(
    raw: &[(String, u64)],
    min_abs: u64,
    min_rel: f64,
    max_tags: usize,
    exclude: &[&str],
) -> Vec<TagInput> {
    let max_count = raw.iter().map(|(_, c)| *c).max().unwrap_or(0);
    if max_count == 0 {
        return Vec::new();
    }
    let mut ranked: Vec<&(String, u64)> = raw.iter().collect();
    ranked.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    let mut out: Vec<TagInput> = Vec::new();
    for (name, count) in ranked {
        if *count < min_abs || (*count as f64) < min_rel * max_count as f64 {
            continue;
        }
        let Some(tag) = normalize_tag(name, exclude) else {
            continue;
        };
        if out.iter().any(|t| t.tag == tag) {
            continue;
        }
        let class = classify(&tag);
        out.push(TagInput {
            tag,
            kind: class.kind.as_str().to_owned(),
            bucket: class.bucket.map(str::to_owned),
            weight: (*count as f64 / max_count as f64).clamp(0.0, 1.0),
        });
        if out.len() >= max_tags {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_folds_aliases_and_drops_junk() {
        assert_eq!(
            normalize_tag("  Trip  Hop ", &[]).as_deref(),
            Some("trip-hop")
        );
        assert_eq!(normalize_tag("Hip_Hop", &[]).as_deref(), Some("hip-hop"));
        assert_eq!(normalize_tag("RnB", &[]).as_deref(), Some("r&b"));
        assert_eq!(
            normalize_tag("Melancholia", &[]).as_deref(),
            Some("melancholy")
        );
        for junk in [
            "seen live",
            "Favorites",
            "female vocalists",
            "under 2000 listeners",
            "fixme",
            "FIXME recordings merge",
            "my top songs",
            "ab",
            "x".repeat(41).as_str(),
        ] {
            assert_eq!(normalize_tag(junk, &[]), None, "{junk}");
        }
    }

    #[test]
    fn normalize_rejects_the_artist_or_title_name() {
        assert_eq!(
            normalize_tag("Massive Attack", &["Massive Attack", "Teardrop"]),
            None
        );
        assert_eq!(
            normalize_tag("teardrop", &["Massive Attack", "Teardrop"]),
            None
        );
        assert_eq!(
            normalize_tag("ambient", &["", "  "]).as_deref(),
            Some("ambient")
        );
    }

    #[test]
    fn every_mood_bucket_is_reachable_and_consistent() {
        for bucket in MOOD_BUCKETS {
            let tags = tags_for_bucket(bucket);
            assert!(!tags.is_empty(), "{bucket} has tags");
            for tag in &tags {
                let class = classify(tag);
                assert_eq!(class.kind, TagKind::Mood, "{tag}");
                assert_eq!(class.bucket, Some(bucket), "{tag}");
                assert_eq!(mood_bucket_of(tag), Some(bucket));
            }
            assert!(mood_coordinates(bucket).is_some());
        }
        assert!(tags_for_bucket("nonsense").is_empty());
    }

    #[test]
    fn mood_tables_have_no_duplicate_words_across_buckets() {
        let mut seen = std::collections::HashSet::new();
        for (_, words) in MOOD_TAGS {
            for word in *words {
                assert!(seen.insert(*word), "duplicate mood word {word}");
            }
        }
        assert_eq!(MOOD_TAGS.len(), MOOD_BUCKETS.len());
    }

    #[test]
    fn classify_genres_eras_and_unknowns() {
        assert_eq!(classify("trip-hop").kind, TagKind::Genre);
        assert_eq!(classify("90s").kind, TagKind::Era);
        assert_eq!(classify("1998").kind, TagKind::Era);
        assert_eq!(classify("2000s").kind, TagKind::Era);
        assert_eq!(classify("Melancholic").bucket, Some("melancholic"));
        assert_eq!(classify("shoegaze").bucket, None);
    }

    #[test]
    fn mood_similarity_orders_related_moods_above_unrelated() {
        assert_eq!(mood_similarity("chill", "chill"), 1.0);
        assert!(mood_similarity("chill", "dreamy") > mood_similarity("chill", "aggressive"));
        assert!(
            mood_similarity("aggressive", "energetic") > mood_similarity("aggressive", "romantic")
        );
        assert_eq!(mood_similarity("chill", "unknown"), 0.0);
        assert!(mood_similarity("chill", "dreamy") < 1.0);
    }

    #[test]
    fn to_tag_inputs_weights_dedupes_filters_and_caps() {
        let raw = vec![
            ("Trip Hop".to_owned(), 100),
            ("trip-hop".to_owned(), 90), // duplicate after alias fold
            ("melancholy".to_owned(), 60),
            ("seen live".to_owned(), 80),
            ("90s".to_owned(), 40),
            ("weak".to_owned(), 5),
        ];
        let out = to_tag_inputs(&raw, 10, 0.0, 3, &[]);
        let names: Vec<&str> = out.iter().map(|t| t.tag.as_str()).collect();
        assert_eq!(names, vec!["trip-hop", "melancholy", "90s"]);
        assert_eq!(out[0].weight, 1.0);
        assert!((out[1].weight - 0.6).abs() < 1e-9);
        assert_eq!(out[1].kind, "mood");
        assert_eq!(out[1].bucket.as_deref(), Some("melancholic"));
        assert_eq!(out[2].kind, "era");
        assert_eq!(out[0].bucket, None);

        // Relative threshold (MusicBrainz-style vote counts).
        let mb = vec![("downtempo".to_owned(), 12), ("british".to_owned(), 1)];
        assert_eq!(to_tag_inputs(&mb, 1, 0.2, 10, &[]).len(), 1);
        assert!(to_tag_inputs(&[], 1, 0.0, 10, &[]).is_empty());
        assert!(to_tag_inputs(&[("ambient".to_owned(), 0)], 0, 0.0, 10, &[]).is_empty());
    }

    #[test]
    fn is_year_or_decade_edge_cases() {
        assert!(is_year_or_decade("1985"));
        assert!(is_year_or_decade("80s"));
        assert!(!is_year_or_decade("s"));
        assert!(!is_year_or_decade("rock"));
        assert!(!is_year_or_decade("12345"));
    }
}
