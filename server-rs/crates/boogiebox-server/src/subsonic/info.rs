//! Subsonic info methods: lyrics, artist/album info, similar and top songs (via Artist Radio) and now playing.
//! Provider data comes from BoogieBox's caches; network lookups are bounded so apps never wait long.

use std::{collections::HashMap, time::Duration};

use boogiebox_db::{
    music::coerce_entity_id,
    subsonic::{self as sdb, ItemKind, SubsonicArtist},
};
use serde_json::Value as Json;

use super::{
    handlers::{db_err, required_id, song_annotations, songs_nodes, Ctx},
    model::{self, artist_id3_node, artist_legacy_node, song_node},
    response::{Node, SubsonicError},
};
use crate::artist_radio::{build_radio, Focus, RadioOptions, DEFAULT_VARIETY};

/// Longest a lyrics provider lookup may take for an app request.
const LYRICS_BUDGET: Duration = Duration::from_secs(5);
/// Per-provider budget for similar-artist lookups (cache first).
const SIMILAR_BUDGET: Duration = Duration::from_secs(2);
/// Largest similar-songs queue.
const MAX_SIMILAR: i64 = 300;

fn text_node(name: &'static str, value: impl Into<String>) -> Node {
    Node::new(name).text(value.into())
}

/// A cached Last.fm JSON document (`artist:…`, `album:…`, `toptracks:…`), never fetched here.
fn cached_lastfm(conn: &rusqlite::Connection, key: &str) -> Option<Json> {
    boogiebox_db::artwork::get_lastfm_cached(conn, key)
        .and_then(|raw| serde_json::from_str(&raw).ok())
}

fn json_str(value: &Json, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Json::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

// ── Lyrics ─────────────────────────────────────────────────────────────────────

/// `getLyrics` (`artist`, `title`): cached lyrics only.
pub async fn lyrics(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let artist = ctx.params.text("artist").map(str::to_owned);
    let title = ctx.params.text("title").map(str::to_owned);
    let (Some(artist), Some(title)) = (artist, title) else {
        return Ok(Node::new("payload").object(Node::new("lyrics")));
    };
    let (a, t) = (artist.clone(), title.clone());
    let found = ctx
        .db(move |conn| sdb::cached_lyrics(conn, &a, &t).map_err(db_err))
        .await?;
    let node = Node::new("lyrics")
        .attr("artist", artist)
        .attr("title", title);
    Ok(Node::new("payload").object(match found {
        Some(text) => node.text(text),
        None => node,
    }))
}

/// Builds `structuredLyrics` lines from BoogieBox's `[{time, text}]` synced lyrics or plain text.
pub fn structured_lines(plain: &str, synced: Option<&Json>) -> (bool, Vec<Node>) {
    if let Some(Json::Array(lines)) = synced {
        let nodes: Vec<Node> = lines
            .iter()
            .filter_map(|line| {
                let time = line.get("time").and_then(Json::as_f64)?;
                let text = line.get("text").and_then(Json::as_str).unwrap_or("");
                Some(
                    Node::new("line")
                        .attr("start", (time.max(0.0) * 1000.0).round() as i64)
                        .text(text),
                )
            })
            .collect();
        if !nodes.is_empty() {
            return (true, nodes);
        }
    }
    let nodes = plain
        .replace("\r\n", "\n")
        .split('\n')
        .map(|l| Node::new("line").text(l))
        .collect();
    (false, nodes)
}

/// `getLyricsBySongId` (OpenSubsonic `songLyrics`): cache, then LRCLIB / lyrics.ovh within a short budget.
pub async fn lyrics_by_song_id(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let id = required_id(&ctx.params, "id")?;
    let lookup_id = id.clone();
    let song = ctx
        .db(move |conn| sdb::get_song(conn, &lookup_id).map_err(db_err))
        .await?
        .ok_or_else(|| SubsonicError::not_found("Song"))?;
    let http = ctx.http_client();
    let found = tokio::time::timeout(
        LYRICS_BUDGET,
        crate::routes::playback_routes::resolve_track_lyrics(&ctx.db, &http, &id),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .flatten();
    let list = match found {
        Some(lyrics) => {
            let (synced, lines) = structured_lines(&lyrics.lyrics, lyrics.synced.as_ref());
            vec![Node::new("structuredLyrics")
                .opt("displayArtist", song.artist.clone())
                .attr("displayTitle", song.title.clone())
                .attr("lang", "xxx")
                .attr("synced", synced)
                .list("line", lines)]
        }
        None => Vec::new(),
    };
    Ok(Node::new("payload").object(Node::new("lyricsList").list("structuredLyrics", list)))
}

// ── Artist and album info ──────────────────────────────────────────────────────

/// `getArtistInfo` / `getArtistInfo2`: local bio (else the cached Last.fm summary) and similar artists in
/// this library.
pub async fn artist_info(ctx: &Ctx, legacy: bool) -> Result<Node, SubsonicError> {
    let id = required_id(&ctx.params, "id")?;
    let count = ctx.params.clamped("count", 20, 0, 50)? as usize;
    let lookup_id = id.clone();
    let (description, mbid, lastfm) = ctx
        .db(move |conn| {
            let info = sdb::artist_info(conn, &lookup_id)
                .map_err(db_err)?
                .ok_or_else(|| SubsonicError::not_found("Artist"))?;
            let lastfm = cached_lastfm(conn, &format!("artist:{}", info.name.to_lowercase()));
            Ok((info.description, info.musicbrainz_id, lastfm))
        })
        .await?;

    let similar = if count == 0 {
        Vec::new()
    } else {
        let entity = coerce_entity_id(&id);
        let related = crate::similar_artists::gather_related_candidates(
            &ctx.db,
            &ctx.http_client(),
            &entity,
            Some(SIMILAR_BUDGET),
        )
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
        let user = ctx.user_id();
        ctx.db(move |conn| {
            crate::similar_artists::resolve_local_similar_artists_with_listenbrainz(
                conn,
                &user,
                &entity,
                &related.lastfm,
                &related.deezer,
                &related.listenbrainz,
                count,
            )
            .map_err(db_err)
        })
        .await?
    };
    let artists: Vec<SubsonicArtist> = similar
        .into_iter()
        .map(|s| SubsonicArtist {
            id: s.id.to_string(),
            name: s.name,
            album_count: s.album_count,
        })
        .collect();
    let user = ctx.user_id();
    let ids: Vec<String> = artists.iter().map(|a| a.id.clone()).collect();
    let ann = ctx
        .db(move |conn| sdb::annotations(conn, &user, ItemKind::Artist, &ids).map_err(db_err))
        .await?;

    let biography = description.or_else(|| lastfm.as_ref().and_then(|v| json_str(v, "summary")));
    let lastfm_url = lastfm.as_ref().and_then(|v| json_str(v, "url"));
    let articles = ctx.articles();
    let mut node = Node::new(if legacy { "artistInfo" } else { "artistInfo2" });
    if let Some(bio) = biography {
        node = node.object(text_node("biography", bio));
    }
    if let Some(mbid) = mbid {
        node = node.object(text_node("musicBrainzId", mbid));
    }
    if let Some(url) = lastfm_url {
        node = node.object(text_node("lastFmUrl", url));
    }
    let similar_nodes = artists
        .iter()
        .map(|a| {
            if legacy {
                artist_legacy_node(a, &ann)
            } else {
                artist_id3_node(a, &ann, &articles)
            }
        })
        .collect();
    Ok(Node::new("payload").object(node.list("similarArtist", similar_nodes)))
}

/// `getAlbumInfo` / `getAlbumInfo2`: local notes, else the cached Last.fm summary.
pub async fn album_info(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let id = required_id(&ctx.params, "id")?;
    let (description, lastfm) = ctx
        .db(move |conn| {
            let info = sdb::album_info(conn, &id)
                .map_err(db_err)?
                .ok_or_else(|| SubsonicError::not_found("Album"))?;
            let key = format!(
                "album:{}:{}",
                info.artist.to_lowercase(),
                info.title.to_lowercase()
            );
            Ok((info.description, cached_lastfm(conn, &key)))
        })
        .await?;
    let notes = description.or_else(|| lastfm.as_ref().and_then(|v| json_str(v, "summary")));
    let mut node = Node::new("albumInfo");
    if let Some(notes) = notes {
        node = node.object(text_node("notes", notes));
    }
    if let Some(url) = lastfm.as_ref().and_then(|v| json_str(v, "url")) {
        node = node.object(text_node("lastFmUrl", url));
    }
    Ok(Node::new("payload").object(node))
}

// ── Similar and top songs ──────────────────────────────────────────────────────

/// `getSimilarSongs` / `getSimilarSongs2` ("Instant Mix"): an Artist Radio queue seeded by the song's, album's
/// or artist's artist.
pub async fn similar_songs(ctx: &Ctx, legacy: bool) -> Result<Node, SubsonicError> {
    let id = required_id(&ctx.params, "id")?;
    let count = ctx.params.clamped("count", 50, 1, MAX_SIMILAR)?;
    let seed = ctx
        .db(move |conn| {
            let kind = sdb::resolve_item_kind(conn, &id)
                .map_err(db_err)?
                .ok_or_else(|| SubsonicError::not_found("Item"))?;
            Ok(match kind {
                ItemKind::Track => sdb::get_song(conn, &id)
                    .map_err(db_err)?
                    .and_then(|s| s.artist_id),
                ItemKind::Album => sdb::get_album(conn, &id)
                    .map_err(db_err)?
                    .and_then(|a| a.artist_id),
                ItemKind::Artist => Some(id),
            })
        })
        .await?;
    let container = if legacy {
        "similarSongs"
    } else {
        "similarSongs2"
    };
    let Some(seed) = seed else {
        return Ok(Node::new("payload").object(Node::new(container)));
    };
    let options = RadioOptions {
        limit: count.max(10) as usize,
        focus: Focus::Similar,
        moods: Vec::new(),
        variety: DEFAULT_VARIETY,
    };
    let built = build_radio(
        &ctx.db,
        &ctx.http_client(),
        &ctx.user.id,
        &coerce_entity_id(&seed),
        &options,
        SIMILAR_BUDGET,
    )
    .await
    .map_err(|err| {
        tracing::warn!(error = %err, "Subsonic similar songs: radio build failed");
        SubsonicError::generic("Could not build similar songs")
    })?;
    let ids: Vec<String> = built
        .map(|b| {
            b.plan
                .tracks
                .iter()
                .take(count as usize)
                .map(|p| p.track.track.id.to_string())
                .collect()
        })
        .unwrap_or_default();
    let user = ctx.user_id();
    let (songs, ann) = ctx
        .db(move |conn| {
            let songs = sdb::songs_by_ids(conn, &ids).map_err(db_err)?;
            let ann = song_annotations(conn, &user, &songs)?;
            Ok((songs, ann))
        })
        .await?;
    Ok(Node::new("payload").object(Node::new(container).list("song", songs_nodes(&songs, &ann))))
}

/// Orders an artist's songs: Last.fm's cached top tracks first (in their order), then by local plays.
pub fn rank_top_songs(
    mut songs: Vec<boogiebox_db::subsonic::SubsonicSong>,
    lastfm_top: &[String],
) -> Vec<boogiebox_db::subsonic::SubsonicSong> {
    let rank: HashMap<String, usize> = lastfm_top
        .iter()
        .enumerate()
        .map(|(i, name)| (name.trim().to_lowercase(), i))
        .collect();
    // Stable: songs without a Last.fm rank keep their play-count order.
    songs.sort_by_key(|s| {
        rank.get(&s.title.trim().to_lowercase())
            .copied()
            .unwrap_or(usize::MAX)
    });
    songs
}

/// `getTopSongs` (`artist` name).
pub async fn top_songs(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let artist = ctx.params.required("artist")?.to_owned();
    let count = ctx.params.clamped("count", 50, 1, 500)? as usize;
    let user = ctx.user_id();
    let (songs, ann) = ctx
        .db(move |conn| {
            let Some(artist_id) = sdb::artist_id_by_name(conn, &artist).map_err(db_err)? else {
                return Ok((Vec::new(), Default::default()));
            };
            let songs = sdb::artist_songs_by_plays(conn, &artist_id, 500).map_err(db_err)?;
            let lastfm_top: Vec<String> =
                cached_lastfm(conn, &format!("toptracks:{}", artist.to_lowercase()))
                    .and_then(|v| v.as_array().cloned())
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|t| json_str(t, "name"))
                    .collect();
            let mut songs = rank_top_songs(songs, &lastfm_top);
            songs.truncate(count);
            let ann = song_annotations(conn, &user, &songs)?;
            Ok((songs, ann))
        })
        .await?;
    Ok(Node::new("payload").object(Node::new("topSongs").list("song", songs_nodes(&songs, &ann))))
}

// ── Now playing ────────────────────────────────────────────────────────────────

/// `getNowPlaying`: what Subsonic apps reported in the last 10 minutes (all users, as in Subsonic).
pub async fn now_playing(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let entries = ctx.subsonic.now_playing();
    if entries.is_empty() {
        return Ok(Node::new("payload").object(Node::new("nowPlaying")));
    }
    let user = ctx.user_id();
    let lookups: Vec<(String, String)> = entries
        .iter()
        .map(|e| (e.user_id.clone(), e.track_id.clone()))
        .collect();
    let (songs, mixes, ann) = ctx
        .db(move |conn| {
            let track_ids: Vec<String> = lookups
                .iter()
                .filter(|(_, id)| model::mix_id(id).is_none())
                .map(|(_, id)| id.clone())
                .collect();
            let songs = sdb::songs_by_ids(conn, &track_ids).map_err(db_err)?;
            let mut mixes = HashMap::new();
            for (owner, id) in &lookups {
                if let Some(mix_id) = model::mix_id(id) {
                    if let Some(mix) = sdb::get_user_mix(conn, owner, mix_id).map_err(db_err)? {
                        mixes.insert(id.clone(), mix);
                    }
                }
            }
            let ann = song_annotations(conn, &user, &songs)?;
            Ok((songs, mixes, ann))
        })
        .await?;
    let by_id: HashMap<&str, &boogiebox_db::subsonic::SubsonicSong> =
        songs.iter().map(|s| (s.id.as_str(), s)).collect();
    let nodes: Vec<Node> = entries
        .iter()
        .filter_map(|e| {
            let base = match mixes.get(&e.track_id) {
                Some(mix) => model::mix_song_node(mix),
                None => song_node(by_id.get(e.track_id.as_str())?, &ann),
            };
            Some(
                base.attr("username", e.username.as_str())
                    .attr("minutesAgo", (e.at.elapsed().as_secs() / 60) as i64)
                    .attr("playerId", 0)
                    .attr("playerName", e.client.as_str()),
            )
        })
        .collect();
    Ok(Node::new("payload").object(Node::new("nowPlaying").list("entry", nodes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use boogiebox_db::subsonic::SubsonicSong;

    #[test]
    fn synced_lyrics_become_millisecond_lines() {
        let synced = serde_json::json!([{ "time": 1.5, "text": "Hello" }, { "time": 3.25, "text": "" }, { "bad": 1 }]);
        let (is_synced, lines) = structured_lines("ignored", Some(&synced));
        assert!(is_synced);
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0].attrs[0].1,
            super::super::response::Scalar::Int(1500)
        );
        assert_eq!(
            lines[1].attrs[0].1,
            super::super::response::Scalar::Int(3250)
        );

        let (is_synced, lines) = structured_lines("one\r\ntwo", None);
        assert!(!is_synced);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].attrs.is_empty());
        let (is_synced, _) = structured_lines("x", Some(&serde_json::json!([])));
        assert!(!is_synced);
    }

    #[test]
    fn top_songs_put_lastfm_hits_first_then_plays() {
        let song = |id: &str, title: &str| SubsonicSong {
            id: id.into(),
            title: title.into(),
            ..Default::default()
        };
        // Input is already in local play-count order.
        let ranked = rank_top_songs(
            vec![
                song("a", "Deep Cut"),
                song("b", "Hit Two"),
                song("c", "Other"),
                song("d", "hit one "),
            ],
            &["Hit One".into(), "Hit Two".into(), "Not Local".into()],
        );
        assert_eq!(
            ranked.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["d", "b", "a", "c"]
        );
    }
}
