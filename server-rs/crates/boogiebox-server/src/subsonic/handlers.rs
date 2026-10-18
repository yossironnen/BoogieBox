//! Subsonic method handlers: browsing, lists and search here; playlists, annotation and info in sibling
//! modules. Each handler runs its queries in one blocking task (one short hold of the shared connection)
//! and builds the response tree after releasing it.

use axum::{http::HeaderMap, response::Response};
use boogiebox_db::subsonic::{self as sdb, Annotations, ItemKind, SubsonicSong};
use rusqlite::Connection;

use super::{
    annotate, info,
    model::{
        self, album_child_node, album_id3_node, artist_id3_node, artist_legacy_node, song_node,
    },
    params::Params,
    playlists,
    response::{Node, SubsonicError},
    state::{SubsonicSettings, SubsonicState, SubsonicUser},
};
use crate::{DbPool, SharedState};

/// Largest page any list endpoint returns.
pub const MAX_PAGE: i64 = 500;
/// Most ids accepted in one request (scrobble, star, …).
pub const MAX_IDS: usize = 500;

/// Methods in the Subsonic/OpenSubsonic spec that BoogieBox does not offer (yet). See plan §15.
const UNSUPPORTED: &[&str] = &[
    "getPlayQueue",
    "savePlayQueue",
    "getPlayQueueByIndex",
    "savePlayQueueByIndex",
    "getScanStatus",
    "startScan",
    "getBookmarks",
    "createBookmark",
    "deleteBookmark",
    "tokenInfo",
    "getUsers",
    "createUser",
    "updateUser",
    "deleteUser",
    "changePassword",
    "getAvatar",
    "getCaptions",
    "hls",
    "getVideos",
    "getVideoInfo",
    "jukeboxControl",
    "getShares",
    "createShare",
    "updateShare",
    "deleteShare",
    "getPodcasts",
    "getNewestPodcasts",
    "refreshPodcasts",
    "createPodcastChannel",
    "deletePodcastChannel",
    "deletePodcastEpisode",
    "downloadPodcastEpisode",
    "getPodcastEpisode",
    "getInternetRadioStations",
    "createInternetRadioStation",
    "updateInternetRadioStation",
    "deleteInternetRadioStation",
    "getChatMessages",
    "addChatMessage",
    "search",
];

/// OpenSubsonic extensions implemented by this build.
const EXTENSIONS: &[(&str, &[i64])] = &[("formPost", &[1]), ("songLyrics", &[1])];

/// Per-request context.
pub struct Ctx {
    pub state: SharedState,
    pub db: DbPool,
    pub subsonic: SubsonicState,
    pub user: SubsonicUser,
    pub params: Params,
    pub headers: HeaderMap,
    pub head: bool,
    pub settings: SubsonicSettings,
}

/// What a handler produced.
pub enum Reply {
    Payload(Node),
    Raw(Response),
}

impl Ctx {
    /// Runs `f` against the database in one blocking task.
    pub async fn db<T, F>(&self, f: F) -> Result<T, SubsonicError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, SubsonicError> + Send + 'static,
    {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|p| p.into_inner());
            f(&conn)
        })
        .await
        .map_err(|_| SubsonicError::generic("Internal error"))?
    }

    pub fn user_id(&self) -> String {
        self.user.id.clone()
    }

    /// Shared HTTP client (lyrics and similar-artist lookups).
    pub fn http_client(&self) -> reqwest::Client {
        self.state
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .http_client
            .clone()
    }

    pub fn articles(&self) -> Vec<String> {
        model::article_list(&self.settings.ignored_articles)
    }
}

/// Maps a database error to a generic Subsonic error without leaking details.
pub fn db_err(err: rusqlite::Error) -> SubsonicError {
    tracing::error!(error = %err, "Subsonic query failed");
    SubsonicError::generic("Database error")
}

/// Validates an id parameter: at most 128 characters of `[A-Za-z0-9_-]`.
pub fn valid_id(raw: &str) -> Result<String, SubsonicError> {
    if !raw.is_empty()
        && raw.len() <= 128
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Ok(raw.to_owned())
    } else {
        Err(SubsonicError::not_found("Item"))
    }
}

pub fn required_id(params: &Params, key: &str) -> Result<String, SubsonicError> {
    valid_id(params.required(key)?)
}

/// `musicFolderId` → library id. Folder ids are 1-based positions; a raw library id is accepted too.
pub fn resolve_folder(
    conn: &Connection,
    raw: Option<&str>,
) -> Result<Option<String>, SubsonicError> {
    let Some(raw) = raw else { return Ok(None) };
    let folders = sdb::music_folders(conn).map_err(db_err)?;
    if let Ok(n) = raw.parse::<usize>() {
        if n >= 1 && n <= folders.len() {
            return Ok(Some(folders[n - 1].0.clone()));
        }
    }
    folders
        .into_iter()
        .find(|(id, _)| id == raw)
        .map(|(id, _)| Some(id))
        .ok_or_else(|| SubsonicError::not_found("Music folder"))
}

pub fn song_annotations(
    conn: &Connection,
    user: &str,
    songs: &[SubsonicSong],
) -> Result<Annotations, SubsonicError> {
    let ids: Vec<String> = songs.iter().map(|s| s.id.clone()).collect();
    sdb::annotations(conn, user, ItemKind::Track, &ids).map_err(db_err)
}

pub fn songs_nodes(songs: &[SubsonicSong], ann: &Annotations) -> Vec<Node> {
    songs.iter().map(|s| song_node(s, ann)).collect()
}

/// Routes a method name (without `.view`) to its handler.
pub async fn handle(name: &str, ctx: &Ctx) -> Result<Reply, SubsonicError> {
    let payload = match name {
        "ping" => Node::new("payload"),
        "getLicense" => Node::new("payload").object(Node::new("license").attr("valid", true)),
        "getOpenSubsonicExtensions" => open_subsonic_extensions(),
        "getUser" => get_user(ctx).await?,
        "getMusicFolders" => music_folders(ctx).await?,
        "getIndexes" => indexes(ctx).await?,
        "getArtists" => artists(ctx).await?,
        "getArtist" => artist(ctx).await?,
        "getAlbum" => album(ctx).await?,
        "getSong" => song(ctx).await?,
        "getGenres" => genres(ctx).await?,
        "getMusicDirectory" => music_directory(ctx).await?,
        "getAlbumList" => album_list(ctx, true).await?,
        "getAlbumList2" => album_list(ctx, false).await?,
        "getRandomSongs" => random_songs(ctx).await?,
        "getSongsByGenre" => songs_by_genre(ctx).await?,
        "getStarred" => starred(ctx, true).await?,
        "getStarred2" => starred(ctx, false).await?,
        "getNowPlaying" => info::now_playing(ctx).await?,
        "search2" => search(ctx, true).await?,
        "search3" => search(ctx, false).await?,
        "getPlaylists" => playlists::playlists(ctx).await?,
        "getPlaylist" => playlists::playlist(ctx).await?,
        "createPlaylist" => playlists::create_playlist(ctx).await?,
        "updatePlaylist" => playlists::update_playlist(ctx).await?,
        "deletePlaylist" => playlists::delete_playlist(ctx).await?,
        "scrobble" => annotate::scrobble(ctx).await?,
        "star" => annotate::star(ctx, true).await?,
        "unstar" => annotate::star(ctx, false).await?,
        "setRating" => annotate::set_rating(ctx).await?,
        "getLyrics" => info::lyrics(ctx).await?,
        "getLyricsBySongId" => info::lyrics_by_song_id(ctx).await?,
        "getArtistInfo" => info::artist_info(ctx, true).await?,
        "getArtistInfo2" => info::artist_info(ctx, false).await?,
        "getAlbumInfo" | "getAlbumInfo2" => info::album_info(ctx).await?,
        "getSimilarSongs" => info::similar_songs(ctx, true).await?,
        "getSimilarSongs2" => info::similar_songs(ctx, false).await?,
        "getTopSongs" => info::top_songs(ctx).await?,
        "stream" => return super::media::stream(ctx, false).await.map(Reply::Raw),
        "download" => return super::media::stream(ctx, true).await.map(Reply::Raw),
        "getCoverArt" => return super::media::cover_art(ctx).await.map(Reply::Raw),
        _ if UNSUPPORTED.contains(&name) => return Err(SubsonicError::unsupported()),
        _ => {
            let shown: String = name.chars().take(64).collect();
            return Err(SubsonicError::generic(format!("Unknown method: {shown}")));
        }
    };
    Ok(Reply::Payload(payload))
}

/// `getOpenSubsonicExtensions` (no authentication needed).
pub fn open_subsonic_extensions() -> Node {
    Node::new("payload").list(
        "openSubsonicExtensions",
        EXTENSIONS
            .iter()
            .map(|(name, versions)| {
                Node::new("x").attr("name", *name).list(
                    "versions",
                    versions.iter().map(|v| Node::new("v").text(*v)).collect(),
                )
            })
            .collect(),
    )
}

// ── System ─────────────────────────────────────────────────────────────────────

async fn get_user(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let username = ctx.params.required("username")?.to_owned();
    if !username.eq_ignore_ascii_case(&ctx.user.username) && !ctx.user.is_admin() {
        return Err(SubsonicError::not_authorized());
    }
    let (account, folders) = ctx
        .db(move |conn| {
            let account = sdb::get_account_by_username(conn, &username).map_err(db_err)?;
            let folders = sdb::music_folders(conn).map_err(db_err)?;
            Ok((account, folders.len()))
        })
        .await?;
    let account = account.ok_or_else(|| SubsonicError::not_found("User"))?;
    let admin = account.role == "admin";
    Ok(Node::new("payload").object(
        Node::new("user")
            .attr("username", account.username)
            .attr("scrobblingEnabled", true)
            .attr("adminRole", admin)
            .attr("settingsRole", false)
            .attr("downloadRole", true)
            .attr("uploadRole", false)
            .attr("playlistRole", true)
            .attr("coverArtRole", true)
            .attr("commentRole", false)
            .attr("podcastRole", false)
            .attr("streamRole", true)
            .attr("jukeboxRole", false)
            .attr("shareRole", false)
            .attr("videoConversionRole", false)
            .list(
                "folder",
                (1..=folders).map(|n| Node::new("f").text(n)).collect(),
            ),
    ))
}

async fn music_folders(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let folders = ctx
        .db(|conn| sdb::music_folders(conn).map_err(db_err))
        .await?;
    Ok(Node::new("payload").object(
        Node::new("musicFolders").list(
            "musicFolder",
            folders
                .into_iter()
                .enumerate()
                .map(|(i, (_, name))| Node::new("x").attr("id", i + 1).attr("name", name))
                .collect(),
        ),
    ))
}

// ── Browsing ───────────────────────────────────────────────────────────────────

async fn indexes(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let folder = ctx.params.text("musicFolderId").map(str::to_owned);
    let since = ctx.params.int("ifModifiedSince")?;
    let user = ctx.user_id();
    let (artists, ann, last_modified_ms) = ctx
        .db(move |conn| {
            let lib = resolve_folder(conn, folder.as_deref())?;
            let last = sdb::library_last_modified_secs(conn)
                .map_err(db_err)?
                .unwrap_or(0)
                * 1000;
            if since.is_some_and(|s| last > 0 && s >= last) {
                return Ok((Vec::new(), Annotations::default(), last));
            }
            let artists = sdb::list_artists(conn, lib.as_deref(), None, 0, -1).map_err(db_err)?;
            let ids: Vec<String> = artists.iter().map(|a| a.id.clone()).collect();
            let ann = sdb::annotations(conn, &user, ItemKind::Artist, &ids).map_err(db_err)?;
            Ok((artists, ann, last))
        })
        .await?;
    Ok(Node::new("payload").object(
        Node::new("indexes")
            .attr("lastModified", last_modified_ms)
            .attr("ignoredArticles", ctx.settings.ignored_articles.as_str())
            .list(
                "index",
                model::artist_indexes(&artists, &ann, &ctx.articles(), true),
            ),
    ))
}

async fn artists(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let folder = ctx.params.text("musicFolderId").map(str::to_owned);
    let user = ctx.user_id();
    let (artists, ann) = ctx
        .db(move |conn| {
            let lib = resolve_folder(conn, folder.as_deref())?;
            let artists = sdb::list_artists(conn, lib.as_deref(), None, 0, -1).map_err(db_err)?;
            let ids: Vec<String> = artists.iter().map(|a| a.id.clone()).collect();
            let ann = sdb::annotations(conn, &user, ItemKind::Artist, &ids).map_err(db_err)?;
            Ok((artists, ann))
        })
        .await?;
    Ok(Node::new("payload").object(
        Node::new("artists")
            .attr("ignoredArticles", ctx.settings.ignored_articles.as_str())
            .list(
                "index",
                model::artist_indexes(&artists, &ann, &ctx.articles(), false),
            ),
    ))
}

async fn artist(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let id = required_id(&ctx.params, "id")?;
    let user = ctx.user_id();
    let (artist, artist_ann, albums, album_ann) = ctx
        .db(move |conn| {
            let artist = sdb::get_artist(conn, &id)
                .map_err(db_err)?
                .ok_or_else(|| SubsonicError::not_found("Artist"))?;
            let artist_ann = sdb::annotations(
                conn,
                &user,
                ItemKind::Artist,
                std::slice::from_ref(&artist.id),
            )
            .map_err(db_err)?;
            let albums = sdb::artist_albums(conn, &artist.id).map_err(db_err)?;
            let ids: Vec<String> = albums.iter().map(|a| a.id.clone()).collect();
            let album_ann = sdb::annotations(conn, &user, ItemKind::Album, &ids).map_err(db_err)?;
            Ok((artist, artist_ann, albums, album_ann))
        })
        .await?;
    Ok(Node::new("payload").object(
        artist_id3_node(&artist, &artist_ann, &ctx.articles()).list(
            "album",
            albums
                .iter()
                .map(|a| album_id3_node(a, &album_ann))
                .collect(),
        ),
    ))
}

async fn album(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let id = required_id(&ctx.params, "id")?;
    let user = ctx.user_id();
    let (album, album_ann, songs, song_ann) = ctx
        .db(move |conn| {
            let album = sdb::get_album(conn, &id)
                .map_err(db_err)?
                .ok_or_else(|| SubsonicError::not_found("Album"))?;
            let album_ann = sdb::annotations(
                conn,
                &user,
                ItemKind::Album,
                std::slice::from_ref(&album.id),
            )
            .map_err(db_err)?;
            let songs = sdb::album_songs(conn, &album.id).map_err(db_err)?;
            let song_ann = song_annotations(conn, &user, &songs)?;
            Ok((album, album_ann, songs, song_ann))
        })
        .await?;
    Ok(Node::new("payload")
        .object(album_id3_node(&album, &album_ann).list("song", songs_nodes(&songs, &song_ann))))
}

async fn song(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let id = required_id(&ctx.params, "id")?;
    if let Some(mix_id) = model::mix_id(&id).map(str::to_owned) {
        let user = ctx.user_id();
        let mix = ctx
            .db(move |conn| sdb::get_user_mix(conn, &user, &mix_id).map_err(db_err))
            .await?
            .ok_or_else(|| SubsonicError::not_found("Song"))?;
        return Ok(Node::new("payload").object(model::mix_song_node(&mix)));
    }
    let user = ctx.user_id();
    let (song, ann) = ctx
        .db(move |conn| {
            let song = sdb::get_song(conn, &id)
                .map_err(db_err)?
                .ok_or_else(|| SubsonicError::not_found("Song"))?;
            let ann = song_annotations(conn, &user, std::slice::from_ref(&song))?;
            Ok((song, ann))
        })
        .await?;
    Ok(Node::new("payload").object(song_node(&song, &ann)))
}

async fn genres(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let genres = ctx.db(|conn| sdb::genres(conn).map_err(db_err)).await?;
    Ok(Node::new("payload").object(
        Node::new("genres").list(
            "genre",
            genres
                .into_iter()
                .map(|g| {
                    Node::new("x")
                        .attr("songCount", g.song_count)
                        .attr("albumCount", g.album_count)
                        .text(g.name)
                })
                .collect(),
        ),
    ))
}

async fn music_directory(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let id = required_id(&ctx.params, "id")?;
    let user = ctx.user_id();
    let dir = ctx
        .db(move |conn| {
            if let Some(album) = sdb::get_album(conn, &id).map_err(db_err)? {
                let songs = sdb::album_songs(conn, &album.id).map_err(db_err)?;
                let ann = song_annotations(conn, &user, &songs)?;
                let album_ann = sdb::annotations(
                    conn,
                    &user,
                    ItemKind::Album,
                    std::slice::from_ref(&album.id),
                )
                .map_err(db_err)?;
                return Ok(Node::new("directory")
                    .attr("id", &album.id)
                    .opt("parent", album.artist_id.clone())
                    .attr("name", &album.name)
                    .attr("playCount", album.play_count)
                    .opt(
                        "starred",
                        album_ann.starred.get(&album.id).map(|t| model::iso(t)),
                    )
                    .opt(
                        "userRating",
                        album_ann
                            .ratings
                            .get(&album.id)
                            .map(|r| model::user_rating(*r)),
                    )
                    .list("child", songs_nodes(&songs, &ann)));
            }
            if let Some(artist) = sdb::get_artist(conn, &id).map_err(db_err)? {
                let albums = sdb::artist_albums(conn, &artist.id).map_err(db_err)?;
                let ids: Vec<String> = albums.iter().map(|a| a.id.clone()).collect();
                let ann = sdb::annotations(conn, &user, ItemKind::Album, &ids).map_err(db_err)?;
                let artist_ann = sdb::annotations(
                    conn,
                    &user,
                    ItemKind::Artist,
                    std::slice::from_ref(&artist.id),
                )
                .map_err(db_err)?;
                return Ok(Node::new("directory")
                    .attr("id", &artist.id)
                    .attr("name", &artist.name)
                    .opt(
                        "starred",
                        artist_ann.starred.get(&artist.id).map(|t| model::iso(t)),
                    )
                    .opt(
                        "userRating",
                        artist_ann
                            .ratings
                            .get(&artist.id)
                            .map(|r| model::user_rating(*r)),
                    )
                    .list(
                        "child",
                        albums.iter().map(|a| album_child_node(a, &ann)).collect(),
                    ));
            }
            Err(SubsonicError::not_found("Directory"))
        })
        .await?;
    Ok(Node::new("payload").object(dir))
}

// ── Lists ──────────────────────────────────────────────────────────────────────

async fn album_list(ctx: &Ctx, legacy: bool) -> Result<Node, SubsonicError> {
    let p = &ctx.params;
    let list_type = p.required("type")?.to_owned();
    let size = p.clamped("size", 10, 1, MAX_PAGE)?;
    let offset = p.clamped("offset", 0, 0, 1_000_000)?;
    let (from_year, to_year) = (p.int("fromYear")?, p.int("toYear")?);
    if list_type == "byYear" {
        p.required("fromYear")?;
        p.required("toYear")?;
    }
    let genre = p.text("genre").map(str::to_owned);
    if list_type == "byGenre" && genre.is_none() {
        return Err(SubsonicError::missing("genre"));
    }
    let folder = p.text("musicFolderId").map(str::to_owned);
    let user = ctx.user_id();
    let (albums, ann) = ctx
        .db(move |conn| {
            let lib = resolve_folder(conn, folder.as_deref())?;
            let albums = sdb::album_list(
                conn,
                sdb::AlbumListQuery {
                    list_type: &list_type,
                    user_id: &user,
                    size,
                    offset,
                    from_year,
                    to_year,
                    genre: genre.as_deref(),
                    library: lib.as_deref(),
                },
            )
            .map_err(db_err)?
            .ok_or_else(|| {
                SubsonicError::generic(format!(
                    "Invalid list type: {}",
                    list_type.chars().take(32).collect::<String>()
                ))
            })?;
            let ids: Vec<String> = albums.iter().map(|a| a.id.clone()).collect();
            let ann = sdb::annotations(conn, &user, ItemKind::Album, &ids).map_err(db_err)?;
            Ok((albums, ann))
        })
        .await?;
    let (container, nodes) = if legacy {
        (
            "albumList",
            albums.iter().map(|a| album_child_node(a, &ann)).collect(),
        )
    } else {
        (
            "albumList2",
            albums.iter().map(|a| album_id3_node(a, &ann)).collect(),
        )
    };
    Ok(Node::new("payload").object(Node::new(container).list("album", nodes)))
}

async fn random_songs(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let p = &ctx.params;
    let size = p.clamped("size", 10, 1, MAX_PAGE)?;
    let genre = p.text("genre").map(str::to_owned);
    let (from_year, to_year) = (p.int("fromYear")?, p.int("toYear")?);
    let folder = p.text("musicFolderId").map(str::to_owned);
    let user = ctx.user_id();
    let (songs, ann) = ctx
        .db(move |conn| {
            let lib = resolve_folder(conn, folder.as_deref())?;
            let songs = sdb::random_songs(
                conn,
                size,
                genre.as_deref(),
                from_year,
                to_year,
                lib.as_deref(),
            )
            .map_err(db_err)?;
            let ann = song_annotations(conn, &user, &songs)?;
            Ok((songs, ann))
        })
        .await?;
    Ok(Node::new("payload")
        .object(Node::new("randomSongs").list("song", songs_nodes(&songs, &ann))))
}

async fn songs_by_genre(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let p = &ctx.params;
    let genre = p.required("genre")?.to_owned();
    let count = p.clamped("count", 10, 1, MAX_PAGE)?;
    let offset = p.clamped("offset", 0, 0, i64::MAX / 2)?;
    let folder = p.text("musicFolderId").map(str::to_owned);
    let user = ctx.user_id();
    let (songs, ann) = ctx
        .db(move |conn| {
            let lib = resolve_folder(conn, folder.as_deref())?;
            let songs =
                sdb::songs_by_genre(conn, &genre, lib.as_deref(), offset, count).map_err(db_err)?;
            let ann = song_annotations(conn, &user, &songs)?;
            Ok((songs, ann))
        })
        .await?;
    Ok(Node::new("payload")
        .object(Node::new("songsByGenre").list("song", songs_nodes(&songs, &ann))))
}

async fn starred(ctx: &Ctx, legacy: bool) -> Result<Node, SubsonicError> {
    let user = ctx.user_id();
    let (artists, artist_ann, albums, album_ann, songs, song_ann) = ctx
        .db(move |conn| {
            let artist_ids = sdb::starred_ids(conn, &user, ItemKind::Artist).map_err(db_err)?;
            let album_ids = sdb::starred_ids(conn, &user, ItemKind::Album).map_err(db_err)?;
            let song_ids = sdb::starred_ids(conn, &user, ItemKind::Track).map_err(db_err)?;
            let artists = sdb::artists_by_ids(conn, &artist_ids).map_err(db_err)?;
            let albums = sdb::albums_by_ids(conn, &album_ids).map_err(db_err)?;
            let songs = sdb::songs_by_ids(conn, &song_ids).map_err(db_err)?;
            let artist_ann =
                sdb::annotations(conn, &user, ItemKind::Artist, &artist_ids).map_err(db_err)?;
            let album_ann =
                sdb::annotations(conn, &user, ItemKind::Album, &album_ids).map_err(db_err)?;
            let song_ann = song_annotations(conn, &user, &songs)?;
            Ok((artists, artist_ann, albums, album_ann, songs, song_ann))
        })
        .await?;
    let articles = ctx.articles();
    let container = if legacy {
        Node::new("starred")
            .list(
                "artist",
                artists
                    .iter()
                    .map(|a| artist_legacy_node(a, &artist_ann))
                    .collect(),
            )
            .list(
                "album",
                albums
                    .iter()
                    .map(|a| album_child_node(a, &album_ann))
                    .collect(),
            )
    } else {
        Node::new("starred2")
            .list(
                "artist",
                artists
                    .iter()
                    .map(|a| artist_id3_node(a, &artist_ann, &articles))
                    .collect(),
            )
            .list(
                "album",
                albums
                    .iter()
                    .map(|a| album_id3_node(a, &album_ann))
                    .collect(),
            )
    };
    Ok(Node::new("payload").object(container.list("song", songs_nodes(&songs, &song_ann))))
}

// ── Search ─────────────────────────────────────────────────────────────────────

/// Clients sync by searching for "" (sometimes sent as `""` or `*`).
fn normalize_query(raw: Option<&str>) -> String {
    let q = raw.unwrap_or("").trim();
    if q == "\"\"" || q == "*" || q == "''" {
        String::new()
    } else {
        q.chars().take(256).collect()
    }
}

async fn search(ctx: &Ctx, legacy: bool) -> Result<Node, SubsonicError> {
    let p = &ctx.params;
    let query = normalize_query(p.get("query"));
    let page = |count: &str, offset: &str| -> Result<(i64, i64), SubsonicError> {
        Ok((
            p.clamped(count, 20, 0, MAX_PAGE)?,
            p.clamped(offset, 0, 0, i64::MAX / 2)?,
        ))
    };
    let (artist_count, artist_offset) = page("artistCount", "artistOffset")?;
    let (album_count, album_offset) = page("albumCount", "albumOffset")?;
    let (song_count, song_offset) = page("songCount", "songOffset")?;
    let folder = p.text("musicFolderId").map(str::to_owned);
    let user = ctx.user_id();
    let (artists, artist_ann, albums, album_ann, songs, song_ann) = ctx
        .db(move |conn| {
            let lib = resolve_folder(conn, folder.as_deref())?;
            let lib = lib.as_deref();
            let name_query = (!query.is_empty()).then_some(query.as_str());
            let artists = if artist_count > 0 {
                sdb::list_artists(conn, lib, name_query, artist_offset, artist_count)
                    .map_err(db_err)?
            } else {
                Vec::new()
            };
            let albums = if album_count > 0 {
                sdb::search_albums(conn, &query, lib, album_offset, album_count).map_err(db_err)?
            } else {
                Vec::new()
            };
            let songs = if song_count == 0 {
                Vec::new()
            } else if query.is_empty() {
                sdb::songs_page(conn, lib, song_offset, song_count).map_err(db_err)?
            } else {
                sdb::search_songs(conn, &query, lib, song_offset, song_count).map_err(db_err)?
            };
            let artist_ids: Vec<String> = artists.iter().map(|a| a.id.clone()).collect();
            let album_ids: Vec<String> = albums.iter().map(|a| a.id.clone()).collect();
            let artist_ann =
                sdb::annotations(conn, &user, ItemKind::Artist, &artist_ids).map_err(db_err)?;
            let album_ann =
                sdb::annotations(conn, &user, ItemKind::Album, &album_ids).map_err(db_err)?;
            let song_ann = song_annotations(conn, &user, &songs)?;
            Ok((artists, artist_ann, albums, album_ann, songs, song_ann))
        })
        .await?;
    let articles = ctx.articles();
    let result = if legacy {
        Node::new("searchResult2")
            .list(
                "artist",
                artists
                    .iter()
                    .map(|a| artist_legacy_node(a, &artist_ann))
                    .collect(),
            )
            .list(
                "album",
                albums
                    .iter()
                    .map(|a| album_child_node(a, &album_ann))
                    .collect(),
            )
    } else {
        Node::new("searchResult3")
            .list(
                "artist",
                artists
                    .iter()
                    .map(|a| artist_id3_node(a, &artist_ann, &articles))
                    .collect(),
            )
            .list(
                "album",
                albums
                    .iter()
                    .map(|a| album_id3_node(a, &album_ann))
                    .collect(),
            )
    };
    Ok(Node::new("payload").object(result.list("song", songs_nodes(&songs, &song_ann))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_restricted_to_safe_characters() {
        assert_eq!(
            valid_id("0190a3c4-7f0e-7a3b-9f1e-1234567890ab")
                .unwrap()
                .len(),
            36
        );
        assert!(valid_id("12").is_ok());
        assert_eq!(valid_id("../etc/passwd").unwrap_err().code, 70);
        assert_eq!(valid_id("a b").unwrap_err().code, 70);
        assert!(valid_id(&"a".repeat(129)).is_err());
        assert!(valid_id("").is_err());
    }

    #[test]
    fn empty_search_spellings() {
        assert_eq!(normalize_query(None), "");
        assert_eq!(normalize_query(Some("\"\"")), "");
        assert_eq!(normalize_query(Some(" * ")), "");
        assert_eq!(normalize_query(Some(" abba ")), "abba");
        assert_eq!(normalize_query(Some(&"x".repeat(300))).len(), 256);
    }

    #[test]
    fn unsupported_list_has_no_implemented_methods() {
        for implemented in [
            "ping",
            "getLicense",
            "getUser",
            "getMusicFolders",
            "getIndexes",
            "getArtists",
            "getArtist",
            "getAlbum",
            "getSong",
            "getGenres",
            "getMusicDirectory",
            "getAlbumList",
            "getAlbumList2",
            "getRandomSongs",
            "getSongsByGenre",
            "getStarred",
            "getStarred2",
            "getNowPlaying",
            "star",
            "unstar",
            "setRating",
            "createPlaylist",
            "updatePlaylist",
            "deletePlaylist",
            "getLyrics",
            "getLyricsBySongId",
            "getArtistInfo",
            "getArtistInfo2",
            "getAlbumInfo",
            "getAlbumInfo2",
            "getSimilarSongs",
            "getSimilarSongs2",
            "getTopSongs",
            "search2",
            "search3",
            "getPlaylists",
            "getPlaylist",
            "scrobble",
            "stream",
            "download",
            "getCoverArt",
            "getOpenSubsonicExtensions",
        ] {
            assert!(!UNSUPPORTED.contains(&implemented), "{implemented}");
        }
    }
}
