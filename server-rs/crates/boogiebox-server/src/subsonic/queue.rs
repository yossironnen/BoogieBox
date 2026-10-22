//! Saved play queues: `getPlayQueue`/`savePlayQueue` and the index-based pair (OpenSubsonic
//! `indexBasedQueue`). The stored source of truth is the entry index, because a queue may repeat a song.
//! Apps save every few seconds while playing, so saves are one upsert; reads drop songs that no longer exist.

use std::collections::HashMap;

use boogiebox_db::subsonic::{self as sdb, SubsonicMix, SubsonicSong};

use super::{
    handlers::{db_err, song_annotations, valid_id, Ctx},
    model::{self, song_node},
    response::{Node, SubsonicError},
};

/// A queue entry that still resolves.
enum Entry {
    Song(Box<SubsonicSong>),
    Mix(SubsonicMix),
}

/// `savePlayQueue` (`current` = song id; its first occurrence) or `savePlayQueueByIndex` (`currentIndex`).
/// No `id` clears the queue.
pub async fn save(ctx: &Ctx, by_index: bool) -> Result<Node, SubsonicError> {
    let raw = ctx.params.all("id");
    if raw.len() > sdb::MAX_PLAY_QUEUE {
        return Err(SubsonicError::generic(
            "A play queue holds at most 1000 songs",
        ));
    }
    let ids: Vec<String> = raw.into_iter().map(valid_id).collect::<Result<_, _>>()?;
    let current_idx = if ids.is_empty() {
        None
    } else if by_index {
        match ctx.params.int("currentIndex")? {
            None => None,
            Some(i) if i >= 0 && (i as usize) < ids.len() => Some(i as usize),
            Some(_) => {
                return Err(SubsonicError::generic(
                    "currentIndex is outside the play queue",
                ))
            }
        }
    } else {
        ctx.params
            .text("current")
            .and_then(|current| ids.iter().position(|id| id == current))
    };
    let position = ctx.params.int("position")?.unwrap_or(0).max(0);
    let client = ctx.params.text("c").map(str::to_owned);
    let user = ctx.user_id();
    ctx.db(move |conn| {
        sdb::save_play_queue(conn, &user, &ids, current_idx, position, client.as_deref())
            .map_err(db_err)
    })
    .await?;
    Ok(Node::new("payload"))
}

/// `getPlayQueue` / `getPlayQueueByIndex`. No saved queue → an empty response.
pub async fn get(ctx: &Ctx, by_index: bool) -> Result<Node, SubsonicError> {
    let user = ctx.user_id();
    let loaded = ctx
        .db(move |conn| {
            let Some(queue) = sdb::get_play_queue(conn, &user).map_err(db_err)? else {
                return Ok(None);
            };
            let song_ids: Vec<String> = queue
                .track_ids
                .iter()
                .filter(|id| model::mix_id(id).is_none())
                .cloned()
                .collect();
            let songs = sdb::songs_by_id_map(conn, &song_ids).map_err(db_err)?;
            // Mixes are owner-only; another user's mix id simply drops out.
            let mut mixes: HashMap<String, Option<SubsonicMix>> = HashMap::new();
            let mut entries = Vec::new();
            let mut current = None;
            for (i, id) in queue.track_ids.iter().enumerate() {
                let entry = match model::mix_id(id) {
                    Some(mix_id) => {
                        if !mixes.contains_key(mix_id) {
                            let mix = sdb::get_user_mix(conn, &user, mix_id).map_err(db_err)?;
                            mixes.insert(mix_id.to_owned(), mix);
                        }
                        mixes[mix_id].clone().map(Entry::Mix)
                    }
                    None => songs.get(id).cloned().map(|s| Entry::Song(Box::new(s))),
                };
                if let Some(entry) = entry {
                    if queue.current_idx == Some(i) {
                        current = Some(entries.len());
                    }
                    entries.push((id.clone(), entry));
                }
            }
            let listed: Vec<SubsonicSong> = entries
                .iter()
                .filter_map(|(_, e)| match e {
                    Entry::Song(s) => Some(s.as_ref().clone()),
                    Entry::Mix(_) => None,
                })
                .collect();
            let ann = song_annotations(conn, &user, &listed)?;
            Ok(Some((queue, entries, current, ann)))
        })
        .await?;
    let Some((queue, entries, current, ann)) = loaded else {
        return Ok(Node::new("payload"));
    };
    // The saved position belongs to the current entry; without one it means nothing.
    let position = if current.is_some() {
        queue.position_ms
    } else {
        0
    };
    let node = Node::new(if by_index {
        "playQueueByIndex"
    } else {
        "playQueue"
    })
    .attr("position", position)
    .attr("username", ctx.user.username.as_str())
    .attr("changed", model::iso(&queue.changed_at))
    .attr("changedBy", queue.changed_by.unwrap_or_default());
    let node = if by_index {
        node.opt("currentIndex", current)
    } else {
        node.opt("current", current.map(|i| entries[i].0.clone()))
    };
    Ok(Node::new("payload").object(
        node.list(
            "entry",
            entries
                .iter()
                .map(|(_, e)| match e {
                    Entry::Song(s) => song_node(s, &ann),
                    Entry::Mix(m) => model::mix_song_node(m),
                })
                .collect(),
        ),
    ))
}
