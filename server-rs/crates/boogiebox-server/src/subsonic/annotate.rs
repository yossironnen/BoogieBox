//! Subsonic annotation: `scrobble` (plays and "now playing"), `star`/`unstar` and `setRating`.

use boogiebox_db::{
    music::coerce_entity_id,
    playlists as pdb,
    subsonic::{self as sdb, ItemKind},
};

use super::{
    handlers::{db_err, required_id, valid_id, Ctx, MAX_IDS},
    model,
    response::{Node, SubsonicError},
    state::NowPlayingEntry,
};

fn mix_not_annotatable() -> SubsonicError {
    SubsonicError::generic("BoogieMix mixes can't be starred or rated")
}

/// `scrobble`: `submission=true` (default) records finished plays at `time` (ms); `submission=false` marks
/// the song as now playing for `getNowPlaying`. Mixes (`mx-…`) are accepted and not counted.
pub async fn scrobble(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let ids = ctx.params.all("id");
    if ids.is_empty() {
        return Err(SubsonicError::missing("id"));
    }
    if ids.len() > MAX_IDS {
        return Err(SubsonicError::generic("Too many ids"));
    }
    let ids: Vec<String> = ids.into_iter().map(valid_id).collect::<Result<_, _>>()?;
    if !ctx.params.flag("submission", true) {
        if let Some(track_id) = ids.last() {
            ctx.subsonic.set_now_playing(NowPlayingEntry {
                user_id: ctx.user.id.clone(),
                username: ctx.user.username.clone(),
                client: ctx
                    .params
                    .text("c")
                    .unwrap_or("Subsonic app")
                    .chars()
                    .take(64)
                    .collect(),
                track_id: track_id.clone(),
                at: std::time::Instant::now(),
            });
        }
        return Ok(Node::new("payload"));
    }
    let times: Vec<Option<i64>> = ctx
        .params
        .all("time")
        .into_iter()
        .map(|t| t.parse::<i64>().ok().map(|ms| ms / 1000))
        .collect();
    let user = ctx.user_id();
    let missing = ctx
        .db(move |conn| {
            let mut missing = 0usize;
            for (i, id) in ids.iter().enumerate() {
                if model::mix_id(id).is_some() {
                    continue;
                }
                let at = times.get(i).copied().flatten();
                if sdb::record_scrobble(conn, &user, id, at).map_err(db_err)?
                    == sdb::ScrobbleOutcome::NotFound
                {
                    missing += 1;
                }
            }
            Ok(missing)
        })
        .await?;
    if missing > 0 {
        return Err(SubsonicError::not_found("Song"));
    }
    Ok(Node::new("payload"))
}

/// `star` / `unstar`: `id…` (song, album or artist — resolved in that order), `albumId…`, `artistId…`.
pub async fn star(ctx: &Ctx, star: bool) -> Result<Node, SubsonicError> {
    let p = &ctx.params;
    let (ids, album_ids, artist_ids) = (p.all("id"), p.all("albumId"), p.all("artistId"));
    if ids.is_empty() && album_ids.is_empty() && artist_ids.is_empty() {
        return Err(SubsonicError::missing("id"));
    }
    if ids.len() + album_ids.len() + artist_ids.len() > MAX_IDS {
        return Err(SubsonicError::generic("Too many ids"));
    }
    let check = |values: Vec<&str>| -> Result<Vec<String>, SubsonicError> {
        values
            .into_iter()
            .map(|v| {
                let id = valid_id(v)?;
                if model::mix_id(&id).is_some() {
                    return Err(mix_not_annotatable());
                }
                Ok(id)
            })
            .collect()
    };
    let (ids, album_ids, artist_ids) = (check(ids)?, check(album_ids)?, check(artist_ids)?);
    let user = ctx.user_id();
    ctx.db(move |conn| {
        let mut items = Vec::new();
        for id in ids {
            let kind = sdb::resolve_item_kind(conn, &id)
                .map_err(db_err)?
                .ok_or_else(|| SubsonicError::not_found("Item"))?;
            items.push((kind, id));
        }
        for (expected, list) in [(ItemKind::Album, album_ids), (ItemKind::Artist, artist_ids)] {
            for id in list {
                let found = match expected {
                    ItemKind::Album => sdb::get_album(conn, &id).map_err(db_err)?.is_some(),
                    _ => sdb::get_artist(conn, &id).map_err(db_err)?.is_some(),
                };
                if !found {
                    return Err(SubsonicError::not_found(if expected == ItemKind::Album {
                        "Album"
                    } else {
                        "Artist"
                    }));
                }
                items.push((expected, id));
            }
        }
        sdb::set_stars(conn, &user, &items, star).map_err(db_err)
    })
    .await?;
    Ok(Node::new("payload"))
}

/// `setRating`: whole stars 1–5, 0 removes. Applies to a song, album or artist (resolved in that order).
pub async fn set_rating(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let id = required_id(&ctx.params, "id")?;
    if model::mix_id(&id).is_some() {
        return Err(mix_not_annotatable());
    }
    let rating = ctx
        .params
        .int("rating")?
        .ok_or_else(|| SubsonicError::missing("rating"))?;
    if !(0..=5).contains(&rating) {
        return Err(SubsonicError::generic("Rating must be between 0 and 5"));
    }
    let value = (rating > 0).then_some(rating as f64);
    let user = ctx.user_id();
    ctx.db(move |conn| {
        let kind = sdb::resolve_item_kind(conn, &id)
            .map_err(db_err)?
            .ok_or_else(|| SubsonicError::not_found("Item"))?;
        let entity = coerce_entity_id(&id);
        let updated = match kind {
            ItemKind::Track => pdb::set_track_rating(conn, &entity, &user, value),
            ItemKind::Album => pdb::set_album_rating(conn, &entity, &user, value),
            ItemKind::Artist => pdb::set_artist_rating(conn, &entity, &user, value),
        }
        .map_err(db_err)?;
        updated
            .map(|_| ())
            .ok_or_else(|| SubsonicError::not_found("Item"))
    })
    .await?;
    Ok(Node::new("payload"))
}
