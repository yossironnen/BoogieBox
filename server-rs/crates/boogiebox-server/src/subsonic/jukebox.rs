//! `jukeboxControl`: Subsonic's jukebox drives one Play On speaker (`subsonicJukeboxRenderer`) as the requesting
//! user. It is an ordinary Play On session, so takeover protection, the rate limit and the web UI's Play On
//! view all apply. See wip/opensubsonic-api-implementation-plan.md §15.5 (2.5-7).

use std::collections::HashMap;

use boogiebox_db::subsonic::{self as sdb, ItemKind};
use serde_json::Value;

use super::{
    handlers::{db_err, valid_id, Ctx, MAX_IDS},
    model,
    response::{Node, SubsonicError},
};
use crate::{
    remote::{
        queue::{EnqueueMode, QueueItem},
        session::{Command, Controller, SessionError, SessionState, SessionStatus},
        RemoteManager,
    },
    routes::remote_session_routes::{resolve_items, ItemRef},
};

fn unavailable() -> SubsonicError {
    SubsonicError::generic("The jukebox speaker is not available")
}

fn session_error(err: SessionError) -> SubsonicError {
    match err {
        SessionError::InUse(info) => SubsonicError::new(
            50,
            format!(
                "The jukebox speaker is in use by {}",
                info.owner_name.as_deref().unwrap_or("another user")
            ),
        ),
        SessionError::BadRequest(m) => SubsonicError::generic(m),
        SessionError::Unsupported(what) => {
            SubsonicError::generic(format!("The jukebox speaker does not support {what}"))
        }
        SessionError::Renderer(_) | SessionError::Ended => unavailable(),
    }
}

/// True when the jukebox can be used: a speaker is chosen and Play On is on (blocking: reads settings).
pub fn jukebox_enabled(conn: &rusqlite::Connection, renderer: Option<&str>) -> bool {
    renderer.is_some() && crate::remote::read_remote_settings(conn).enabled
}

/// Position now, interpolating from the last sample while playing.
fn position_now(state: &SessionState) -> i64 {
    let mut pos = state.position_sec;
    if state.status == SessionStatus::Playing {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(state.sampled_at_ms);
        pos += now_ms.saturating_sub(state.sampled_at_ms) as f64 / 1000.0;
    }
    if let Some(d) = state.duration_sec {
        pos = pos.min(d);
    }
    pos.max(0.0) as i64
}

/// `jukeboxStatus` / `jukeboxPlaylist` attributes.
pub fn status_node(name: &'static str, state: Option<&SessionState>) -> Node {
    let Some(s) = state else {
        return Node::new(name)
            .attr("currentIndex", -1i64)
            .attr("playing", false)
            .attr("gain", 1.0f64)
            .attr("position", 0i64);
    };
    let index = s
        .current_qid
        .and_then(|q| s.queue.iter().position(|e| e.qid == q))
        .map(|i| i as i64)
        .unwrap_or(-1);
    Node::new(name)
        .attr("currentIndex", index)
        .attr(
            "playing",
            matches!(s.status, SessionStatus::Playing | SessionStatus::Loading),
        )
        .attr(
            "gain",
            s.volume.map(|v| f64::from(v) / 100.0).unwrap_or(1.0),
        )
        .attr("position", position_now(s))
}

fn refs(ids: &[String]) -> Vec<ItemRef> {
    ids.iter()
        .map(|id| match model::mix_id(id) {
            Some(mix) => ItemRef {
                kind: "mix".into(),
                id: Value::String(mix.to_owned()),
                radio_reason: None,
            },
            None => ItemRef {
                kind: "track".into(),
                id: Value::String(id.clone()),
                radio_reason: None,
            },
        })
        .collect()
}

fn song_ids(ctx: &Ctx) -> Result<Vec<String>, SubsonicError> {
    let ids = ctx.params.all("id");
    if ids.len() > MAX_IDS {
        return Err(SubsonicError::generic("Too many ids"));
    }
    ids.into_iter().map(valid_id).collect()
}

fn index_param(ctx: &Ctx) -> Result<usize, SubsonicError> {
    let index = ctx
        .params
        .int("index")?
        .ok_or_else(|| SubsonicError::missing("index"))?;
    usize::try_from(index).map_err(|_| SubsonicError::generic("Invalid index"))
}

fn qid_at(state: Option<&SessionState>, index: usize) -> Result<u64, SubsonicError> {
    state
        .and_then(|s| s.queue.get(index))
        .map(|e| e.qid)
        .ok_or_else(|| SubsonicError::generic("Invalid index"))
}

async fn items(ctx: &Ctx, ids: &[String]) -> Result<Vec<QueueItem>, SubsonicError> {
    resolve_items(&ctx.db, refs(ids), &ctx.user.id)
        .await
        .map_err(|_| SubsonicError::not_found("Song"))
}

/// The jukebox playlist as `entry` songs, in play order.
async fn playlist_node(ctx: &Ctx, state: Option<&SessionState>) -> Result<Node, SubsonicError> {
    let node = status_node("jukeboxPlaylist", state);
    let Some(state) = state else {
        return Ok(node.list("entry", Vec::new()));
    };
    let entries: Vec<QueueItem> = state.queue.iter().map(|e| e.item.clone()).collect();
    let user = ctx.user_id();
    let (songs, mixes, ann) = ctx
        .db(move |conn| {
            let track_ids: Vec<String> = entries
                .iter()
                .filter_map(|i| match i {
                    QueueItem::Track { id, .. } => Some(id.clone()),
                    QueueItem::Mix { .. } => None,
                })
                .collect();
            let songs = sdb::songs_by_ids(conn, &track_ids).map_err(db_err)?;
            let ann = sdb::annotations(conn, &user, ItemKind::Track, &track_ids).map_err(db_err)?;
            let mut mixes = HashMap::new();
            for item in &entries {
                if let QueueItem::Mix { id, .. } = item {
                    if let Some(mix) = sdb::get_user_mix(conn, &user, id).map_err(db_err)? {
                        mixes.insert(id.clone(), mix);
                    }
                }
            }
            Ok((songs, mixes, ann))
        })
        .await?;
    let by_id: HashMap<&str, &sdb::SubsonicSong> =
        songs.iter().map(|s| (s.id.as_str(), s)).collect();
    let nodes = state
        .queue
        .iter()
        .filter_map(|e| match &e.item {
            QueueItem::Track { id, .. } => {
                by_id.get(id.as_str()).map(|s| model::song_node(s, &ann))
            }
            QueueItem::Mix { id, .. } => mixes.get(id).map(model::mix_song_node),
        })
        .collect();
    Ok(node.list("entry", nodes))
}

/// `jukeboxControl` (`action` = get, status, set, start, stop, skip, add, clear, remove, shuffle, setGain).
pub async fn control(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let renderer = ctx
        .settings
        .jukebox_renderer
        .clone()
        .ok_or_else(unavailable)?;
    let check = renderer.clone();
    let enabled = ctx
        .db(move |conn| Ok(jukebox_enabled(conn, Some(&check))))
        .await?;
    let remote: RemoteManager = ctx
        .state
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .remote
        .clone();
    if !enabled || !remote.registry().is_online(&renderer) {
        return Err(unavailable());
    }
    let action = ctx.params.required("action")?.to_owned();
    let current = remote.session(&renderer).map(|h| h.snapshot());
    if matches!(action.as_str(), "get" | "status") {
        return Ok(Node::new("payload").object(if action == "get" {
            playlist_node(ctx, current.as_ref()).await?
        } else {
            status_node("jukeboxStatus", current.as_ref())
        }));
    }
    if !remote.allow_request(&ctx.user.id) {
        return Err(SubsonicError::generic("Too many requests"));
    }
    let by = Controller {
        user_id: ctx.user.id.clone(),
        username: ctx.user.username.clone(),
    };
    let send = |cmd: Command| {
        let (remote, db, udn, by) = (remote.clone(), ctx.db.clone(), renderer.clone(), by.clone());
        async move {
            remote
                .command(&db, &udn, cmd, by, false)
                .await
                .map_err(session_error)
        }
    };
    let state = match action.as_str() {
        "start" => send(Command::Play).await?,
        "stop" => send(Command::Pause).await?,
        "clear" => send(Command::Clear).await?,
        "shuffle" => send(Command::SetShuffle(true)).await?,
        "add" => {
            let ids = song_ids(ctx)?;
            if ids.is_empty() {
                return Err(SubsonicError::missing("id"));
            }
            let items = items(ctx, &ids).await?;
            send(Command::Enqueue {
                items,
                mode: EnqueueMode::End,
            })
            .await?
        }
        "set" => {
            let ids = song_ids(ctx)?;
            let was_playing = current.as_ref().is_some_and(|s| {
                matches!(s.status, SessionStatus::Playing | SessionStatus::Loading)
            });
            let items = if ids.is_empty() {
                Vec::new()
            } else {
                items(ctx, &ids).await?
            };
            let mut state = send(Command::Clear).await?;
            if !items.is_empty() {
                state = send(Command::Enqueue {
                    items,
                    mode: EnqueueMode::End,
                })
                .await?;
                if was_playing {
                    state = send(Command::Play).await?;
                }
            }
            state
        }
        "skip" => {
            let qid = qid_at(current.as_ref(), index_param(ctx)?)?;
            let mut state = send(Command::Jump(qid)).await?;
            let offset = ctx.params.int("offset")?.unwrap_or(0);
            if offset > 0 {
                state = send(Command::Seek(offset as f64)).await?;
            }
            state
        }
        "remove" => {
            let qid = qid_at(current.as_ref(), index_param(ctx)?)?;
            send(Command::Remove(qid)).await?
        }
        "setGain" => {
            let gain: f64 = ctx
                .params
                .required("gain")?
                .trim()
                .parse()
                .ok()
                .filter(|g: &f64| g.is_finite())
                .ok_or_else(|| SubsonicError::generic("Invalid gain"))?;
            send(Command::SetVolume(
                (gain.clamp(0.0, 1.0) * 100.0).round() as u8
            ))
            .await?
        }
        other => {
            let shown: String = other.chars().take(32).collect();
            return Err(SubsonicError::generic(format!(
                "Unknown jukebox action: {shown}"
            )));
        }
    };
    Ok(Node::new("payload").object(status_node("jukeboxStatus", Some(&state))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::queue::QueueEntry;

    fn attr<'a>(node: &'a Node, name: &str) -> Option<&'a super::super::response::Scalar> {
        node.attrs.iter().find(|(k, _)| *k == name).map(|(_, v)| v)
    }

    #[test]
    fn status_maps_session_state() {
        use super::super::response::Scalar::*;
        let empty = status_node("jukeboxStatus", None);
        assert_eq!(attr(&empty, "currentIndex"), Some(&Int(-1)));
        assert_eq!(attr(&empty, "playing"), Some(&Bool(false)));

        let mut s = SessionState::new("udn", "Speaker", true, true);
        s.queue = vec![
            QueueEntry {
                qid: 7,
                item: QueueItem::Track {
                    id: "t1".into(),
                    title: "A".into(),
                    artist: None,
                    album: None,
                    album_id: None,
                    duration: Some(100.0),
                    radio_reason: None,
                },
            },
            QueueEntry {
                qid: 9,
                item: QueueItem::Mix {
                    id: "m1".into(),
                    title: "Mix".into(),
                    duration: None,
                },
            },
        ];
        s.current_qid = Some(9);
        s.status = SessionStatus::Paused;
        s.position_sec = 42.7;
        s.volume = Some(35);
        let node = status_node("jukeboxStatus", Some(&s));
        assert_eq!(attr(&node, "currentIndex"), Some(&Int(1)));
        assert_eq!(attr(&node, "playing"), Some(&Bool(false)));
        assert_eq!(attr(&node, "gain"), Some(&Float(0.35)));
        assert_eq!(attr(&node, "position"), Some(&Int(42)));
        assert_eq!(qid_at(Some(&s), 0).unwrap(), 7);
        assert!(qid_at(Some(&s), 2).is_err());
        assert!(qid_at(None, 0).is_err());
    }

    #[test]
    fn ids_become_track_or_mix_refs() {
        let r = refs(&["t1".into(), "mx-55".into()]);
        assert_eq!(r[0].kind, "track");
        assert_eq!(r[1].kind, "mix");
        assert_eq!(r[1].id, Value::String("55".into()));
    }

    #[test]
    fn session_errors_map_to_subsonic_errors() {
        let in_use = session_error(SessionError::InUse(Box::new(
            crate::remote::session::InUseInfo {
                udn: "u".into(),
                device_name: "Speaker".into(),
                owner_user_id: "u2".into(),
                owner_name: Some("Bob".into()),
                status: SessionStatus::Playing,
                now_playing: None,
            },
        )));
        assert_eq!(in_use.code, 50);
        assert!(in_use.message.contains("Bob"));
        assert_eq!(
            session_error(SessionError::Ended).message,
            unavailable().message
        );
        assert!(session_error(SessionError::Unsupported("volume"))
            .message
            .contains("volume"));
        assert_eq!(
            session_error(SessionError::BadRequest("queue is empty".into())).message,
            "queue is empty"
        );
    }
}
