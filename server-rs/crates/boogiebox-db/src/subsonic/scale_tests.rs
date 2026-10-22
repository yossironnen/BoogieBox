//! Subsonic sync timings on a synthetic 100k-track library (plan P3-5). Ignored by default; run with
//! `cargo test --manifest-path server-rs/Cargo.toml -p boogiebox-db subsonic_scale -- --ignored --nocapture`.
//! Each figure is the slowest of three runs of one call, i.e. one request's hold of the shared connection.

use std::time::{Duration, Instant};

use rusqlite::{params, Connection};

use super::*;

const TRACKS: usize = 100_000;
const ALBUMS: usize = 8_000;
const ARTISTS: usize = 3_000;
const PAGE: i64 = 500;
/// Target from the plan (§10): one 500-row page under 40 ms. `frequent`/`recent` sit near it here because the
/// fixture user has 50k distinct tracks played (one primary-key lookup each); a covering `tracks(id, album_id)`
/// index only took them from ~38 to ~30 ms, so it was not added (2026-10-22).
const TARGET: Duration = Duration::from_millis(40);

fn seed(conn: &Connection) {
    let genres = [
        "Rock",
        "Pop",
        "Jazz",
        "Electronic",
        "Rock, Alternative",
        "Hip-Hop",
        "Classical",
        "Soul",
    ];
    conn.execute_batch(
        "INSERT INTO users(id, username, role) VALUES ('u1', 'alice', 'user');
         INSERT INTO libraries(id, path, name) VALUES ('lib1', '/music', 'Music');
         BEGIN;",
    )
    .unwrap();
    {
        let mut artist = conn
            .prepare("INSERT INTO artists(id, name) VALUES (?1, ?2)")
            .unwrap();
        for a in 0..ARTISTS {
            artist
                .execute(params![format!("ar{a:05}"), format!("Artist {a:05}")])
                .unwrap();
        }
        let mut album = conn
            .prepare("INSERT INTO albums(id, title, artist_id, year, added_at) VALUES (?1, ?2, ?3, ?4, ?5)")
            .unwrap();
        for al in 0..ALBUMS {
            album
                .execute(params![
                    format!("al{al:05}"),
                    format!("Album {al:05}"),
                    format!("ar{:05}", al % ARTISTS),
                    1960 + (al % 60) as i64,
                    format!("2026-01-01 00:{:02}:{:02}", (al / 60) % 60, al % 60),
                ])
                .unwrap();
        }
        let mut track = conn
            .prepare(
                "INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path, duration, track_number,
                                    disc_number, genre, year, bitrate, file_size)
                 VALUES (?1, 'lib1', ?2, ?3, ?4, ?5, 240, ?6, 1, ?7, 2000, 320, 9000000)",
            )
            .unwrap();
        for t in 0..TRACKS {
            let al = t % ALBUMS;
            track
                .execute(params![
                    format!("t{t:06}"),
                    format!("ar{:05}", al % ARTISTS),
                    format!("al{al:05}"),
                    format!("Song {t:06}"),
                    format!("/music/{al:05}/{t:06}.flac"),
                    (t / ALBUMS + 1) as i64,
                    genres[t % genres.len()],
                ])
                .unwrap();
        }
        let mut rating = conn
            .prepare("INSERT INTO track_ratings(user_id, track_id, rating) VALUES ('u1', ?1, ?2)")
            .unwrap();
        for t in (0..TRACKS).step_by(5) {
            rating
                .execute(params![format!("t{t:06}"), (t % 5 + 1) as f64])
                .unwrap();
        }
        let mut star = conn
            .prepare("INSERT INTO subsonic_stars(user_id, item_type, item_id) VALUES ('u1', 'track', ?1)")
            .unwrap();
        for t in (0..TRACKS).step_by(50) {
            star.execute([format!("t{t:06}")]).unwrap();
        }
        let mut play = conn
            .prepare("INSERT INTO play_history(id, user_id, track_id, played_at) VALUES (?1, 'u1', ?2, ?3)")
            .unwrap();
        for p in 0..50_000usize {
            play.execute(params![
                format!("p{p:06}"),
                format!("t{:06}", (p * 7) % TRACKS),
                format!("2026-05-{:02} 10:00:00", p % 28 + 1),
            ])
            .unwrap();
        }
    }
    conn.execute_batch("COMMIT; ANALYZE;").unwrap();
}

/// Slowest of three runs.
fn time<T>(f: impl Fn() -> T) -> Duration {
    (0..3)
        .map(|_| {
            let start = Instant::now();
            std::hint::black_box(f());
            start.elapsed()
        })
        .max()
        .unwrap_or_default()
}

#[test]
#[ignore = "builds a 100k-track library; run manually for P3-5 timings"]
fn subsonic_scale() {
    let dir = std::env::temp_dir().join(format!("bb-subsonic-scale-{}", uuid::Uuid::now_v7()));
    let conn = crate::init_db(&dir).unwrap().connection;
    let seeded = Instant::now();
    seed(&conn);
    println!(
        "seeded {TRACKS} tracks in {:.1}s",
        seeded.elapsed().as_secs_f64()
    );

    let mut rows: Vec<(String, Duration, bool)> = Vec::new();
    let mut add = |name: String, d: Duration, page: bool| rows.push((name, d, page));

    for offset in [0, 50_000, TRACKS as i64 - PAGE] {
        add(
            format!("search3 songs  offset {offset:>6} (+annotations)"),
            time(|| {
                let songs = songs_page(&conn, None, offset, PAGE).unwrap();
                let ids: Vec<String> = songs.iter().map(|s| s.id.clone()).collect();
                annotations(&conn, "u1", ItemKind::Track, &ids).unwrap()
            }),
            true,
        );
    }
    for offset in [0, 4_000, ALBUMS as i64 - PAGE] {
        add(
            format!("search3 albums offset {offset:>6} (+annotations)"),
            time(|| {
                let albums = search_albums(&conn, "", None, offset, PAGE).unwrap();
                let ids: Vec<String> = albums.iter().map(|a| a.id.clone()).collect();
                annotations(&conn, "u1", ItemKind::Album, &ids).unwrap()
            }),
            true,
        );
    }
    for offset in [0, 1_500, ARTISTS as i64 - PAGE] {
        add(
            format!("search3 artists offset {offset:>5}"),
            time(|| list_artists(&conn, None, None, offset, PAGE).unwrap()),
            true,
        );
    }
    add(
        "getArtists (all artists)".into(),
        time(|| list_artists(&conn, None, None, 0, -1).unwrap()),
        false,
    );
    for list_type in [
        "newest",
        "frequent",
        "recent",
        "alphabeticalByName",
        "alphabeticalByArtist",
        "random",
    ] {
        for offset in [0, 7_000] {
            add(
                format!("getAlbumList2 {list_type} offset {offset}"),
                time(|| {
                    album_list(
                        &conn,
                        AlbumListQuery {
                            list_type,
                            user_id: "u1",
                            size: PAGE,
                            offset,
                            from_year: None,
                            to_year: None,
                            genre: None,
                            library: None,
                        },
                    )
                    .unwrap()
                }),
                true,
            );
        }
    }
    add(
        "getRandomSongs 500".into(),
        time(|| random_songs(&conn, PAGE, None, None, None, None).unwrap()),
        true,
    );
    add(
        "getSongsByGenre Rock offset 0".into(),
        time(|| songs_by_genre(&conn, "Rock", None, 0, PAGE).unwrap()),
        true,
    );
    add(
        "getAlbumList2 byGenre Rock offset 0".into(),
        time(|| {
            album_list(
                &conn,
                AlbumListQuery {
                    list_type: "byGenre",
                    user_id: "u1",
                    size: PAGE,
                    offset: 0,
                    from_year: None,
                    to_year: None,
                    genre: Some("Rock"),
                    library: None,
                },
            )
            .unwrap()
        }),
        true,
    );
    add("getGenres".into(), time(|| genres(&conn).unwrap()), false);

    let queue: Vec<String> = (0..MAX_PLAY_QUEUE)
        .map(|i| format!("t{:06}", i * 97 % TRACKS))
        .collect();
    add(
        "savePlayQueue 1000".into(),
        time(|| save_play_queue(&conn, "u1", &queue, Some(10), 1000, Some("bench")).unwrap()),
        false,
    );
    add(
        "getPlayQueue 1000 (+songs, annotations)".into(),
        time(|| {
            let q = get_play_queue(&conn, "u1").unwrap().unwrap();
            let songs = songs_by_id_map(&conn, &q.track_ids).unwrap();
            let ids: Vec<String> = songs.keys().cloned().collect();
            annotations(&conn, "u1", ItemKind::Track, &ids).unwrap()
        }),
        false,
    );

    println!("\n{:<52} {:>9}", "call", "max ms");
    for (name, d, page) in &rows {
        let flag = if *page && *d > TARGET {
            "  OVER 40 ms"
        } else {
            ""
        };
        println!("{name:<52} {:>9.1}{flag}", d.as_secs_f64() * 1000.0);
    }
    drop(conn);
    let _ = std::fs::remove_dir_all(&dir);
}
