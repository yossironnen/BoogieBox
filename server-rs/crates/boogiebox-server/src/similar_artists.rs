//! Local-only similar artist resolution and deterministic provider ranking.

use crate::{
    providers::{
        fetch_deezer_related_artists, fetch_listenbrainz_similar_artists, RelatedArtistCandidate,
    },
    DbPool,
};
use boogiebox_db::{
    artwork::{get_lastfm_cached, get_lastfm_cached_stale, save_lastfm_cache},
    music::{
        find_owned_artist_by_external_identity, find_owned_artist_by_name, get_artist,
        get_artist_external_identity, ArtistIdentityProvider, EntityId,
    },
    radio::get_artist_musicbrainz_id,
};
use reqwest::Client;
use rusqlite::Connection;
use serde::Serialize;
use std::{collections::HashMap, time::Duration};

/// Cache lifetime for provider similarity graphs.
const RELATED_CACHE_DAYS: i64 = 7;

/// Raw provider similarity candidates for one artist, before local resolution.
#[derive(Debug, Default, Clone)]
pub struct RelatedSet {
    pub deezer: Vec<RelatedArtistCandidate>,
    pub listenbrainz: Vec<RelatedArtistCandidate>,
}

fn decode_related_cache(payload: Option<String>) -> Option<Vec<RelatedArtistCandidate>> {
    payload.and_then(|raw| serde_json::from_str(&raw).ok())
}

/// Reads a bool setting that defaults to `true` when unset.
struct RelatedContext {
    deezer_artist_id: Option<String>,
    musicbrainz_id: Option<String>,
    keyless: bool,
    keys: [String; 2],
    fresh: [Option<String>; 2],
    stale: [Option<String>; 2],
}

/// Gathers Deezer / ListenBrainz similar-artist candidates for one
/// artist, cache-first (7 days). On a miss it fetches, bounded by
/// `network_budget` per provider when given (the radio path), and falls back to
/// stale cache when a provider fails or times out. `Ok(None)` = unknown artist.
pub async fn gather_related_candidates(
    db: &DbPool,
    http_client: &Client,
    artist_id: &EntityId,
    network_budget: Option<Duration>,
) -> rusqlite::Result<Option<RelatedSet>> {
    let ctx_db = db.clone();
    let source_id = artist_id.clone();
    let context =
        tokio::task::spawn_blocking(move || -> rusqlite::Result<Option<RelatedContext>> {
            let conn = ctx_db.lock().unwrap_or_else(|p| p.into_inner());
            let Some(identity) = get_artist_external_identity(&conn, &source_id)? else {
                return Ok(None);
            };
            let source_key = identity.artist_id.to_string();
            let keys = [
                format!("artist-similar:deezer:{source_key}"),
                format!("artist-similar:listenbrainz:{source_key}"),
            ];
            let fresh = [
                get_lastfm_cached(&conn, &keys[0]),
                get_lastfm_cached(&conn, &keys[1]),
            ];
            let stale = [
                get_lastfm_cached_stale(&conn, &keys[0]),
                get_lastfm_cached_stale(&conn, &keys[1]),
            ];
            Ok(Some(RelatedContext {
                deezer_artist_id: identity.deezer_artist_id,
                musicbrainz_id: get_artist_musicbrainz_id(&conn, &source_id)?,
                keyless: crate::radio_metadata::keyless_enabled(&conn),
                keys,
                fresh,
                stale,
            }))
        })
        .await
        .map_err(|_| rusqlite::Error::InvalidQuery)??;
    let Some(ctx) = context else {
        return Ok(None);
    };

    let mut cache_updates: Vec<(String, String)> = Vec::new();
    let budget = |duration: Option<Duration>| duration.unwrap_or(Duration::from_secs(60));
    let limit = budget(network_budget);

    let deezer = if let Some(cached) = decode_related_cache(ctx.fresh[0].clone()) {
        cached
    } else if let Some(deezer_id) = ctx.deezer_artist_id.as_deref() {
        match tokio::time::timeout(
            limit,
            fetch_deezer_related_artists(http_client, deezer_id, 100),
        )
        .await
        {
            Ok(Ok(candidates)) => {
                if let Ok(payload) = serde_json::to_string(&candidates) {
                    cache_updates.push((ctx.keys[0].clone(), payload));
                }
                candidates
            }
            _ => decode_related_cache(ctx.stale[0].clone()).unwrap_or_default(),
        }
    } else {
        decode_related_cache(ctx.stale[0].clone()).unwrap_or_default()
    };

    let listenbrainz = if !ctx.keyless {
        Vec::new()
    } else if let Some(cached) = decode_related_cache(ctx.fresh[1].clone()) {
        cached
    } else if let Some(mbid) = ctx.musicbrainz_id.as_deref() {
        match tokio::time::timeout(
            limit,
            fetch_listenbrainz_similar_artists(http_client, mbid, 100),
        )
        .await
        {
            Ok(Ok(candidates)) => {
                if let Ok(payload) = serde_json::to_string(&candidates) {
                    cache_updates.push((ctx.keys[1].clone(), payload));
                }
                candidates
            }
            _ => decode_related_cache(ctx.stale[1].clone()).unwrap_or_default(),
        }
    } else {
        decode_related_cache(ctx.stale[1].clone()).unwrap_or_default()
    };

    if !cache_updates.is_empty() {
        let cache_db = db.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let conn = cache_db.lock().unwrap_or_else(|p| p.into_inner());
            for (key, payload) in cache_updates {
                save_lastfm_cache(&conn, &key, &payload, RELATED_CACHE_DAYS);
            }
        })
        .await;
    }
    Ok(Some(RelatedSet {
        deezer,
        listenbrainz,
    }))
}

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
    1.0 / (1.0 + 0.08 * rank.saturating_sub(1) as f64)
}

fn listenbrainz_rank_score(candidate: &RelatedArtistCandidate) -> f64 {
    candidate
        .match_score
        .unwrap_or_else(|| 1.0 / candidate.rank.max(1) as f64)
        .clamp(0.0, 1.0)
}

/// Intersects provider candidates with the local collection, rejects
/// ambiguous/non-owning matches, combines evidence, and returns a stable rank.
/// Either source can stand alone; ListenBrainz adds a small consensus boost
/// when Deezer agrees.
pub fn resolve_local_similar_artists(
    conn: &Connection,
    user_id: &str,
    source_artist_id: &EntityId,
    deezer: &[RelatedArtistCandidate],
    listenbrainz: &[RelatedArtistCandidate],
    limit: usize,
) -> rusqlite::Result<Vec<SimilarArtistResult>> {
    let mut matches: HashMap<EntityId, RankedMatch> = HashMap::new();

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
                deezer_rank: Some(candidate.rank),
                providers: vec!["deezer".to_owned()],
            });
    }

    for candidate in listenbrainz {
        let Some(local_id) =
            resolve_candidate(conn, ArtistIdentityProvider::MusicBrainz, candidate)?
        else {
            continue;
        };
        if &local_id == source_artist_id {
            continue;
        }
        let score = listenbrainz_rank_score(candidate);
        matches
            .entry(local_id)
            .and_modify(|existing| {
                existing.score = (existing.score + 0.06).clamp(0.0, 1.0);
                if !existing
                    .providers
                    .iter()
                    .any(|value| value == "listenbrainz")
                {
                    existing.providers.push("listenbrainz".to_owned());
                }
            })
            .or_insert_with(|| RankedMatch {
                score,
                deezer_rank: None,
                providers: vec!["listenbrainz".to_owned()],
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
            ranked.deezer_rank,
        ));
    }

    results.sort_by(|(left, left_deezer), (right, right_deezer)| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| {
                left_deezer
                    .unwrap_or(usize::MAX)
                    .cmp(&right_deezer.unwrap_or(usize::MAX))
            })
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.id.to_string().cmp(&right.id.to_string()))
    });
    results.truncate(limit);
    Ok(results.into_iter().map(|(result, _)| result).collect())
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
             INSERT INTO artists(id, name, musicbrainz_artist_id, deezer_artist_id) VALUES
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
                candidate(Some("p-deezer"), "Wrong Provider Name", None, 2),
                candidate(Some("t-deezer"), "Tricky", None, 1),
                candidate(Some("source-deezer"), "Source", None, 3),
                candidate(Some("a-deezer"), "Appears Only", None, 4),
            ],
            &[candidate(Some("p-mbid"), "Portishead", Some(0.95), 1)],
            10,
        )
        .unwrap();

        assert_eq!(
            results.len(),
            2,
            "the source and an artist without albums drop out"
        );
        assert_eq!(results[0].id, coerce_entity_id("tricky"));
        assert_eq!(results[1].id, coerce_entity_id("portishead"));
        assert_eq!(results[1].providers, vec!["deezer", "listenbrainz"]);
        assert!((results[1].score - (deezer_rank_score(2) + 0.06)).abs() < 1e-9);
    }

    #[test]
    fn listenbrainz_stands_alone_resolves_by_mbid_and_boosts_consensus() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO users(id, username) VALUES('user-1', 'user');
             INSERT INTO artists(id, name, musicbrainz_artist_id, lastfm_mbid) VALUES
               ('source', 'Source', NULL, NULL),
               ('portishead', 'Portishead', 'mb-portishead', NULL),
               ('tricky', 'Tricky', NULL, 'lfm-tricky');
             INSERT INTO albums(id, title, album_artist, artist_id) VALUES
               ('p-a', 'Dummy', 'Portishead', 'portishead'),
               ('t-a', 'Maxinquaye', 'Tricky', 'tricky');",
        )
        .unwrap();
        let lb = [
            candidate(Some("mb-portishead"), "Portishead", Some(1.0), 1),
            candidate(Some("lfm-tricky"), "Tricky", Some(0.4), 2), // matched via lastfm_mbid
            candidate(Some("unknown-mbid"), "Nobody", Some(0.3), 3),
        ];
        let alone = resolve_local_similar_artists(
            &conn,
            "user-1",
            &coerce_entity_id("source"),
            &[],
            &lb,
            10,
        )
        .unwrap();
        assert_eq!(alone.len(), 2);
        assert_eq!(alone[0].name, "Portishead");
        assert_eq!(alone[0].providers, vec!["listenbrainz"]);
        assert!((alone[0].score - 1.0).abs() < 1e-9);
        // Rank-only candidates (no score) still score.
        assert!((listenbrainz_rank_score(&candidate(None, "X", None, 2)) - 0.5).abs() < 1e-9);
        assert!((deezer_rank_score(1) - 1.0).abs() < 1e-9);
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
            &[],
            &[
                candidate(None, "Same Name", Some(1.0), 1),
                candidate(None, "Beta", Some(0.5), 3),
                candidate(None, "Alpha", Some(0.5), 2),
            ],
            1,
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Alpha");
    }

    // -- gather_related_candidates ------------------------------------------------

    use crate::providers::provider_fetch_tests::ENV_LOCK;
    use std::sync::{Arc, Mutex};
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    const MBID: &str = "10adbe5e-a2c0-4bf3-8249-2b4cbf6e6ca8";
    const PEER: &str = "8f6bd1e4-fbe1-4f50-aa9b-94c450ec0f11";

    fn gather_db() -> DbPool {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();
        conn.execute_batch(&format!(
            "INSERT INTO artists(id, name, lastfm_mbid, deezer_artist_id) VALUES
               ('src', 'Source', '{MBID}', 'dz-1'), ('nomb', 'No Mbid', NULL, NULL);"
        ))
        .unwrap();
        Arc::new(Mutex::new(conn))
    }

    fn set(db: &DbPool, key: &str, value: &str) {
        db.lock()
            .unwrap()
            .execute(
                "INSERT INTO settings(key, value, updated_at) VALUES(?1, ?2, datetime('now'))
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                rusqlite::params![key, value],
            )
            .unwrap();
    }

    fn cached(db: &DbPool, key: &str) -> Option<String> {
        boogiebox_db::artwork::get_lastfm_cached(&db.lock().unwrap(), key)
    }

    #[tokio::test]
    async fn gather_fetches_both_sources_then_serves_them_from_cache() {
        let _env = ENV_LOCK.lock().await;
        let deezer = MockServer::start().await;
        let labs = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_DEEZER_API_BASE", deezer.uri());
        std::env::set_var("BOOGIEBOX_LISTENBRAINZ_LABS_API_BASE", labs.uri());
        Mock::given(method("GET"))
            .and(path("/artist/dz-1/related"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "id": 7, "name": "Tricky" }]
            })))
            .expect(1)
            .mount(&deezer)
            .await;
        Mock::given(method("GET"))
            .and(path("/similar-artists/json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "artist_mbid": PEER, "name": "Portishead", "score": 100 }
            ])))
            .expect(1)
            .mount(&labs)
            .await;

        let db = gather_db();
        // An old Last.fm list in the cache is not read any more.
        boogiebox_db::artwork::save_lastfm_cache(
            &db.lock().unwrap(),
            "artist-similar:lastfm:src",
            r#"[{"name":"Old","rank":1}]"#,
            7,
        );
        let client = Client::new();
        let id = coerce_entity_id("src");
        let first = gather_related_candidates(&db, &client, &id, Some(Duration::from_secs(5)))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.deezer[0].name, "Tricky");
        assert_eq!(first.listenbrainz[0].external_id.as_deref(), Some(PEER));
        assert!(cached(&db, "artist-similar:listenbrainz:src").is_some());

        // Second call: everything is cached, so no provider is contacted again
        // (each mock allows exactly one request).
        let second = gather_related_candidates(&db, &client, &id, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(second.deezer, first.deezer);
        assert_eq!(second.listenbrainz, first.listenbrainz);
        deezer.verify().await;
        labs.verify().await;

        for var in [
            "BOOGIEBOX_DEEZER_API_BASE",
            "BOOGIEBOX_LISTENBRAINZ_LABS_API_BASE",
        ] {
            std::env::remove_var(var);
        }
    }

    #[tokio::test]
    async fn gather_skips_listenbrainz_when_keyless_is_off_and_needs_an_mbid() {
        let _env = ENV_LOCK.lock().await;
        let labs = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LISTENBRAINZ_LABS_API_BASE", labs.uri());
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&labs)
            .await;
        let db = gather_db();
        let client = Client::new();

        set(&db, "radioKeylessProviders", "false");
        let off = gather_related_candidates(&db, &client, &coerce_entity_id("src"), None)
            .await
            .unwrap()
            .unwrap();
        assert!(off.listenbrainz.is_empty());
        set(&db, "radioKeylessProviders", "true");
        // An artist with no MusicBrainz id can't be looked up.
        let no_id = gather_related_candidates(&db, &client, &coerce_entity_id("nomb"), None)
            .await
            .unwrap()
            .unwrap();
        assert!(no_id.listenbrainz.is_empty());
        assert!(labs.received_requests().await.unwrap().is_empty());
        // Unknown artist.
        assert!(
            gather_related_candidates(&db, &client, &coerce_entity_id("ghost"), None)
                .await
                .unwrap()
                .is_none()
        );
        std::env::remove_var("BOOGIEBOX_LISTENBRAINZ_LABS_API_BASE");
    }

    #[tokio::test]
    async fn gather_falls_back_to_stale_cache_when_a_provider_fails_or_is_too_slow() {
        let _env = ENV_LOCK.lock().await;
        let labs = MockServer::start().await;
        std::env::set_var("BOOGIEBOX_LISTENBRAINZ_LABS_API_BASE", labs.uri());
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(400))
                    .set_body_json(serde_json::json!([])),
            )
            .mount(&labs)
            .await;
        let db = gather_db();
        let stale = serde_json::to_string(&vec![candidate(Some(PEER), "Portishead", Some(0.5), 1)])
            .unwrap();
        {
            let conn = db.lock().unwrap();
            conn.execute(
                "INSERT INTO lastfm_cache(cache_key, data, fetched_at, expires_at)
                 VALUES('artist-similar:listenbrainz:src', ?1, datetime('now','-30 days'), datetime('now','-20 days'))",
                [&stale],
            )
            .unwrap();
        }
        let client = Client::new();
        // Too slow for the budget → the expired cache entry is served instead.
        let timed_out = gather_related_candidates(
            &db,
            &client,
            &coerce_entity_id("src"),
            Some(Duration::from_millis(50)),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(timed_out.listenbrainz.len(), 1);
        // A provider error also falls back to the stale entry.
        labs.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&labs)
            .await;
        let failed = gather_related_candidates(&db, &client, &coerce_entity_id("src"), None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(failed.listenbrainz.len(), 1);
        std::env::remove_var("BOOGIEBOX_LISTENBRAINZ_LABS_API_BASE");
    }
}
