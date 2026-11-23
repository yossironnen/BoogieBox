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
           ('t1', 'lib1', 'ar1', 'al1', 'Opening', '/m/1.flac', 200);",
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

fn endpoints(librefm: &MockServer, lb: &MockServer) -> Endpoints {
    Endpoints {
        librefm: librefm.uri(),
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
    let (enc, nonce) = seal(&key, "u1", ScrobbleService::LibreFm, "sk-123").unwrap();
    let account = |user: &str, service| qdb::ScrobbleAccount {
        user_id: user.into(),
        service,
        secret_enc: enc.clone(),
        secret_nonce: nonce.clone(),
        account_name: None,
        needs_reconnect: false,
        api_base: None,
    };
    assert_eq!(
        open(&key, &account("u1", ScrobbleService::LibreFm)).as_deref(),
        Some("sk-123")
    );
    assert_eq!(open(&key, &account("u2", ScrobbleService::LibreFm)), None);
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
    }
    connect(&env, "u1", ScrobbleService::LibreFm, "sk");
    connect(&env, "u1", ScrobbleService::ListenBrainz, "lb-token-1234");
    let conn = env.db.lock().unwrap();
    assert_eq!(enqueue_play(&conn, "u1", "t1", 100, 100), 2);
    assert_eq!(enqueue_play(&conn, "u1", "missing", 100, 100), 0);
    assert_eq!(enqueue_play(&conn, "u2", "t1", 100, 100), 0);
}

#[tokio::test]
async fn delivery_sends_defers_and_flags_reconnects() {
    let env = env("scrobble-deliver");
    let librefm = MockServer::start().await;
    let lb = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_string_contains("method=track.scrobble"))
        .and(body_string_contains("sk=good-sk"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"scrobbles": {}})))
        .mount(&librefm)
        .await;
    Mock::given(method("POST"))
        .and(path("/1/submit-listens"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&lb)
        .await;
    connect(&env, "u1", ScrobbleService::LibreFm, "good-sk");
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
    let report = deliver_due(&env, &endpoints(&librefm, &lb)).await;
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
            .any(|a| a.service == ScrobbleService::LibreFm && a.last_scrobble_at.is_some()));
    }
    // Deferred plays are not due yet.
    assert_eq!(
        deliver_due(&env, &endpoints(&librefm, &lb)).await,
        PassReport::default()
    );

    // An expired Libre.fm session flags the account and keeps its plays.
    let librefm2 = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({"error": 9, "message": "Invalid session key"})),
        )
        .mount(&librefm2)
        .await;
    {
        let conn = env.db.lock().unwrap();
        qdb::enqueue(
            &conn,
            "u1",
            ScrobbleService::LibreFm,
            &qdb::track_meta(&conn, "t1").unwrap().unwrap(),
            unix_now() - 50,
            unix_now(),
        )
        .unwrap();
    }
    let report = deliver_due(&env, &endpoints(&librefm2, &lb)).await;
    assert_eq!(report.reconnect, 1);
    let active = qdb::active_services(&env.db.lock().unwrap(), "u1").unwrap();
    assert_eq!(active, [ScrobbleService::ListenBrainz]);
    assert_eq!(
        queued(&env)
            .iter()
            .filter(|(_, s, _)| s == "librefm")
            .count(),
        1
    );
}

#[tokio::test]
async fn rejected_batches_are_dropped() {
    let env = env("scrobble-reject");
    let librefm = MockServer::start().await;
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
    let report = deliver_due(&env, &endpoints(&librefm, &lb)).await;
    assert_eq!(report.dropped, 1);
    assert!(queued(&env).is_empty());
}

#[tokio::test]
async fn now_playing_reaches_every_connected_service() {
    let env = env("scrobble-nowplaying");
    let librefm = MockServer::start().await;
    let lb = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_string_contains("method=track.updateNowPlaying"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"nowplaying": {}})))
        .expect(1)
        .mount(&librefm)
        .await;
    Mock::given(method("POST"))
        .and(path("/1/submit-listens"))
        .and(body_partial_json(json!({"listen_type": "playing_now"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})))
        .expect(1)
        .mount(&lb)
        .await;
    // No accounts: nothing is sent.
    forward_now_playing(&env, &endpoints(&librefm, &lb), "u1", "t1").await;
    connect(&env, "u1", ScrobbleService::LibreFm, "sk");
    connect(&env, "u1", ScrobbleService::ListenBrainz, "lb-token-1234");
    forward_now_playing(&env, &endpoints(&librefm, &lb), "u1", "t1").await;
    forward_now_playing(&env, &endpoints(&librefm, &lb), "u1", "missing").await;
}

#[tokio::test]
async fn connecting_listenbrainz_validates_the_token() {
    let env = env("scrobble-connect-lb");
    let librefm = MockServer::start().await;
    let lb = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/1/validate-token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"valid": true, "user_name": "lb_alice"})),
        )
        .mount(&lb)
        .await;
    let ep = endpoints(&librefm, &lb);
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
async fn librefm_sign_in_uses_a_one_time_state() {
    let env = env("scrobble-connect-librefm");
    let librefm = MockServer::start().await;
    let lb = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_string_contains("method=auth.getSession"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"session": {"name": "lf_alice", "key": "sk-9"}})),
        )
        .mount(&librefm)
        .await;
    let ep = endpoints(&librefm, &lb);
    assert_eq!(
        begin_librefm_sign_in(&env, "u1", "http://box/path"),
        Err(ConnectError::Rejected)
    );
    let url = begin_librefm_sign_in(&env, "u1", "http://box.local:3000/").unwrap();
    assert!(
        url.starts_with("https://libre.fm/api/auth/?api_key=boogiebox"),
        "{url}"
    );
    assert!(url.contains(
        "&cb=http%3A%2F%2Fbox.local%3A3000%2Fapi%2Fscrobble%2Flibrefm%2Fcallback%3Fstate%3D"
    ));
    let state = url.rsplit("state%3D").next().unwrap().to_owned();
    assert_eq!(
        finish_librefm_sign_in(&env, &ep, "forged", "tok").await,
        Err(ConnectError::Expired)
    );
    // Tokens are URL-safe and can contain `-` and `_`.
    assert_eq!(
        finish_librefm_sign_in(&env, &ep, &state, "4fVKx6ab9u_ToICGrT2GRbq-25WsC")
            .await
            .unwrap(),
        "lf_alice"
    );
    assert_eq!(
        finish_librefm_sign_in(&env, &ep, &state, "tok123").await,
        Err(ConnectError::Expired),
        "a state works once"
    );
    let retry = begin_librefm_sign_in(&env, "u1", "http://box.local:3000").unwrap();
    let retry_state = retry.rsplit("state%3D").next().unwrap().to_owned();
    assert_eq!(
        finish_librefm_sign_in(&env, &ep, &retry_state, "tok 123").await,
        Err(ConnectError::Rejected)
    );
    let conn = env.db.lock().unwrap();
    assert!(qdb::get_account(&conn, "u1", ScrobbleService::LibreFm)
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn librefm_signs_in_and_scrobbles_without_any_admin_setup() {
    let env = env("scrobble-librefm");
    let librefm = MockServer::start().await;
    let lb = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_string_contains("method=auth.getSession"))
        .and(body_string_contains("api_key=boogiebox"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"session": {"name": "lf_alice", "key": "lf-sk"}})),
        )
        .mount(&librefm)
        .await;
    Mock::given(method("POST"))
        .and(body_string_contains("method=track.scrobble"))
        .and(body_string_contains("sk=lf-sk"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"scrobbles": {}})))
        .expect(1)
        .mount(&librefm)
        .await;
    let ep = endpoints(&librefm, &lb);
    let url = begin_librefm_sign_in(&env, "u1", "http://box.local:3000").unwrap();
    let state = url.rsplit("state%3D").next().unwrap().to_owned();
    assert_eq!(
        finish_librefm_sign_in(&env, &ep, &state, "lf-token_1")
            .await
            .unwrap(),
        "lf_alice"
    );
    {
        let conn = env.db.lock().unwrap();
        assert_eq!(
            enqueue_play(&conn, "u1", "t1", unix_now() - 300, unix_now()),
            1
        );
    }
    let report = deliver_due(&env, &ep).await;
    assert_eq!(report.sent, 1, "{report:?}");
    assert!(queued(&env).is_empty());
}

#[tokio::test]
async fn own_server_connects_with_its_address_and_receives_plays() {
    let env = env("scrobble-own-server");
    let librefm = MockServer::start().await;
    let lb = MockServer::start().await;
    let own = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/apis/listenbrainz/1/validate-token"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"valid": true, "user_name": "mal"})),
        )
        .mount(&own)
        .await;
    Mock::given(method("POST"))
        .and(path("/apis/listenbrainz/1/submit-listens"))
        .and(body_partial_json(json!({"listen_type": "single"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})))
        .expect(1)
        .mount(&own)
        .await;
    Mock::given(method("POST"))
        .and(path("/apis/listenbrainz/1/submit-listens"))
        .and(body_partial_json(json!({"listen_type": "playing_now"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})))
        .expect(1)
        .mount(&own)
        .await;
    assert_eq!(
        connect_own_server(&env, "u1", "box.lan/apis", "maloja-key").await,
        Err(ConnectError::BadAddress)
    );
    assert_eq!(
        connect_own_server(&env, "u1", &own.uri(), "two words").await,
        Err(ConnectError::Rejected)
    );
    let address = format!("{}/apis/listenbrainz/1/", own.uri());
    let (stored, name) = connect_own_server(&env, "u1", &address, " maloja-key ")
        .await
        .unwrap();
    assert_eq!(stored, format!("{}/apis/listenbrainz", own.uri()));
    assert_eq!(name.as_deref(), Some("mal"));
    // ListenBrainz and the own server are separate targets.
    connect(&env, "u1", ScrobbleService::ListenBrainz, "lb-token-1234");
    Mock::given(method("POST"))
        .and(path("/1/submit-listens"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})))
        .mount(&lb)
        .await;
    let ep = endpoints(&librefm, &lb);
    forward_now_playing(&env, &ep, "u1", "t1").await;
    {
        let conn = env.db.lock().unwrap();
        assert_eq!(
            enqueue_play(&conn, "u1", "t1", unix_now() - 300, unix_now()),
            2
        );
    }
    let report = deliver_due(&env, &ep).await;
    assert_eq!(report.sent, 2, "{report:?}");
    let conn = env.db.lock().unwrap();
    let account = qdb::get_account(&conn, "u1", ScrobbleService::OwnServer)
        .unwrap()
        .unwrap();
    assert_eq!(account.api_base, Some(stored));
    assert_ne!(account.secret_enc, b"maloja-key");
}

#[tokio::test]
async fn own_server_without_an_address_asks_to_reconnect() {
    let env = env("scrobble-own-server-missing");
    let librefm = MockServer::start().await;
    let lb = MockServer::start().await;
    connect(&env, "u1", ScrobbleService::OwnServer, "maloja-key");
    {
        let conn = env.db.lock().unwrap();
        enqueue_play(&conn, "u1", "t1", unix_now() - 300, unix_now());
    }
    let report = deliver_due(&env, &endpoints(&librefm, &lb)).await;
    assert_eq!(report.reconnect, 1);
    assert_eq!(queued(&env).len(), 1, "the play is kept");
}
