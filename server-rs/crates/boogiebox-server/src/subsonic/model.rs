//! Field mapping from BoogieBox rows to Subsonic elements — the one place that decides what clients see.
//! Real filesystem paths never leave the server: `path` is a virtual `Artist/Album/NN - Title.ext`.

use std::path::Path;

use boogiebox_db::subsonic::{Annotations, SubsonicAlbum, SubsonicArtist, SubsonicSong};

use super::response::Node;
use crate::ffmpeg;

/// SQLite `YYYY-MM-DD HH:MM:SS` (UTC) → ISO-8601 `YYYY-MM-DDTHH:MM:SSZ`.
pub fn iso(ts: &str) -> String {
    let t = ts.trim();
    let mut out = t.replacen(' ', "T", 1);
    if out.len() == 10 {
        out.push_str("T00:00:00");
    }
    if !out.ends_with('Z') && !out.contains('+') {
        out.push('Z');
    }
    out
}

/// BoogieBox half-star rating → Subsonic whole stars (half rounds up), 1–5.
pub fn user_rating(rating: f64) -> i64 {
    ((rating + 0.5).floor() as i64).clamp(1, 5)
}

/// Lower-case file extension.
pub fn suffix(file_path: &str) -> String {
    Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

/// Individual tags of a compound genre string ("Rock, Alternative").
pub fn split_genres(genre: Option<&str>) -> Vec<String> {
    genre
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(str::to_owned)
        .collect()
}

fn path_segment(raw: Option<&str>, fallback: &str) -> String {
    let cleaned: String = raw
        .unwrap_or("")
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .replace("..", "_");
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Virtual path shown to clients (display/sorting only).
pub fn virtual_path(song: &SubsonicSong) -> String {
    let artist = path_segment(
        song.album_artist.as_deref().or(song.artist.as_deref()),
        "Unknown Artist",
    );
    let album = path_segment(song.album.as_deref(), "Unknown Album");
    let title = path_segment(Some(&song.title), "Untitled");
    let ext = suffix(&song.file_path);
    let name = match song.track_number {
        Some(n) if n > 0 => format!("{n:02} - {title}"),
        _ => title,
    };
    if ext.is_empty() {
        format!("{artist}/{album}/{name}")
    } else {
        format!("{artist}/{album}/{name}.{ext}")
    }
}

/// Cover-art id for an album.
pub fn album_cover_id(album_id: &str) -> String {
    format!("al-{album_id}")
}

/// Cover-art id for an artist.
pub fn artist_cover_id(artist_id: &str) -> String {
    format!("ar-{artist_id}")
}

fn effective_bpm(song: &SubsonicSong) -> Option<i64> {
    song.bpm
        .filter(|b| *b > 0.0)
        .or(song.bpm_detected.filter(|b| *b > 0.0))
        .map(|b| b.round() as i64)
}

fn id_name(id: Option<&str>, name: Option<&str>) -> Vec<Node> {
    match (id, name) {
        (Some(id), Some(name)) => vec![Node::new("x").attr("id", id).attr("name", name)],
        _ => Vec::new(),
    }
}

fn genre_list(genres: &[String]) -> Vec<Node> {
    genres
        .iter()
        .map(|g| Node::new("x").attr("name", g.as_str()))
        .collect()
}

/// A song (`Child`). `name` is `song`, `child` or `entry` depending on the response.
pub fn song_node(song: &SubsonicSong, ann: &Annotations) -> Node {
    let path = Path::new(&song.file_path);
    let genres = split_genres(song.genre.as_deref());
    let cover = song
        .album_id
        .as_deref()
        .map(album_cover_id)
        .unwrap_or_else(|| format!("tr-{}", song.id));
    let mut node = Node::new("song")
        .attr("id", &song.id)
        .opt("parent", song.album_id.clone())
        .attr("isDir", false)
        .attr("title", &song.title)
        .opt("album", song.album.clone())
        .opt("artist", song.artist.clone())
        .opt("track", song.track_number)
        .opt("year", song.year)
        .opt("genre", genres.first().cloned())
        .attr("coverArt", cover)
        .attr("size", song.file_size)
        .attr("contentType", ffmpeg::audio_mime_type(path))
        .attr("suffix", suffix(&song.file_path));
    if ffmpeg::needs_audio_transcode(path) {
        node = node
            .attr("transcodedContentType", "audio/mpeg")
            .attr("transcodedSuffix", "mp3");
    }
    node.attr("duration", song.duration.max(0.0).round() as i64)
        .opt("bitRate", song.bitrate)
        .attr("path", virtual_path(song))
        .attr("playCount", song.play_count)
        .opt("played", ann.played.get(&song.id).map(|t| iso(t)))
        .opt(
            "userRating",
            ann.ratings.get(&song.id).map(|r| user_rating(*r)),
        )
        .opt("starred", ann.starred.get(&song.id).map(|t| iso(t)))
        .opt("discNumber", song.disc_number)
        .opt("created", song.created.as_deref().map(iso))
        .opt("albumId", song.album_id.clone())
        .opt("artistId", song.artist_id.clone())
        .attr("type", "music")
        .attr("isVideo", false)
        .attr("mediaType", "song")
        .opt("bpm", effective_bpm(song))
        .opt("comment", song.comment.clone())
        .opt("samplingRate", song.sample_rate)
        .opt("channelCount", song.channels)
        .opt("displayArtist", song.artist.clone())
        .opt("displayAlbumArtist", song.album_artist.clone())
        .list("genres", genre_list(&genres))
        .list(
            "artists",
            id_name(song.artist_id.as_deref(), song.artist.as_deref()),
        )
        .list(
            "albumArtists",
            id_name(
                song.album_artist_id.as_deref(),
                song.album_artist.as_deref(),
            ),
        )
}

/// Id of the per-user read-only "BoogieMix" playlist.
pub const BOOGIEMIX_PLAYLIST_ID: &str = "bm-mixes";
/// Song-id prefix for BoogieMix mixes.
pub const MIX_ID_PREFIX: &str = "mx-";

/// The mix id inside an `mx-…` song id.
pub fn mix_id(song_id: &str) -> Option<&str> {
    song_id
        .strip_prefix(MIX_ID_PREFIX)
        .filter(|id| !id.is_empty())
}

/// A BoogieMix render as one long song in the "BoogieMix" playlist.
pub fn mix_song_node(mix: &boogiebox_db::subsonic::SubsonicMix) -> Node {
    let format = if mix.format.is_empty() {
        "mp3"
    } else {
        mix.format.as_str()
    };
    let name = path_segment(Some(&mix.name), "BoogieMix");
    Node::new("song")
        .attr("id", format!("{MIX_ID_PREFIX}{}", mix.id))
        .attr("parent", BOOGIEMIX_PLAYLIST_ID)
        .attr("isDir", false)
        .attr("title", &mix.name)
        .attr("album", "BoogieMix")
        .attr("artist", "BoogieMix")
        .opt(
            "coverArt",
            mix.cover_album_id.as_deref().map(album_cover_id),
        )
        .attr("size", mix.file_size)
        .attr(
            "contentType",
            ffmpeg::audio_mime_type(Path::new(&format!("mix.{format}"))),
        )
        .attr("suffix", format)
        .attr("duration", mix.duration.max(0.0).round() as i64)
        .attr("path", format!("BoogieMix/{name}.{format}"))
        .attr("playCount", 0)
        .opt("created", mix.created.as_deref().map(iso))
        .attr("type", "music")
        .attr("isVideo", false)
        .attr("mediaType", "song")
        .opt(
            "comment",
            mix.source_playlist_name
                .as_deref()
                .filter(|n| !n.trim().is_empty())
                .map(|n| format!("Mixed from {n}")),
        )
        .attr("displayArtist", "BoogieMix")
}

/// An album as `AlbumID3` (`getAlbum`, `getAlbumList2`, `search3`).
pub fn album_id3_node(album: &SubsonicAlbum, ann: &Annotations) -> Node {
    let genres = split_genres(album.genre.as_deref());
    Node::new("album")
        .attr("id", &album.id)
        .attr("name", &album.name)
        .attr("artist", &album.artist)
        .opt("artistId", album.artist_id.clone())
        .attr("coverArt", album_cover_id(&album.id))
        .attr("songCount", album.song_count)
        .attr("duration", album.duration.max(0.0).round() as i64)
        .attr("playCount", album.play_count)
        .attr(
            "created",
            iso(album.created.as_deref().unwrap_or("1970-01-01 00:00:00")),
        )
        .opt("year", album.year)
        .opt("genre", genres.first().cloned())
        .opt(
            "userRating",
            ann.ratings.get(&album.id).map(|r| user_rating(*r)),
        )
        .opt("starred", ann.starred.get(&album.id).map(|t| iso(t)))
        .attr("displayArtist", &album.artist)
        .attr(
            "isCompilation",
            album.release_type.as_deref() == Some("compilation"),
        )
        .attr("mediaType", "album")
        .list("genres", genre_list(&genres))
        .list(
            "artists",
            id_name(
                album.artist_id.as_deref(),
                Some(album.artist.as_str()).filter(|a| !a.is_empty()),
            ),
        )
}

/// An album as a directory `Child` (`getAlbumList`, `getMusicDirectory`, `search2`).
pub fn album_child_node(album: &SubsonicAlbum, ann: &Annotations) -> Node {
    let genres = split_genres(album.genre.as_deref());
    Node::new("album")
        .attr("id", &album.id)
        .opt("parent", album.artist_id.clone())
        .attr("isDir", true)
        .attr("title", &album.name)
        .attr("name", &album.name)
        .attr("album", &album.name)
        .attr("artist", &album.artist)
        .opt("year", album.year)
        .opt("genre", genres.first().cloned())
        .attr("coverArt", album_cover_id(&album.id))
        .attr("duration", album.duration.max(0.0).round() as i64)
        .attr("playCount", album.play_count)
        .attr(
            "created",
            iso(album.created.as_deref().unwrap_or("1970-01-01 00:00:00")),
        )
        .opt(
            "userRating",
            ann.ratings.get(&album.id).map(|r| user_rating(*r)),
        )
        .opt("starred", ann.starred.get(&album.id).map(|t| iso(t)))
        .opt("artistId", album.artist_id.clone())
        .attr("songCount", album.song_count)
        .attr("isVideo", false)
}

/// An artist as `ArtistID3`.
pub fn artist_id3_node(artist: &SubsonicArtist, ann: &Annotations, articles: &[String]) -> Node {
    Node::new("artist")
        .attr("id", &artist.id)
        .attr("name", &artist.name)
        .attr("coverArt", artist_cover_id(&artist.id))
        .attr("albumCount", artist.album_count)
        .opt("starred", ann.starred.get(&artist.id).map(|t| iso(t)))
        .opt(
            "userRating",
            ann.ratings.get(&artist.id).map(|r| user_rating(*r)),
        )
        .attr("sortName", sort_name(&artist.name, articles))
}

/// An artist in the legacy folder model (`getIndexes`, `search2`).
pub fn artist_legacy_node(artist: &SubsonicArtist, ann: &Annotations) -> Node {
    Node::new("artist")
        .attr("id", &artist.id)
        .attr("name", &artist.name)
        .attr("coverArt", artist_cover_id(&artist.id))
        .attr("albumCount", artist.album_count)
        .opt("starred", ann.starred.get(&artist.id).map(|t| iso(t)))
        .opt(
            "userRating",
            ann.ratings.get(&artist.id).map(|r| user_rating(*r)),
        )
}

/// Ignored-articles setting as words.
pub fn article_list(setting: &str) -> Vec<String> {
    setting
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .collect()
}

/// Name without a leading ignored article ("The Beatles" → "Beatles").
pub fn sort_name(name: &str, articles: &[String]) -> String {
    let trimmed = name.trim();
    for article in articles {
        if let Some((first, rest)) = trimmed.split_once(' ') {
            if first.to_lowercase() == *article && !rest.trim().is_empty() {
                return rest.trim().to_owned();
            }
        }
    }
    trimmed.to_owned()
}

/// Index letter for a sort name: the upper-cased first letter, or `#` for digits/symbols.
pub fn index_letter(sort: &str) -> String {
    match sort.chars().next() {
        Some(c) if c.is_alphabetic() => c.to_uppercase().collect(),
        _ => "#".to_owned(),
    }
}

/// Groups artists into `index` nodes ordered by sort name; `#` sorts last.
pub fn artist_indexes(
    artists: &[SubsonicArtist],
    ann: &Annotations,
    articles: &[String],
    legacy: bool,
) -> Vec<Node> {
    let mut keyed: Vec<(String, &SubsonicArtist)> = artists
        .iter()
        .map(|a| (sort_name(&a.name, articles).to_lowercase(), a))
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.name.cmp(&b.1.name)));
    let mut groups: Vec<(String, Vec<Node>)> = Vec::new();
    for (key, artist) in keyed {
        let letter = index_letter(&key);
        let node = if legacy {
            artist_legacy_node(artist, ann)
        } else {
            artist_id3_node(artist, ann, articles)
        };
        match groups.iter_mut().find(|(l, _)| *l == letter) {
            Some((_, nodes)) => nodes.push(node),
            None => groups.push((letter, vec![node])),
        }
    }
    groups.sort_by(|a, b| match (a.0 == "#", b.0 == "#") {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => a.0.cmp(&b.0),
    });
    groups
        .into_iter()
        .map(|(letter, nodes)| {
            Node::new("index")
                .attr("name", letter)
                .list("artist", nodes)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> SubsonicSong {
        SubsonicSong {
            id: "t1".into(),
            title: "What/Is: This?".into(),
            album_id: Some("al1".into()),
            album: Some("..Secret ../Album".into()),
            artist_id: Some("ar1".into()),
            artist: Some("Artist".into()),
            album_artist_id: Some("ar9".into()),
            album_artist: Some("Album Artist".into()),
            track_number: Some(3),
            disc_number: Some(1),
            year: Some(2001),
            genre: Some("Rock, Alternative".into()),
            file_path: r"D:\Music\private\folder\03 song.FLAC".into(),
            file_size: 1234,
            duration: 201.6,
            bitrate: Some(900),
            sample_rate: Some(44100),
            channels: Some(2),
            play_count: 4,
            bpm: Some(0.0),
            bpm_detected: Some(127.6),
            comment: None,
            created: Some("2026-03-04 05:06:07".into()),
            library_id: Some("lib1".into()),
        }
    }

    fn attr<'a>(node: &'a Node, name: &str) -> Option<&'a super::super::response::Scalar> {
        node.attrs.iter().find(|(k, _)| *k == name).map(|(_, v)| v)
    }

    #[test]
    fn song_node_maps_fields_without_leaking_paths() {
        let mut ann = Annotations::default();
        ann.ratings.insert("t1".into(), 3.5);
        ann.starred
            .insert("t1".into(), "2026-01-01 00:00:00".into());
        let node = song_node(&song(), &ann);
        use super::super::response::Scalar::*;
        assert_eq!(
            attr(&node, "path"),
            Some(&Str(
                "Album Artist/_Secret __Album/03 - What_Is_ This_.flac".into()
            ))
        );
        assert_eq!(attr(&node, "suffix"), Some(&Str("flac".into())));
        assert_eq!(attr(&node, "contentType"), Some(&Str("audio/flac".into())));
        assert_eq!(attr(&node, "transcodedSuffix"), Some(&Str("mp3".into())));
        assert_eq!(attr(&node, "duration"), Some(&Int(202)));
        assert_eq!(attr(&node, "genre"), Some(&Str("Rock".into())));
        assert_eq!(attr(&node, "coverArt"), Some(&Str("al-al1".into())));
        assert_eq!(attr(&node, "userRating"), Some(&Int(4)));
        assert_eq!(
            attr(&node, "starred"),
            Some(&Str("2026-01-01T00:00:00Z".into()))
        );
        assert_eq!(
            attr(&node, "created"),
            Some(&Str("2026-03-04T05:06:07Z".into()))
        );
        assert_eq!(attr(&node, "bpm"), Some(&Int(128)));
        assert!(attr(&node, "played").is_none());
        let rendered = format!("{node:?}");
        assert!(
            !rendered.contains("private"),
            "real path leaked: {rendered}"
        );
        assert_eq!(
            node.lists
                .iter()
                .find(|(n, _)| *n == "genres")
                .unwrap()
                .1
                .len(),
            2
        );
    }

    #[test]
    fn ratings_round_half_up_and_clamp() {
        assert_eq!(user_rating(0.5), 1);
        assert_eq!(user_rating(2.5), 3);
        assert_eq!(user_rating(3.0), 3);
        assert_eq!(user_rating(5.0), 5);
    }

    #[test]
    fn iso_formats() {
        assert_eq!(iso("2026-01-02 03:04:05"), "2026-01-02T03:04:05Z");
        assert_eq!(iso("2026-01-02"), "2026-01-02T00:00:00Z");
        assert_eq!(iso("2026-01-02T03:04:05Z"), "2026-01-02T03:04:05Z");
    }

    #[test]
    fn artist_indexes_strip_articles_and_group() {
        let articles = article_list("The El");
        let artists: Vec<SubsonicArtist> = ["The Beatles", "abba", "Björk", "2Pac", "The", "Air"]
            .iter()
            .enumerate()
            .map(|(i, n)| SubsonicArtist {
                id: format!("a{i}"),
                name: (*n).into(),
                album_count: 1,
            })
            .collect();
        let idx = artist_indexes(&artists, &Annotations::default(), &articles, false);
        let letters: Vec<String> = idx
            .iter()
            .map(|n| match attr(n, "name") {
                Some(super::super::response::Scalar::Str(s)) => s.clone(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(letters, ["A", "B", "T", "#"]);
        assert_eq!(idx[1].lists[0].1.len(), 2); // Beatles + Björk
        assert_eq!(sort_name("The Beatles", &articles), "Beatles");
        assert_eq!(sort_name("The", &articles), "The");
        assert_eq!(sort_name("Theory", &articles), "Theory");
    }
}
