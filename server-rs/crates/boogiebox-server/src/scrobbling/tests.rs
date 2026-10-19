use std::sync::{Arc, RwLock};

use boogiebox_db::scrobble::{self as qdb, ScrobbleService};
use serde_json::json;
use wiremock::{
    matchers::{body_partial_json, body_string_contains, method, path},
    Mock, MockServer, ResponseTemplate,
};

use super::*;
use crate::test_support::{build_test_state, temp_db_path};

fn env(prefix: &str) -> PlayEnv {
    let state = Arc::new(RwLock::new(build_test_state(&temp_db_path(prefix))));
    let env = PlayEnv::from_state(&state).unwrap();
    let conn = env.db.lock().unwrap();
    conn.execute_batch(
        "INSERT INTO users(id, username, role) VALUES ('u1', 'alice', 'user'), ('u2', 'bob', 'user');
         INSERT INTO libraries(id, path, name) VALUES ('lib1', '/m', 'Music');
         INSERT INTO artists(id, name) VALUES ('ar1', 'The Band');
         INSERT INTO albums(id, title, artist_id) VALUES ('al1', 'First', 'ar1');
         INSERT INTO tracks(id, library_id, artist_id, album_id, title, file_path, duration) VALUES
           ('t1', 'lib1', 'ar1', 'al1', 'Opening', '/m/1.flac', 200);
         INSERT OR REPLACE INTO settings(key, value) VALUES ('lastfmKey', 'app-key'), ('lastfmSecret', 'app-secret');",
    )
    .unwrap();
    drop(conn);
    env
}

fn connect(env: &PlayEnv, user: &str, service: ScrobbleService, secret: &str) {
    let key = env.subsonic.key(env.db_folder.as_deref(), &env.db).unwrap();
    let (enc, nonce) = seal(&key, user, service, secret).unwrap();
    let conn = env.db.lock().unwrap();
    qdb::upsert_account(&conn, user, service, &enc, &nonce, Some("name")).unwrap();
}

fn queued(env: &PlayEnv) -> Vec<(String, String, i64)> {
    let conn = env.db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT user_id, service, attempts FROM scrobble_queue ORDER BY id")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn endpoints(lastfm: &MockServer, lb: &MockServer) -> Endpoints {
    Endpoints {
        lastfm: lastfm.uri(),
        listenbrainz: lb.uri(),
    }
}

#[test]
fn backoff_doubles_up_to_six_hours() {
    assert_eq!(backoff_secs(0), 60);
    assert_eq!(backoff_secs(1), 120);
    assert_eq!(backoff_secs(5), 1920);
    assert_eq!(backoff_secs(19), MAX_BACKOFF_SECS);
    assert_eq!(backoff_secs(-3), 60);
}

#[test]
fn origins_must_be_bare_http_hosts() {
    assert!(valid_origin("http://box.local:3000"));
    assert!(valid_origin("https://music.example.com"));
    assert!(!valid_origin("http://box.local/path"));
    assert!(!valid_origin("http://evil@box.local"));
    assert!(!valid_origin("javascript:alert(1)"));
    assert!(!valid_origin("box.local"));
}

#[test]
fn secrets_are_bound_to_user_and_service() {
    let key = [9u8; 32];
    let (enc, nonce) = seal(&key, "u1", ScrobbleService::LastFm, "sk-123").unwrap();
    let account = |user: &str, service| qdb::ScrobbleAccount {
        user_id: user.into(),
        service,
        secret_enc: enc.clone(),
        secret_nonce: nonce.clone(),
        account_name: None,
        needs_reconnect: false,
    };
    assert_eq!(
        open(&key, &account("u1", ScrobbleService::LastFm)).as_deref(),
        Some("sk-123")
    );
    assert_eq!(open(&key, &account("u2", ScrobbleService::LastFm)), None);
    assert_eq!(
        open(&key, &account("u1", ScrobbleService::ListenBrainz)),
        None
    );
}

#[tokio::test]
async fn plays_queue_only_for_connected_accounts() {
    let env = env("scrobble-enqueue");
    {
        let conn = env.db.lock().unwrap();
        assert_eq!(
            enqueue_play(&conn, "u1", "t1", 100, 100),
            0,
            "no account yet"
        );
        assert!(lastfm_app(&conn).is_some());
    }
    connect(&env, "u1", ScrobbleService::LastFm, "sk");
    connect(&env, "u1", ScrobbleService::ListenBrainz, "lb-token-1234");
    let conn = env.db.lock().unwrap();
    assert_eq!(enqueue_play(&conn, "u1", "t1", 100, 100), 2);
    assert_eq!(enqueue_play(&conn, "u1", "missing", 100, 100), 0);
    assert_eq!(enqueue_play(&conn, "u2", "t1", 100, 100), 0);
}

#[tokio::test]
async fn delivery_sends_defers_and_flags_reconnects() {
    let env = env("scrobble-deliver");
    let lastfm = MockServer::start().await;
    let lb = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_string_contains("method=track.scrobble"))
        .and(body_string_contains("sk=good-sk"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"scrobbles": {}})))
        .mount(&lastfm)
        .await;
    Mock::given(method("POST"))
        .and(path("/1/submit-listens"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&lb)
        .await;
    connect(&env, "u1", ScrobbleService::LastFm, "good-sk");
    connect(&env, "u1", ScrobbleService::ListenBrainz, "lb-token-1234");
    {
        let conn = env.db.lock().unwrap();
        assert_eq!(
            enqueue_play(&conn, "u1", "t1", unix_now() - 300, unix_now()),
            2
        );
        assert_eq!(
            enqueue_play(&conn, "u1", "t1", unix_now() - 100, unix_now()),
            2
        );
    }
    let report = deliver_due(&env, &endpoints(&lastfm, &lb)).await;
    assert_eq!(report.sent, 2, "{report:?}");
    assert_eq!(report.deferred, 2);
    let left = queued(&env);
    assert_eq!(left.len(), 2);
    assert!(left.iter().all(|(_, s, a)| s == "listenbrainz" && *a == 1));
    {
        let conn = env.db.lock().unwrap();
        let accounts = qdb::list_accounts(&conn, "u1").unwrap();
        assert!(accounts
            .iter()
            .any(|a| a.service == ScrobbleService::LastFm && a.last_scrobble_at.is_some()));
    }
    // Deferred plays are not due yet.
    assert_eq!(
        deliver_due(&env, &endpoints(&lastfm, &lb)).await,
        PassReport::default()
    );

    // An expired Last.fm session flags the account and keeps its plays.
    let lastfm2 = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"error": 9, "message": "Invalid session key"})),
        )
        .mount(&lastfm2)
        .await;
    {
        let conn = env.db.lock().unwrap();
        qdb::enqueue(
            &conn,
            "u1",
            ScrobbleService::LastFm,
            &qdb::track_meta(&conn, "t1").unwrap().unwrap(),
            unix_now() - 50,
            unix_now(),
        )
        .unwrap();
    }
    let report = deliver_due(&env, &endpoints(&lastfm2, &lb)).await;
    assert_eq!(report.reconnect, 1);
    let active = qdb::active_services(&env.db.lock().unwrap(), "u1").unwrap();
    assert_eq!(active, [ScrobbleService::ListenBrainz]);
    assert_eq!(
        queued(&env)
            .iter()
            .filter(|(_, s, _)| s == "lastfm")
            .count(),
        1
    );
}

#[tokio::test]
async fn rejected_batches_are_dropped() {
    let env = env("scrobble-reject");
    let lastfm = MockServer::start().await;
    let lb = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/1/submit-listens"))
        .respond_with(ResponseTemplate::new(400))
        .mount(&lb)
        .await;
    connect(&env, "u1", ScrobbleService::ListenBrainz, "lb-token-1234");
    {
        let conn = env.db.lock().unwrap();
        enqueue_play(&conn, "u1", "t1", unix_now() - 300, unix_now());
    }
    let report = deliver_due(&env, &endpoints(&lastfm, &lb)).await;
    assert_eq!(report.dropped, 1);
    assert!(queued(&env).is_empty());
}

#[tokio::test]
async fn now_playing_reaches_every_connected_service() {
    let env = env("scrobble-nowplaying");
    let lastfm = MockServer::start().await;
    let lb = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_string_contains("method=track.updateNowPlaying"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"nowplaying": {}})))
        .expect(1)
        .mount(&lastfm)
        .await;
    Mock::given(method("POST"))
        .and(path("/1/submit-listens"))
        .and(body_partial_json(json!({"listen_type": "playing_now"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})))
        .expect(1)
        .mount(&lb)
        .await;
    // No accounts: nothing is sent.
    forward_now_playing(&env, &endpoints(&lastfm, &lb), "u1", "t1").await;
    connect(&env, "u1", ScrobbleService::LastFm, "sk");
    connect(&env, "u1", ScrobbleService::ListenBrainz, "lb-token-1234");
    forward_now_playing(&env, &endpoints(&lastfm, &lb), "u1", "t1").await;
    forward_now_playing(&env, &endpoints(&lastfm, &lb), "u1", "missing").await;
}

#[tokio::test]
async fn connecting_listenbrainz_validates_the_token() {
    let env = env("scrobble-connect-lb");
    let lastfm = MockServer::start().await;
    let lb = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/1/validate-token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"valid": true, "user_name": "lb_alice"})),
        )
        .mount(&lb)
        .await;
    let ep = endpoints(&lastfm, &lb);
    assert_eq!(
        connect_listenbrainz(&env, &ep, "u1", "bad token").await,
        Err(ConnectError::Rejected)
    );
    assert_eq!(
        connect_listenbrainz(&env, &ep, "u1", " lb-token-1234 ")
            .await
            .unwrap(),
        "lb_alice"
    );
    let conn = env.db.lock().unwrap();
    let account = qdb::get_account(&conn, "u1", ScrobbleService::ListenBrainz)
        .unwrap()
        .unwrap();
    assert_eq!(account.account_name.as_deref(), Some("lb_alice"));
    assert_ne!(
        account.secret_enc, b"lb-token-1234",
        "the token is stored encrypted"
    );
}

#[tokio::test]
async fn lastfm_sign_in_uses_a_one_time_state() {
    let env = env("scrobble-connect-lastfm");
    let lastfm = MockServer::start().await;
    let lb = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_string_contains("method=auth.getSession"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"session": {"name": "fm_alice", "key": "sk-9"}})),
        )
        .mount(&lastfm)
        .await;
    let ep = endpoints(&lastfm, &lb);
    assert_eq!(
        begin_lastfm_connect(&env, "u1", "http://box/path").await,
        Err(ConnectError::Rejected)
    );
    let url = begin_lastfm_connect(&env, "u1", "http://box.local:3000/")
        .await
        .unwrap();
    assert!(url.starts_with("https://www.last.fm/api/auth/?api_key=app-key&cb=http%3A%2F%2Fbox.local%3A3000%2Fapi%2Fscrobble%2Flastfm%2Fcallback%3Fstate%3D"));
    let state = url.rsplit("state%3D").next().unwrap().to_owned();
    assert_eq!(
        finish_lastfm_connect(&env, &ep, "forged", "tok").await,
        Err(ConnectError::Expired)
    );
    assert_eq!(
        finish_lastfm_connect(&env, &ep, &state, "tok123")
            .await
            .unwrap(),
        "fm_alice"
    );
    assert_eq!(
        finish_lastfm_connect(&env, &ep, &state, "tok123").await,
        Err(ConnectError::Expired),
        "a state works once"
    );
    {
        let conn = env.db.lock().unwrap();
        assert!(qdb::get_account(&conn, "u1", ScrobbleService::LastFm)
            .unwrap()
            .is_some());
        conn.execute("DELETE FROM settings WHERE key = 'lastfmSecret'", [])
            .unwrap();
    }
    assert_eq!(
        begin_lastfm_connect(&env, "u1", "http://box.local:3000").await,
        Err(ConnectError::NotConfigured)
    );
    assert!(!ConnectError::NotConfigured.message().is_empty());
}
