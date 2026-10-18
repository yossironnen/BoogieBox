//! Subsonic playlists: the user's own playlists (read and edit) plus the read-only "BoogieMix" playlist that
//! lists the user's rendered mixes as long tracks (plan §15.1a).

use boogiebox_db::{
    music::coerce_entity_id,
    playlists::{self as pdb, PlaylistRow, UpdatePlaylistError},
    subsonic::{self as sdb, SubsonicMix},
};

use super::{
    handlers::{db_err, required_id, song_annotations, songs_nodes, valid_id, Ctx},
    model::{self, BOOGIEMIX_PLAYLIST_ID},
    response::{Node, SubsonicError},
};

/// Longest playlist name accepted.
const MAX_NAME_CHARS: usize = 200;
/// Longest playlist comment accepted.
const MAX_COMMENT_CHARS: usize = 2000;
/// Most songs set or added in one request.
const MAX_PLAYLIST_IDS: usize = 2000;

fn playlist_node(row: &PlaylistRow, owner: &str) -> Node {
    Node::new("playlist")
        .attr("id", row.id.to_string())
        .attr("name", row.name.as_str())
        .opt(
            "comment",
            row.description.clone().filter(|d| !d.trim().is_empty()),
        )
        .attr("owner", owner)
        .attr("public", false)
        .attr("songCount", row.track_count)
        .attr(
            "duration",
            row.total_duration.unwrap_or(0.0).max(0.0).round() as i64,
        )
        .attr(
            "created",
            model::iso(row.created_at.as_deref().unwrap_or("1970-01-01 00:00:00")),
        )
        .attr(
            "changed",
            model::iso(
                row.updated_at
                    .as_deref()
                    .or(row.created_at.as_deref())
                    .unwrap_or("1970-01-01 00:00:00"),
            ),
        )
        .opt(
            "coverArt",
            row.art_album_ids
                .first()
                .map(|id| model::album_cover_id(id)),
        )
}

/// The virtual "BoogieMix" playlist (newest mix first).
fn boogiemix_playlist_node(mixes: &[SubsonicMix], owner: &str) -> Node {
    let newest = mixes
        .first()
        .and_then(|m| m.created.as_deref())
        .unwrap_or("1970-01-01 00:00:00");
    let oldest = mixes
        .last()
        .and_then(|m| m.created.as_deref())
        .unwrap_or(newest);
    Node::new("playlist")
        .attr("id", BOOGIEMIX_PLAYLIST_ID)
        .attr("name", "BoogieMix")
        .attr("comment", "Your BoogieMix mixes")
        .attr("owner", owner)
        .attr("public", false)
        .attr("readonly", true)
        .attr("songCount", mixes.len())
        .attr(
            "duration",
            mixes
                .iter()
                .map(|m| m.duration.max(0.0))
                .sum::<f64>()
                .round() as i64,
        )
        .attr("created", model::iso(oldest))
        .attr("changed", model::iso(newest))
        .opt(
            "coverArt",
            mixes
                .iter()
                .find_map(|m| m.cover_album_id.as_deref())
                .map(model::album_cover_id),
        )
}

fn read_only_error() -> SubsonicError {
    SubsonicError::new(50, "The BoogieMix playlist is read-only")
}

fn bounded_ids(values: Vec<&str>) -> Result<Vec<String>, SubsonicError> {
    if values.len() > MAX_PLAYLIST_IDS {
        return Err(SubsonicError::generic("Too many songs in one request"));
    }
    values.into_iter().map(valid_id).collect()
}

fn checked_name(raw: &str) -> Result<String, SubsonicError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(SubsonicError::missing("name"));
    }
    if name.chars().count() > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        return Err(SubsonicError::generic(
            "Playlist names must be at most 200 characters with no control characters",
        ));
    }
    Ok(name.to_owned())
}

/// `getPlaylists`: own playlists (an admin may name another user), then "BoogieMix" when there are mixes.
pub async fn playlists(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let requested = ctx.params.text("username").map(str::to_owned);
    if let Some(name) = &requested {
        if !name.eq_ignore_ascii_case(&ctx.user.username) && !ctx.user.is_admin() {
            return Err(SubsonicError::not_authorized());
        }
    }
    let me = ctx.user.clone();
    let (rows, mixes, owner) = ctx
        .db(move |conn| {
            let (owner_id, owner_name) = match requested {
                Some(name) if !name.eq_ignore_ascii_case(&me.username) => {
                    let account = sdb::get_account_by_username(conn, &name)
                        .map_err(db_err)?
                        .ok_or_else(|| SubsonicError::not_found("User"))?;
                    (account.user_id, account.username)
                }
                _ => (me.id.clone(), me.username.clone()),
            };
            let rows = pdb::list_playlists(conn, &owner_id).map_err(db_err)?;
            let mixes = sdb::list_user_mixes(conn, &owner_id).map_err(db_err)?;
            Ok((rows, mixes, owner_name))
        })
        .await?;
    let mut nodes: Vec<Node> = rows.iter().map(|r| playlist_node(r, &owner)).collect();
    if !mixes.is_empty() {
        nodes.push(boogiemix_playlist_node(&mixes, &owner));
    }
    Ok(Node::new("payload").object(Node::new("playlists").list("playlist", nodes)))
}

/// One playlist with its entries.
async fn load_playlist(ctx: &Ctx, id: String) -> Result<Node, SubsonicError> {
    let user = ctx.user_id();
    if id == BOOGIEMIX_PLAYLIST_ID {
        let mixes = ctx
            .db(move |conn| sdb::list_user_mixes(conn, &user).map_err(db_err))
            .await?;
        return Ok(boogiemix_playlist_node(&mixes, &ctx.user.username)
            .list("entry", mixes.iter().map(model::mix_song_node).collect()));
    }
    let (row, songs, ann) = ctx
        .db(move |conn| {
            let row = pdb::get_playlist(conn, &coerce_entity_id(&id), &user)
                .map_err(db_err)?
                .ok_or_else(|| SubsonicError::not_found("Playlist"))?;
            let songs = sdb::playlist_songs(conn, &id, &user)
                .map_err(db_err)?
                .ok_or_else(|| SubsonicError::not_found("Playlist"))?;
            let ann = song_annotations(conn, &user, &songs)?;
            Ok((row, songs, ann))
        })
        .await?;
    Ok(playlist_node(&row, &ctx.user.username).list("entry", songs_nodes(&songs, &ann)))
}

/// `getPlaylist`.
pub async fn playlist(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let id = required_id(&ctx.params, "id")?;
    Ok(Node::new("payload").object(load_playlist(ctx, id).await?))
}

/// `createPlaylist`: `name` + `songId…` creates (a taken name gets " (2)" …); `playlistId` + `songId…`
/// replaces the entries. Returns the playlist (OpenSubsonic).
pub async fn create_playlist(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let song_ids = bounded_ids(ctx.params.all("songId"))?;
    let user = ctx.user_id();
    let id = match ctx.params.text("playlistId") {
        Some(raw) => {
            let id = valid_id(raw)?;
            if id == BOOGIEMIX_PLAYLIST_ID {
                return Err(read_only_error());
            }
            let target = id.clone();
            let replaced = ctx
                .db(move |conn| {
                    sdb::set_playlist_tracks(conn, &target, &user, &song_ids).map_err(db_err)
                })
                .await?;
            if !replaced {
                return Err(SubsonicError::not_found("Playlist"));
            }
            id
        }
        None => {
            let name = checked_name(ctx.params.get("name").unwrap_or(""))?;
            ctx.db(move |conn| {
                let name = sdb::unique_playlist_name(conn, &user, &name).map_err(db_err)?;
                let row =
                    pdb::create_playlist(conn, &name, "", &user).map_err(|err| match err {
                        pdb::CreatePlaylistError::NameTaken => {
                            SubsonicError::generic("A playlist with that name already exists")
                        }
                        pdb::CreatePlaylistError::Db(e) => db_err(e),
                    })?;
                let id = row.id.to_string();
                sdb::set_playlist_tracks(conn, &id, &user, &song_ids).map_err(db_err)?;
                Ok(id)
            })
            .await?
        }
    };
    Ok(Node::new("payload").object(load_playlist(ctx, id).await?))
}

/// `updatePlaylist`: `name`, `comment`, `songIndexToRemove…` (0-based, applied first) and `songIdToAdd…`.
/// `public` is accepted and ignored (BoogieBox playlists are private).
pub async fn update_playlist(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let id = required_id(&ctx.params, "playlistId")?;
    if id == BOOGIEMIX_PLAYLIST_ID {
        return Err(read_only_error());
    }
    let name = match ctx.params.get("name") {
        Some(raw) => Some(checked_name(raw)?),
        None => None,
    };
    let comment = ctx.params.get("comment").map(|c| c.trim().to_owned());
    if comment
        .as_deref()
        .is_some_and(|c| c.chars().count() > MAX_COMMENT_CHARS)
    {
        return Err(SubsonicError::generic(
            "Playlist comments must be at most 2000 characters",
        ));
    }
    let add = bounded_ids(ctx.params.all("songIdToAdd"))?;
    let remove: Vec<usize> = ctx
        .params
        .all("songIndexToRemove")
        .into_iter()
        .map(|i| {
            i.parse::<usize>().map_err(|_| {
                SubsonicError::generic("Invalid value for parameter: songIndexToRemove")
            })
        })
        .collect::<Result<_, _>>()?;
    let user = ctx.user_id();
    ctx.db(move |conn| {
        let entity = coerce_entity_id(&id);
        let row = pdb::get_playlist(conn, &entity, &user)
            .map_err(db_err)?
            .ok_or_else(|| SubsonicError::not_found("Playlist"))?;
        if name.is_some() || comment.is_some() {
            let new_name = name.unwrap_or_else(|| row.name.clone());
            let new_comment =
                comment.unwrap_or_else(|| row.description.clone().unwrap_or_default());
            match pdb::update_playlist(conn, &entity, &new_name, &new_comment, None, &user) {
                Ok(Some(_)) => {}
                Ok(None) | Err(UpdatePlaylistError::NotFound) => {
                    return Err(SubsonicError::not_found("Playlist"))
                }
                Err(UpdatePlaylistError::NameTaken) => {
                    return Err(SubsonicError::generic(
                        "A playlist with that name already exists",
                    ))
                }
                Err(UpdatePlaylistError::Db(e)) => return Err(db_err(e)),
            }
        }
        if !remove.is_empty() || !add.is_empty() {
            sdb::edit_playlist_entries(conn, &id, &user, &remove, &add).map_err(db_err)?;
        }
        Ok(())
    })
    .await?;
    Ok(Node::new("payload"))
}

/// `deletePlaylist`.
pub async fn delete_playlist(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let id = required_id(&ctx.params, "id")?;
    if id == BOOGIEMIX_PLAYLIST_ID {
        return Err(read_only_error());
    }
    let user = ctx.user_id();
    ctx.db(move |conn| {
        let entity = coerce_entity_id(&id);
        if pdb::get_playlist(conn, &entity, &user)
            .map_err(db_err)?
            .is_none()
        {
            return Err(SubsonicError::not_found("Playlist"));
        }
        pdb::delete_playlist(conn, &entity, &user).map_err(db_err)
    })
    .await?;
    Ok(Node::new("payload"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(checked_name("  Road trip ").unwrap(), "Road trip");
        assert_eq!(checked_name("   ").unwrap_err().code, 10);
        assert_eq!(checked_name(&"x".repeat(201)).unwrap_err().code, 0);
        assert_eq!(checked_name("bad\u{1}name").unwrap_err().code, 0);
    }

    #[test]
    fn boogiemix_playlist_summarizes_mixes() {
        let mixes = vec![
            SubsonicMix {
                id: "m2".into(),
                duration: 60.4,
                created: Some("2026-02-01 00:00:00".into()),
                ..Default::default()
            },
            SubsonicMix {
                id: "m1".into(),
                duration: 3600.0,
                cover_album_id: Some("al9".into()),
                created: Some("2026-01-01 00:00:00".into()),
                ..Default::default()
            },
        ];
        let node = boogiemix_playlist_node(&mixes, "alice");
        let get = |k: &str| {
            node.attrs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.clone())
        };
        use super::super::response::Scalar;
        assert_eq!(get("id"), Some(Scalar::Str(BOOGIEMIX_PLAYLIST_ID.into())));
        assert_eq!(get("readonly"), Some(Scalar::Bool(true)));
        assert_eq!(get("songCount"), Some(Scalar::Int(2)));
        assert_eq!(get("duration"), Some(Scalar::Int(3660)));
        assert_eq!(get("coverArt"), Some(Scalar::Str("al-al9".into())));
        assert_eq!(
            get("created"),
            Some(Scalar::Str("2026-01-01T00:00:00Z".into()))
        );
        assert_eq!(
            get("changed"),
            Some(Scalar::Str("2026-02-01T00:00:00Z".into()))
        );
    }
}
