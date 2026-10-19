//! End-to-end `/rest` and credential-API tests against the full router and a temp database.

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    Router,
};
use rusqlite::params;
use serde_json::Value as Json;

use crate::test_support::{
    json_body, new_test_app_with_pool, seed_admin_session, seed_user_session, send, send_full,
};
use crate::DbPool;

const PASSWORD: &str = "Groove!2026";

struct Fixture {
    app: Router,
    pool: DbPool,
    cookie: String,
    lib_dir: std::path::PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.lib_dir);
    }
}

fn seed_library(pool: &DbPool) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bb-subsonic-lib-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(dir.join("Band/Album")).unwrap();
    std::fs::write(dir.join("Band/Album/01.mp3"), b"0123456789abcdef").unwrap();
    std::fs::write(dir.join("Band/Album/02.mp3"), b"second track bytes").unwrap();
    let outside =
        std::env::temp_dir().join(format!("bb-subsonic-outside-{}.mp3", uuid::Uuid::now_v7()));
    std::fs::write(&outside, b"outside").unwrap();
    let conn = pool.lock().unwrap();
    conn.execute(
        "INSERT INTO libraries(id, path, name, last_scan) VALUES ('lib1', ?1, 'Music', '2026-05-01 00:00:00')",
        params![dir.to_string_lossy()],
    )
    .unwrap();
    conn.execute_batch(
        "INSERT INTO artists(id, name) VALUES ('ar1', 'The Band'), ('ar2', 'Guest');
         INSERT INTO albums(id, title, artist_id, year, added_at) VALUES ('al1', 'Album', 'ar1', 2001, '2026-01-01 00:00:00');",
    )
    .unwrap();
    for (id, artist, title, file, n, genre) in [
        (
            "t1",
            "ar1",
            "Opening",
            dir.join("Band/Album/01.mp3"),
            1,
            "Rock",
        ),
        (
            "t2",
            "ar2",
            "Closing",
            dir.join("Band/Album/02.mp3"),
            2,
            "Rock, Pop",
        ),
        ("t3", "ar1", "Escapee", outside.clone(), 3, "Rock"),
    ] {
        conn.execute(
            "INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path, track_number, duration,
                                bitrate, genre, year, file_size)
             VALUES (?1, 'lib1', ?2, 'al1', ?3, ?4, ?5, 10, 320, ?6, 2001, 16)",
            params![id, artist, title, file.to_string_lossy(), n, genre],
        )
        .unwrap();
    }
    dir
}

impl Fixture {
    async fn new(prefix: &str) -> Self {
        let (app, pool) = new_test_app_with_pool(prefix);
        let cookie = seed_user_session(&pool, "u1");
        let lib_dir = seed_library(&pool);
        pool.lock()
            .unwrap()
            .execute(
                "INSERT INTO settings(key, value) VALUES ('subsonicApiEnabled', 'true')",
                [],
            )
            .unwrap();
        let fx = Fixture {
            app,
            pool,
            cookie,
            lib_dir,
        };
        let (status, body) = fx
            .put_password(
                &fx.cookie.clone(),
                "/api/subsonic/credentials/password",
                PASSWORD,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        fx
    }

    async fn put_password(&self, cookie: &str, uri: &str, password: &str) -> (StatusCode, Vec<u8>) {
        send(
            self.app.clone(),
            Request::builder()
                .method("PUT")
                .uri(uri)
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "password": password }).to_string(),
                ))
                .unwrap(),
        )
        .await
    }

    /// GET `/rest/{method}` as u1 with JSON output.
    async fn rest(&self, method: &str, query: &str) -> Json {
        let (status, body) = send(
            self.app.clone(),
            Request::builder()
                .uri(format!(
                    "/rest/{method}?u=u1&p={}&v=1.16.1&c=test&f=json&{query}",
                    urlencode(PASSWORD)
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        json_body(&body)["subsonic-response"].clone()
    }

    async fn raw(&self, uri: &str) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        send_full(
            self.app.clone(),
            Request::builder().uri(uri).body(Body::empty()).unwrap(),
        )
        .await
    }

    fn auth(&self) -> String {
        format!("u=u1&p={}&v=1.16.1&c=test", urlencode(PASSWORD))
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[tokio::test]
async fn disabled_api_and_missing_credentials_fail_with_subsonic_errors() {
    let (app, _pool) = new_test_app_with_pool("subsonic-disabled");
    let (status, body) = send(
        app.clone(),
        Request::builder()
            .uri("/rest/ping.view?u=a&p=b")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let xml = String::from_utf8(body).unwrap();
    assert!(
        xml.contains("status=\"failed\"") && xml.contains("code=\"0\""),
        "{xml}"
    );
    let (_, body) = send(
        app,
        Request::builder()
            .uri("/rest/getOpenSubsonicExtensions?f=json")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(json_body(&body)["subsonic-response"]["status"], "failed");
}

#[tokio::test]
async fn extensions_are_listed_without_credentials_when_enabled() {
    let fx = Fixture::new("subsonic-extensions").await;
    let (_, body) = send(
        fx.app.clone(),
        Request::builder()
            .uri("/rest/getOpenSubsonicExtensions.view?f=json")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let root = &json_body(&body)["subsonic-response"];
    assert_eq!(root["status"], "ok");
    assert_eq!(root["openSubsonicExtensions"][0]["versions"][0], 1);
    // Everything else still needs credentials.
    let (_, body) = send(
        fx.app.clone(),
        Request::builder()
            .uri("/rest/getArtists?f=json")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(json_body(&body)["subsonic-response"]["error"]["code"], 10);
}

#[tokio::test]
async fn credentials_api_enforces_rules_and_reports_status() {
    let fx = Fixture::new("subsonic-creds").await;
    for (pw, rule) in [("short!", "too_short"), ("NoSymbols1", "needs_symbol")] {
        let (status, body) = fx
            .put_password(&fx.cookie, "/api/subsonic/credentials/password", pw)
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json_body(&body)["rule"], rule);
    }
    let (status, body) = send(
        fx.app.clone(),
        Request::builder()
            .uri("/api/subsonic/credentials")
            .header("cookie", fx.cookie.clone())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let info = json_body(&body);
    assert_eq!(info["enabled"], true);
    assert_eq!(info["hasPassword"], true);
    assert!(info.get("password").is_none());
    assert!(info["serverUrls"].is_array());

    // Wrong password is rejected; the right one works; clearing disconnects.
    let (_, body) = send(
        fx.app.clone(),
        Request::builder()
            .uri("/rest/ping?u=u1&p=nope&f=json")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(json_body(&body)["subsonic-response"]["error"]["code"], 40);
    assert_eq!(fx.rest("ping", "").await["status"], "ok");
    let (status, _) = send(
        fx.app.clone(),
        Request::builder()
            .method("DELETE")
            .uri("/api/subsonic/credentials/password")
            .header("cookie", fx.cookie.clone())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fx.rest("ping", "").await["error"]["code"], 40);
}

#[tokio::test]
async fn admin_can_set_others_password_but_users_cannot() {
    let fx = Fixture::new("subsonic-admin").await;
    let admin = seed_admin_session(&fx.pool, "boss");
    let (status, _) = fx
        .put_password(
            &fx.cookie,
            "/api/admin/users/boss/subsonic-password",
            "Hijack!123",
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = fx
        .put_password(
            &admin,
            "/api/admin/users/u1/subsonic-password",
            "Reset!2026",
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = fx
        .put_password(
            &admin,
            "/api/admin/users/ghost/subsonic-password",
            "Reset!2026",
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, body) = send(
        fx.app.clone(),
        Request::builder()
            .uri("/rest/ping?u=u1&p=Reset!2026&f=json")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(json_body(&body)["subsonic-response"]["status"], "ok");
    let (status, body) = send(
        fx.app.clone(),
        Request::builder()
            .uri("/api/admin/users")
            .header("cookie", admin)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let users = json_body(&body);
    let u1 = users
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["id"] == "u1")
        .unwrap()
        .clone();
    assert_eq!(u1["hasSubsonicPassword"], true);
}

#[tokio::test]
async fn system_and_browsing_methods() {
    let fx = Fixture::new("subsonic-browse").await;
    let ping = fx.rest("ping", "").await;
    assert_eq!(ping["version"], "1.16.1");
    assert_eq!(ping["type"], "boogiebox");
    assert_eq!(ping["openSubsonic"], true);
    assert_eq!(fx.rest("getLicense", "").await["license"]["valid"], true);
    assert_eq!(
        fx.rest("getOpenSubsonicExtensions", "").await["openSubsonicExtensions"][0]["name"],
        "formPost"
    );

    let folders = fx.rest("getMusicFolders", "").await;
    assert_eq!(folders["musicFolders"]["musicFolder"][0]["id"], 1);
    assert_eq!(folders["musicFolders"]["musicFolder"][0]["name"], "Music");

    let user = fx.rest("getUser", "username=u1").await;
    assert_eq!(user["user"]["streamRole"], true);
    assert_eq!(user["user"]["folder"][0], 1);
    assert_eq!(
        fx.rest("getUser", "username=someoneelse").await["error"]["code"],
        50
    );

    let artists = fx.rest("getArtists", "").await;
    let index = &artists["artists"]["index"][0];
    assert_eq!(index["name"], "B"); // "The Band" sorts under B
    assert_eq!(index["artist"][0]["id"], "ar1");
    assert_eq!(index["artist"][0]["albumCount"], 1);
    assert_eq!(
        fx.rest("getArtists", "musicFolderId=9").await["error"]["code"],
        70
    );
    assert_eq!(
        fx.rest("getIndexes", "").await["indexes"]["lastModified"],
        1_777_593_600_000i64
    );
    assert!(
        fx.rest("getIndexes", "ifModifiedSince=1777593600000").await["indexes"]["index"].is_null()
    );

    let artist = fx.rest("getArtist", "id=ar1").await;
    assert_eq!(artist["artist"]["album"][0]["id"], "al1");
    assert_eq!(fx.rest("getArtist", "id=nope").await["error"]["code"], 70);
    assert_eq!(fx.rest("getArtist", "").await["error"]["code"], 10);

    let album = fx.rest("getAlbum", "id=al1").await["album"].clone();
    assert_eq!(album["songCount"], 3);
    let song = &album["song"][0];
    assert_eq!(song["id"], "t1");
    assert_eq!(song["coverArt"], "al-al1");
    assert_eq!(song["path"], "The Band/Album/01 - Opening.mp3");
    assert!(!album
        .to_string()
        .contains(&fx.lib_dir.to_string_lossy().replace('\\', "\\\\")));

    assert_eq!(fx.rest("getSong", "id=t2").await["song"]["artist"], "Guest");
    let genres = fx.rest("getGenres", "").await;
    let names: Vec<String> = genres["genres"]["genre"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["value"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["Pop", "Rock"]);

    let dir = fx.rest("getMusicDirectory", "id=ar1").await;
    assert_eq!(dir["directory"]["child"][0]["isDir"], true);
    let dir = fx.rest("getMusicDirectory", "id=al1").await;
    assert_eq!(dir["directory"]["child"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn lists_search_and_playlists() {
    let fx = Fixture::new("subsonic-lists").await;
    let list = fx.rest("getAlbumList2", "type=newest&size=5").await;
    assert_eq!(list["albumList2"]["album"][0]["id"], "al1");
    assert_eq!(
        fx.rest("getAlbumList", "type=alphabeticalByName").await["albumList"]["album"][0]["isDir"],
        true
    );
    assert_eq!(
        fx.rest("getAlbumList2", "type=byYear").await["error"]["code"],
        10
    );
    assert_eq!(
        fx.rest("getAlbumList2", "type=weird").await["error"]["code"],
        0
    );
    assert_eq!(
        fx.rest("getRandomSongs", "size=2").await["randomSongs"]["song"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        fx.rest("getSongsByGenre", "genre=Pop").await["songsByGenre"]["song"][0]["id"],
        "t2"
    );

    // Sync shape: empty query pages through every song in a stable order.
    let page1 = fx
        .rest(
            "search3",
            "query=%22%22&songCount=2&songOffset=0&artistCount=0&albumCount=0",
        )
        .await;
    let page2 = fx.rest("search3", "query=&songCount=2&songOffset=2").await;
    let ids: Vec<String> = page1["searchResult3"]["song"]
        .as_array()
        .unwrap()
        .iter()
        .chain(page2["searchResult3"]["song"].as_array().unwrap())
        .map(|s| s["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids, ["t1", "t2", "t3"]);
    assert!(page1["searchResult3"]["artist"].is_null());
    assert_eq!(page2["searchResult3"]["album"][0]["id"], "al1");
    assert_eq!(
        fx.rest("search2", "query=band").await["searchResult2"]["artist"][0]["id"],
        "ar1"
    );

    assert!(fx.rest("getStarred2", "").await["starred2"].is_object());
    assert!(fx.rest("getNowPlaying", "").await["nowPlaying"].is_object());

    {
        let conn = fx.pool.lock().unwrap();
        conn.execute_batch(
            "INSERT INTO users(id, username, role) VALUES ('u2', 'other', 'user');
             INSERT INTO playlists(id, user_id, name, description) VALUES ('p1', 'u1', 'Mine', 'desc'), ('p2', 'u2', 'Theirs', '');
             INSERT INTO playlist_tracks(id, playlist_id, track_id, position) VALUES ('pt1', 'p1', 't2', 0), ('pt2', 'p1', 't1', 1);",
        )
        .unwrap();
    }
    let lists = fx.rest("getPlaylists", "").await;
    let pl = &lists["playlists"]["playlist"];
    assert_eq!(pl.as_array().unwrap().len(), 1);
    assert_eq!(pl[0]["owner"], "u1");
    assert_eq!(pl[0]["songCount"], 2);
    assert_eq!(pl[0]["coverArt"], "al-al1");
    let one = fx.rest("getPlaylist", "id=p1").await;
    assert_eq!(one["playlist"]["entry"][0]["id"], "t2");
    assert_eq!(fx.rest("getPlaylist", "id=p2").await["error"]["code"], 70);
    assert_eq!(
        fx.rest("getPlaylists", "username=other").await["error"]["code"],
        50
    );
}

#[tokio::test]
async fn streaming_download_scrobble_and_cover_art() {
    let fx = Fixture::new("subsonic-media").await;
    let (status, headers, body) = fx.raw(&format!("/rest/stream?{}&id=t1", fx.auth())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "audio/mpeg");
    assert_eq!(body, b"0123456789abcdef");

    let (status, _, body) = send_full(
        fx.app.clone(),
        Request::builder()
            .uri(format!("/rest/stream.view?{}&id=t1", fx.auth()))
            .header(header::RANGE, "bytes=0-3")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(body, b"0123");

    let (status, headers, _) = fx.raw(&format!("/rest/download?{}&id=t2", fx.auth())).await;
    assert_eq!(status, StatusCode::OK);
    let disposition = headers[header::CONTENT_DISPOSITION].to_str().unwrap();
    assert!(disposition.contains("02 - Closing.mp3"), "{disposition}");

    // Transcodes (HEAD: headers only, no encoder spawned). 10 s at 128 kbps = 160000 bytes.
    let resp = tower::ServiceExt::oneshot(
        fx.app.clone(),
        Request::builder()
            .method("HEAD")
            .uri(format!("/rest/stream?{}&id=t1&maxBitRate=128", fx.auth()))
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()[header::CONTENT_TYPE], "audio/mpeg");
    assert_eq!(resp.headers()[header::CONTENT_LENGTH], "160000");
    let resp = tower::ServiceExt::oneshot(
        fx.app.clone(),
        Request::builder()
            .method("HEAD")
            .uri(format!("/rest/stream?{}&id=t1", fx.auth()))
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(resp.headers()[header::CONTENT_LENGTH], "16");

    // A track whose file sits outside its library is never served.
    let (_, _, body) = fx.raw(&format!("/rest/stream?{}&id=t3", fx.auth())).await;
    assert!(String::from_utf8_lossy(&body).contains("code=\"70\""));

    // POST form + scrobble with a client timestamp.
    let (status, body) = send(
        fx.app.clone(),
        Request::builder()
            .method("POST")
            .uri("/rest/scrobble")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(format!(
                "{}&f=json&id=t1&id=t2&time=1780000000000&time=1780000300000",
                fx.auth()
            )))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_body(&body)["subsonic-response"]["status"], "ok");
    // A resend of the same play is ignored.
    assert_eq!(
        fx.rest("scrobble", "id=t1&time=1780000005000").await["status"],
        "ok"
    );
    assert_eq!(
        fx.rest("scrobble", "id=t1&submission=false").await["status"],
        "ok"
    );
    assert_eq!(fx.rest("scrobble", "id=missing").await["error"]["code"], 70);
    {
        let conn = fx.pool.lock().unwrap();
        let history: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM play_history WHERE user_id = 'u1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(history, 2);
        let plays: i64 = conn
            .query_row("SELECT play_count FROM tracks WHERE id = 't1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(plays, 1);
    }
    let song = fx.rest("getSong", "id=t1").await;
    assert_eq!(song["song"]["played"], "2026-05-28T20:26:40Z");

    // No cached art in the test data folder → Subsonic "not found", never an HTML page.
    let (status, headers, body) = fx
        .raw(&format!(
            "/rest/getCoverArt?{}&id=al-al1&size=64",
            fx.auth()
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("text/xml"));
    assert!(String::from_utf8_lossy(&body).contains("code=\"70\""));
}

#[tokio::test]
async fn unknown_and_unsupported_methods_return_subsonic_errors() {
    let fx = Fixture::new("subsonic-unknown").await;
    assert_eq!(
        fx.rest("getPodcasts", "").await["error"]["message"],
        "Not supported by BoogieBox"
    );
    let unknown = fx.rest("doesNotExist", "").await;
    assert_eq!(unknown["error"]["code"], 0);
    let (_, headers, _) = fx.raw(&format!("/rest/doesNotExist?{}", fx.auth())).await;
    assert!(headers[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("text/xml"));
    let jsonp = send(
        fx.app.clone(),
        Request::builder()
            .uri(format!("/rest/ping?{}&f=jsonp&callback=cb", fx.auth()))
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .1;
    assert!(String::from_utf8(jsonp).unwrap().starts_with("cb({"));
}

// ── Phase 2 ──────────────────────────────────────────────────────────────────

fn data_folder(pool: &DbPool) -> std::path::PathBuf {
    let file: String = pool
        .lock()
        .unwrap()
        .query_row(
            "SELECT file FROM pragma_database_list WHERE name = 'main'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    std::path::PathBuf::from(file)
        .parent()
        .unwrap()
        .to_path_buf()
}

#[tokio::test]
async fn stars_and_ratings_round_trip() {
    let fx = Fixture::new("subsonic-stars").await;
    assert_eq!(
        fx.rest("star", "id=t1&albumId=al1&artistId=ar1").await["status"],
        "ok"
    );
    // A bare id resolves to an album when it is not a song.
    assert_eq!(fx.rest("star", "id=al1").await["status"], "ok");
    let starred = fx.rest("getStarred2", "").await["starred2"].clone();
    assert_eq!(starred["song"][0]["id"], "t1");
    assert_eq!(starred["album"][0]["id"], "al1");
    assert_eq!(starred["artist"][0]["id"], "ar1");
    assert!(fx.rest("getSong", "id=t1").await["song"]["starred"].is_string());
    assert_eq!(fx.rest("unstar", "id=t1").await["status"], "ok");
    assert!(fx.rest("getStarred2", "").await["starred2"]["song"].is_null());
    assert_eq!(fx.rest("star", "id=nope").await["error"]["code"], 70);
    assert_eq!(fx.rest("star", "albumId=ar1").await["error"]["code"], 70);
    assert_eq!(fx.rest("star", "").await["error"]["code"], 10);

    assert_eq!(fx.rest("setRating", "id=t1&rating=4").await["status"], "ok");
    assert_eq!(fx.rest("getSong", "id=t1").await["song"]["userRating"], 4);
    assert_eq!(
        fx.rest("setRating", "id=al1&rating=2").await["status"],
        "ok"
    );
    assert_eq!(
        fx.rest("getAlbum", "id=al1").await["album"]["userRating"],
        2
    );
    assert_eq!(fx.rest("setRating", "id=t1&rating=0").await["status"], "ok");
    assert!(fx.rest("getSong", "id=t1").await["song"]["userRating"].is_null());
    assert_eq!(
        fx.rest("setRating", "id=t1&rating=7").await["error"]["code"],
        0
    );
    assert_eq!(fx.rest("setRating", "id=t1").await["error"]["code"], 10);
    assert_eq!(
        fx.rest("setRating", "id=zz&rating=3").await["error"]["code"],
        70
    );
}

#[tokio::test]
async fn playlists_can_be_created_edited_and_deleted() {
    let fx = Fixture::new("subsonic-playlist-writes").await;
    let created = fx
        .rest("createPlaylist", "name=Road&songId=t1&songId=t2&songId=t1")
        .await;
    let pl = &created["playlist"];
    assert_eq!(pl["name"], "Road");
    assert_eq!(pl["songCount"], 2);
    let id = pl["id"].as_str().unwrap().to_owned();
    // BoogieBox names are unique per user; apps don't expect that.
    assert_eq!(
        fx.rest("createPlaylist", "name=Road").await["playlist"]["name"],
        "Road (2)"
    );

    let update = format!(
        "playlistId={id}&name=Renamed&comment=Long+drive&songIndexToRemove=0&songIdToAdd=t3"
    );
    assert_eq!(fx.rest("updatePlaylist", &update).await["status"], "ok");
    let pl = fx.rest("getPlaylist", &format!("id={id}")).await["playlist"].clone();
    assert_eq!(pl["name"], "Renamed");
    assert_eq!(pl["comment"], "Long drive");
    let entries: Vec<&str> = pl["entry"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert_eq!(entries, ["t2", "t3"]);

    // createPlaylist with playlistId replaces the entries.
    let replaced = fx
        .rest("createPlaylist", &format!("playlistId={id}&songId=t3"))
        .await;
    assert_eq!(replaced["playlist"]["songCount"], 1);
    assert_eq!(
        fx.rest("updatePlaylist", &format!("playlistId={id}&name=Road+(2)"))
            .await["error"]["code"],
        0
    );
    assert_eq!(
        fx.rest(
            "updatePlaylist",
            &format!("playlistId={id}&songIndexToRemove=x")
        )
        .await["error"]["code"],
        0
    );

    {
        let conn = fx.pool.lock().unwrap();
        conn.execute_batch(
            "INSERT INTO users(id, username, role) VALUES ('u2', 'other', 'user');
             INSERT INTO playlists(id, user_id, name) VALUES ('p2', 'u2', 'Theirs');",
        )
        .unwrap();
    }
    assert_eq!(
        fx.rest("updatePlaylist", "playlistId=p2&name=Mine").await["error"]["code"],
        70
    );
    assert_eq!(
        fx.rest("deletePlaylist", "id=p2").await["error"]["code"],
        70
    );
    assert_eq!(
        fx.rest("deletePlaylist", &format!("id={id}")).await["status"],
        "ok"
    );
    assert_eq!(
        fx.rest("getPlaylist", &format!("id={id}")).await["error"]["code"],
        70
    );
    assert_eq!(fx.rest("createPlaylist", "").await["error"]["code"], 10);
}

#[tokio::test]
async fn boogiemix_playlist_lists_and_streams_own_mixes_only() {
    let fx = Fixture::new("subsonic-boogiemix").await;
    let mix_dir = data_folder(&fx.pool).join("mix-outputs");
    std::fs::create_dir_all(&mix_dir).unwrap();
    let inside = mix_dir.join("friday.mp3");
    std::fs::write(&inside, b"mix bytes").unwrap();
    let outside = std::env::temp_dir().join(format!("bb-mix-outside-{}.mp3", uuid::Uuid::now_v7()));
    std::fs::write(&outside, b"nope").unwrap();
    {
        let conn = fx.pool.lock().unwrap();
        conn.execute_batch("INSERT INTO users(id, username, role) VALUES ('u2', 'other', 'user');")
            .unwrap();
        conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
        for (id, user, path, name, created) in [
            (
                "m1",
                "u1",
                inside.to_string_lossy().into_owned(),
                "Friday",
                "2026-01-01 00:00:00",
            ),
            (
                "m2",
                "u1",
                outside.to_string_lossy().into_owned(),
                "Escaped",
                "2026-01-02 00:00:00",
            ),
            (
                "m3",
                "u2",
                inside.to_string_lossy().into_owned(),
                "Theirs",
                "2026-01-03 00:00:00",
            ),
        ] {
            conn.execute(
                "INSERT INTO mix_outputs(id, job_id, playlist_id, user_id, file_path, file_name, name, cover_album_ids,
                                         source_playlist_name, duration_sec, file_size_bytes, format, created_at)
                 VALUES (?1, ?1, NULL, ?2, ?3, 'f.mp3', ?4, 'al1', 'Party', 1800, 9, 'mp3', ?5)",
                params![id, user, path, name, created],
            )
            .unwrap();
        }
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    }

    let lists = fx.rest("getPlaylists", "").await;
    let mix_pl = lists["playlists"]["playlist"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "bm-mixes")
        .unwrap()
        .clone();
    assert_eq!(mix_pl["name"], "BoogieMix");
    assert_eq!(mix_pl["readonly"], true);
    assert_eq!(mix_pl["songCount"], 2);
    assert_eq!(mix_pl["coverArt"], "al-al1");

    let entries = fx.rest("getPlaylist", "id=bm-mixes").await["playlist"]["entry"].clone();
    assert_eq!(entries[0]["id"], "mx-m2");
    assert_eq!(entries[1]["id"], "mx-m1");
    assert_eq!(entries[1]["album"], "BoogieMix");
    assert_eq!(entries[1]["comment"], "Mixed from Party");
    assert_eq!(entries[1]["path"], "BoogieMix/Friday.mp3");
    assert_eq!(
        fx.rest("getSong", "id=mx-m1").await["song"]["title"],
        "Friday"
    );
    assert_eq!(fx.rest("getSong", "id=mx-m3").await["error"]["code"], 70);

    let (status, _, body) = fx
        .raw(&format!("/rest/stream?{}&id=mx-m1", fx.auth()))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, b"mix bytes");
    let (_, headers, _) = fx
        .raw(&format!("/rest/download?{}&id=mx-m1", fx.auth()))
        .await;
    assert!(headers[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .contains("Friday.mp3"));
    let (_, _, body) = fx
        .raw(&format!("/rest/stream?{}&id=mx-m2", fx.auth()))
        .await;
    assert!(
        String::from_utf8_lossy(&body).contains("code=\"70\""),
        "outside the mix folder"
    );
    let (_, _, body) = fx
        .raw(&format!("/rest/stream?{}&id=mx-m3", fx.auth()))
        .await;
    assert!(
        String::from_utf8_lossy(&body).contains("code=\"70\""),
        "another user's mix"
    );

    assert_eq!(fx.rest("scrobble", "id=mx-m1").await["status"], "ok");
    let history: i64 = fx
        .pool
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM play_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(history, 0, "mixes are not counted as plays");
    assert_eq!(
        fx.rest("updatePlaylist", "playlistId=bm-mixes&name=x")
            .await["error"]["code"],
        50
    );
    assert_eq!(
        fx.rest("deletePlaylist", "id=bm-mixes").await["error"]["code"],
        50
    );
    assert_eq!(
        fx.rest("createPlaylist", "playlistId=bm-mixes&songId=t1")
            .await["error"]["code"],
        50
    );
    assert_eq!(fx.rest("star", "id=mx-m1").await["error"]["code"], 0);
    assert_eq!(
        fx.rest("setRating", "id=mx-m1&rating=3").await["error"]["code"],
        0
    );
    let _ = std::fs::remove_file(&outside);
}

#[tokio::test]
async fn lyrics_info_similar_top_and_now_playing() {
    let fx = Fixture::new("subsonic-info").await;
    {
        let conn = fx.pool.lock().unwrap();
        conn.execute_batch(
            r#"INSERT INTO lyrics_cache(track_id, artist, title, lyrics, synced_lyrics, source) VALUES
                 ('t1', 'The Band', 'Opening', 'Hello
World', '[{"time":1.5,"text":"Hello"},{"time":3,"text":"World"}]', 'lrclib');
               UPDATE artists SET description = 'A band.' WHERE id = 'ar1';
               UPDATE albums SET description = 'Notes.' WHERE id = 'al1';
               UPDATE tracks SET play_count = 9 WHERE id = 't3';"#,
        )
        .unwrap();
    }
    let lyrics = fx.rest("getLyrics", "artist=the+band&title=opening").await;
    assert_eq!(lyrics["lyrics"]["value"], "Hello\nWorld");
    assert!(fx.rest("getLyrics", "artist=x&title=y").await["lyrics"]["value"].is_null());

    let structured =
        fx.rest("getLyricsBySongId", "id=t1").await["lyricsList"]["structuredLyrics"][0].clone();
    assert_eq!(structured["synced"], true);
    assert_eq!(structured["displayTitle"], "Opening");
    assert_eq!(structured["line"][0]["start"], 1500);
    assert_eq!(structured["line"][1]["value"], "World");
    assert_eq!(
        fx.rest("getLyricsBySongId", "id=nope").await["error"]["code"],
        70
    );
    let ext = fx.rest("getOpenSubsonicExtensions", "").await;
    assert!(ext["openSubsonicExtensions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["name"] == "songLyrics"));

    let info = fx.rest("getArtistInfo2", "id=ar1").await["artistInfo2"].clone();
    assert_eq!(info["biography"], "A band.");
    assert_eq!(
        fx.rest("getArtistInfo", "id=ar1&count=0").await["artistInfo"]["biography"],
        "A band."
    );
    assert_eq!(
        fx.rest("getArtistInfo2", "id=zz").await["error"]["code"],
        70
    );
    assert_eq!(
        fx.rest("getAlbumInfo2", "id=al1").await["albumInfo"]["notes"],
        "Notes."
    );

    // Instant Mix: an Artist Radio queue seeded from the song's artist (offline: no provider ids in the fixture).
    let similar = fx.rest("getSimilarSongs2", "id=t1&count=5").await;
    assert!(similar["similarSongs2"].is_object());
    let similar_ids: Vec<&str> = similar["similarSongs2"]["song"]
        .as_array()
        .map(|a| a.iter().filter_map(|s| s["id"].as_str()).collect())
        .unwrap_or_default();
    assert!(similar_ids.iter().all(|id| ["t1", "t2", "t3"].contains(id)));
    assert!(fx.rest("getSimilarSongs", "id=al1").await["similarSongs"].is_object());
    assert_eq!(
        fx.rest("getSimilarSongs2", "id=zz").await["error"]["code"],
        70
    );

    let top = fx.rest("getTopSongs", "artist=The+Band&count=2").await["topSongs"]["song"].clone();
    assert_eq!(top[0]["id"], "t3");
    assert!(fx.rest("getTopSongs", "artist=Nobody").await["topSongs"].is_object());

    assert!(fx.rest("getNowPlaying", "").await["nowPlaying"]["entry"].is_null());
    assert_eq!(
        fx.rest("scrobble", "id=t2&submission=false").await["status"],
        "ok"
    );
    let now = fx.rest("getNowPlaying", "").await["nowPlaying"]["entry"][0].clone();
    assert_eq!(now["id"], "t2");
    assert_eq!(now["username"], "u1");
    assert_eq!(now["playerName"], "test");
    assert_eq!(now["minutesAgo"], 0);
}

// ── Phase 2.5 ────────────────────────────────────────────────────────────────

async fn rest_as(fx: &Fixture, user: &str, method: &str, query: &str) -> Json {
    let (status, body) = send(
        fx.app.clone(),
        Request::builder()
            .uri(format!(
                "/rest/{method}?u={user}&p={}&v=1.16.1&c=test&f=json&{query}",
                urlencode(PASSWORD)
            ))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    json_body(&body)["subsonic-response"].clone()
}

fn list_len(value: &Json) -> usize {
    value.as_array().map_or(0, Vec::len)
}

fn queued(pool: &DbPool) -> i64 {
    pool.lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM scrobble_queue", [], |r| r.get(0))
        .unwrap()
}

#[tokio::test]
async fn per_user_play_counts_and_web_now_playing() {
    let fx = Fixture::new("subsonic-p25-plays").await;
    assert_eq!(fx.rest("getSong", "id=t1").await["song"]["playCount"], 0);
    assert_eq!(
        fx.rest("scrobble", "id=t1&submission=true").await["status"],
        "ok"
    );
    let song = fx.rest("getSong", "id=t1").await["song"].clone();
    assert_eq!(song["playCount"], 1);
    assert!(song["played"].is_string());
    assert_eq!(fx.rest("getAlbum", "id=al1").await["album"]["playCount"], 1);
    assert_eq!(
        fx.rest("getAlbumList2", "type=frequent").await["albumList2"]["album"][0]["id"],
        "al1"
    );

    // Another user's plays don't show up in u1's counts.
    let admin = seed_admin_session(&fx.pool, "a1");
    let (status, _) = fx
        .put_password(&admin, "/api/subsonic/credentials/password", PASSWORD)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        rest_as(&fx, "a1", "getSong", "id=t1").await["song"]["playCount"],
        0
    );

    // The web player's plays appear in getNowPlaying as "BoogieBox".
    let (status, _) = send(
        fx.app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/tracks/t2/played")
            .header("cookie", &fx.cookie)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let now = fx.rest("getNowPlaying", "").await["nowPlaying"]["entry"][0].clone();
    assert_eq!(now["id"], "t2");
    assert_eq!(now["playerName"], "BoogieBox");
    assert_eq!(now["playCount"], 1, "the web play counts too");

    // A finished web play (threshold reached) is queued for connected services only.
    let scrobble = |track: &str| {
        Request::builder()
            .method("POST")
            .uri(format!("/api/tracks/{track}/scrobble"))
            .header("cookie", &fx.cookie)
            .header("content-type", "application/json")
            .body(Body::from(r#"{"startedAt": 1}"#))
            .unwrap()
    };
    assert_eq!(send(fx.app.clone(), scrobble("t2")).await.0, StatusCode::OK);
    assert_eq!(
        send(fx.app.clone(), scrobble("nope")).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(queued(&fx.pool), 0, "no account connected");
    boogiebox_db::scrobble::upsert_account(
        &fx.pool.lock().unwrap(),
        "u1",
        boogiebox_db::scrobble::ScrobbleService::ListenBrainz,
        b"x",
        b"y",
        None,
    )
    .unwrap();
    assert_eq!(send(fx.app.clone(), scrobble("t2")).await.0, StatusCode::OK);
    assert_eq!(queued(&fx.pool), 1);
    // An app's scrobble is queued once; a resent (duplicate) one is not queued again.
    let at = (crate::plays::unix_now() - 1000) * 1000;
    let play = format!("id=t1&time={at}");
    assert_eq!(fx.rest("scrobble", &play).await["status"], "ok");
    assert_eq!(queued(&fx.pool), 2);
    assert_eq!(fx.rest("scrobble", &play).await["status"], "ok");
    assert_eq!(queued(&fx.pool), 2);
}

#[tokio::test]
async fn internet_radio_and_jukebox() {
    let fx = Fixture::new("subsonic-p25-radio").await;
    let stations = |v: Json| v["internetRadioStations"]["internetRadioStation"].clone();
    assert_eq!(
        list_len(&stations(fx.rest("getInternetRadioStations", "").await)),
        0
    );
    let create = "name=Jazz&streamUrl=http%3A%2F%2Fjazz.example%2Flive&homepageUrl=https%3A%2F%2Fjazz.example";
    assert_eq!(
        fx.rest("createInternetRadioStation", create).await["error"]["code"],
        50
    );

    let admin = seed_admin_session(&fx.pool, "a1");
    fx.put_password(&admin, "/api/subsonic/credentials/password", PASSWORD)
        .await;
    assert_eq!(
        rest_as(&fx, "a1", "createInternetRadioStation", create).await["status"],
        "ok"
    );
    let bad = "name=Bad&streamUrl=file%3A%2F%2F%2Fetc";
    assert_eq!(
        rest_as(&fx, "a1", "createInternetRadioStation", bad).await["error"]["code"],
        0
    );
    let list = stations(fx.rest("getInternetRadioStations", "").await);
    assert_eq!(list_len(&list), 1);
    assert_eq!(list[0]["streamUrl"], "http://jazz.example/live");
    assert_eq!(list[0]["homePageUrl"], "https://jazz.example");
    let id = list[0]["id"].as_str().unwrap().to_owned();
    let update = format!("id={id}&name=Jazz+24&streamUrl=http%3A%2F%2Fjazz.example%2F24");
    assert_eq!(
        rest_as(&fx, "a1", "updateInternetRadioStation", &update).await["status"],
        "ok"
    );
    assert_eq!(
        stations(fx.rest("getInternetRadioStations", "").await)[0]["name"],
        "Jazz 24"
    );
    let delete = format!("id={id}");
    assert_eq!(
        fx.rest("deleteInternetRadioStation", &delete).await["error"]["code"],
        50
    );
    assert_eq!(
        rest_as(&fx, "a1", "deleteInternetRadioStation", &delete).await["status"],
        "ok"
    );
    assert_eq!(
        rest_as(&fx, "a1", "deleteInternetRadioStation", &delete).await["error"]["code"],
        70
    );

    // Jukebox: off until a Play On speaker is chosen and Play On is on.
    assert_eq!(
        fx.rest("getUser", "username=u1").await["user"]["jukeboxRole"],
        false
    );
    let off = fx.rest("jukeboxControl", "action=get").await;
    assert_eq!(
        off["error"]["message"],
        "The jukebox speaker is not available"
    );
}

#[tokio::test]
async fn opus_and_aac_streams_are_unsized() {
    let fx = Fixture::new("subsonic-p25-codecs").await;
    let head = |query: &str| {
        Request::builder()
            .method("HEAD")
            .uri(format!("/rest/stream?{}&id=t1&{query}", fx.auth()))
            .body(Body::empty())
            .unwrap()
    };
    let (status, headers, _) = send_full(fx.app.clone(), head("format=aac&maxBitRate=128")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "audio/aac");
    assert_eq!(headers[header::ACCEPT_RANGES], "none");
    let (status, headers, _) = send_full(fx.app.clone(), head("format=opus")).await;
    assert_eq!(status, StatusCode::OK);
    let ct = headers[header::CONTENT_TYPE].to_str().unwrap().to_owned();
    assert!(
        ct == "audio/ogg" || ct == "audio/mpeg",
        "opus, or mp3 without libopus: {ct}"
    );
}
