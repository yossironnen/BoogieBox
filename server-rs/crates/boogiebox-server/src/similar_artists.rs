//! Local-only similar artist resolution and deterministic provider ranking.

use crate::providers::RelatedArtistCandidate;
use boogiebox_db::music::{
    find_owned_artist_by_external_identity, find_owned_artist_by_name, get_artist,
    ArtistIdentityProvider, EntityId,
};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;

/// A provider-ranked candidate resolved onto a release-owning local artist.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SimilarArtistResult {
    pub id: EntityId,
    pub name: String,
    pub rating: Option<f64>,
    pub track_count: i64,
    pub album_count: i64,
    pub score: f64,
    pub providers: Vec<String>,
}

#[derive(Debug)]
struct RankedMatch {
    score: f64,
    lastfm_rank: Option<usize>,
    deezer_rank: Option<usize>,
    providers: Vec<String>,
}

fn resolve_candidate(
    conn: &Connection,
    provider: ArtistIdentityProvider,
    candidate: &RelatedArtistCandidate,
) -> rusqlite::Result<Option<EntityId>> {
    if let Some(external_id) = candidate.external_id.as_deref() {
        if let Some(local_id) = find_owned_artist_by_external_identity(conn, provider, external_id)?
        {
            return Ok(Some(local_id));
        }
    }
    find_owned_artist_by_name(conn, &candidate.name)
}

fn deezer_rank_score(rank: usize) -> f64 {
    0.65 / (1.0 + 0.08 * rank.saturating_sub(1) as f64)
}

/// Intersects provider candidates with the local collection, rejects
/// ambiguous/non-owning matches, combines evidence, and returns a stable rank.
pub fn resolve_local_similar_artists(
    conn: &Connection,
    user_id: &str,
    source_artist_id: &EntityId,
    lastfm: &[RelatedArtistCandidate],
    deezer: &[RelatedArtistCandidate],
    limit: usize,
) -> rusqlite::Result<Vec<SimilarArtistResult>> {
    let mut matches: HashMap<EntityId, RankedMatch> = HashMap::new();

    for candidate in lastfm {
        let Some(local_id) = resolve_candidate(conn, ArtistIdentityProvider::LastFm, candidate)?
        else {
            continue;
        };
        if &local_id == source_artist_id {
            continue;
        }
        let score = candidate
            .match_score
            .unwrap_or_else(|| 1.0 / candidate.rank.max(1) as f64)
            .clamp(0.0, 1.0);
        matches
            .entry(local_id)
            .and_modify(|existing| {
                existing.score = existing.score.max(score);
                existing.lastfm_rank = Some(candidate.rank);
                if !existing.providers.iter().any(|value| value == "lastfm") {
                    existing.providers.push("lastfm".to_owned());
                }
            })
            .or_insert_with(|| RankedMatch {
                score,
                lastfm_rank: Some(candidate.rank),
                deezer_rank: None,
                providers: vec!["lastfm".to_owned()],
            });
    }

    for candidate in deezer {
        let Some(local_id) = resolve_candidate(conn, ArtistIdentityProvider::Deezer, candidate)?
        else {
            continue;
        };
        if &local_id == source_artist_id {
            continue;
        }
        let score = deezer_rank_score(candidate.rank.max(1));
        matches
            .entry(local_id)
            .and_modify(|existing| {
                existing.score = (existing.score + 0.08).clamp(0.0, 1.0);
                existing.deezer_rank = Some(candidate.rank);
                if !existing.providers.iter().any(|value| value == "deezer") {
                    existing.providers.push("deezer".to_owned());
                }
            })
            .or_insert_with(|| RankedMatch {
                score,
                lastfm_rank: None,
                deezer_rank: Some(candidate.rank),
                providers: vec!["deezer".to_owned()],
            });
    }

    let mut results = Vec::with_capacity(matches.len());
    for (artist_id, ranked) in matches {
        let Some(artist) = get_artist(conn, user_id, &artist_id)? else {
            continue;
        };
        if artist.album_count < 1 {
            continue;
        }
        results.push((
            SimilarArtistResult {
                id: artist.id,
                name: artist.name,
                rating: artist.rating,
                track_count: artist.track_count,
                album_count: artist.album_count,
                score: ranked.score,
                providers: ranked.providers,
            },
            ranked.lastfm_rank,
            ranked.deezer_rank,
        ));
    }

    results.sort_by(
        |(left, left_lastfm, left_deezer), (right, right_lastfm, right_deezer)| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| {
                    left_lastfm
                        .unwrap_or(usize::MAX)
                        .cmp(&right_lastfm.unwrap_or(usize::MAX))
                })
                .then_with(|| {
                    left_deezer
                        .unwrap_or(usize::MAX)
                        .cmp(&right_deezer.unwrap_or(usize::MAX))
                })
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
                .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
        },
    );
    results.truncate(limit);
    Ok(results.into_iter().map(|(result, _, _)| result).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use boogiebox_db::{initialize_schema, music::coerce_entity_id};

    fn candidate(
        id: Option<&str>,
        name: &str,
        score: Option<f64>,
        rank: usize,
    ) -> RelatedArtistCandidate {
        RelatedArtistCandidate {
            external_id: id.map(str::to_owned),
            name: name.to_owned(),
            url: None,
            image_url: None,
            match_score: score,
            rank,
        }
    }

    #[test]
    fn resolves_local_owned_artists_dedupes_and_boosts_consensus() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO users(id, username) VALUES('user-1', 'user');
             INSERT INTO libraries(id, path, name) VALUES('library-1', 'D:/Music', 'Music');
             INSERT INTO artists(id, name, lastfm_mbid, deezer_artist_id) VALUES
               ('source', 'Source', 'source-mbid', 'source-deezer'),
               ('portishead', 'Portishead', 'p-mbid', 'p-deezer'),
               ('tricky', 'Tricky', 't-mbid', 't-deezer'),
               ('appears', 'Appears Only', 'a-mbid', 'a-deezer');
             INSERT INTO albums(id, title, album_artist, artist_id) VALUES
               ('source-album', 'Source Album', 'Source', 'source'),
               ('p-album', 'Dummy', 'Portishead', 'portishead'),
               ('t-album', 'Maxinquaye', 'Tricky', 'tricky');
             INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path) VALUES
               ('source-track', 'library-1', 'source', 'source-album', 'Source Track', 'D:/Music/source.flac'),
               ('p-track', 'library-1', 'portishead', 'p-album', 'Roads', 'D:/Music/roads.flac'),
               ('t-track', 'library-1', 'tricky', 't-album', 'Hell Is Round the Corner', 'D:/Music/tricky.flac'),
               ('a-track', 'library-1', 'appears', 'source-album', 'Guest', 'D:/Music/guest.flac');",
        )
        .unwrap();

        let results = resolve_local_similar_artists(
            &conn,
            "user-1",
            &coerce_entity_id("source"),
            &[
                candidate(Some("p-mbid"), "Wrong Provider Name", Some(0.8), 2),
                candidate(Some("t-mbid"), "Tricky", Some(0.82), 1),
                candidate(Some("source-mbid"), "Source", Some(1.0), 3),
                candidate(Some("a-mbid"), "Appears Only", Some(0.99), 4),
            ],
            &[candidate(Some("p-deezer"), "Portishead", None, 1)],
            10,
        )
        .unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, coerce_entity_id("portishead"));
        assert_eq!(results[0].providers, vec!["lastfm", "deezer"]);
        assert_eq!(results[1].id, coerce_entity_id("tricky"));
    }

    #[test]
    fn name_fallback_rejects_homonyms_and_limit_is_deterministic() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO users(id, username) VALUES('user-1', 'user');
             INSERT INTO artists(id, name) VALUES
               ('source', 'Source'), ('same-1', 'Same Name'), ('same-2', 'Same Name'),
               ('alpha', 'Alpha'), ('beta', 'Beta');
             INSERT INTO albums(id, title, album_artist, artist_id) VALUES
               ('same-a', 'A', 'Same Name', 'same-1'), ('same-b', 'B', 'Same Name', 'same-2'),
               ('alpha-a', 'A', 'Alpha', 'alpha'), ('beta-a', 'B', 'Beta', 'beta');",
        )
        .unwrap();

        let results = resolve_local_similar_artists(
            &conn,
            "user-1",
            &coerce_entity_id("source"),
            &[
                candidate(None, "Same Name", Some(1.0), 1),
                candidate(None, "Beta", Some(0.5), 3),
                candidate(None, "Alpha", Some(0.5), 2),
            ],
            &[],
            1,
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Alpha");
    }
}
