//! Stream links for the Windows desktop app's native audio engine (`wip/native-hires-engine-plan.md` §6.2).
//!
//! The engine runs outside the web page and never sees the session cookie, so the signed-in page asks for
//! short-lived, unguessable links (`/media/n/{token}`). A link serves the untouched file with Range, or a
//! lossless WAV at the source rate for formats the engine can't decode (Opus, WMA). Links live in memory,
//! belong to the sign-in session that asked for them and are revoked when that session signs out.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::Rng;

/// Most ids one request may ask links for.
pub const MAX_IDS: usize = 50;
/// Most live links one sign-in session can hold; the soonest to expire go first.
pub const MAX_LINKS_PER_SESSION: usize = 1_000;
/// Expired links answer 410 (ask again) for this long, then 404.
pub const EXPIRED_KEEP: Duration = Duration::from_secs(10 * 60);
const TOKEN_BYTES: usize = 24;

/// How a link delivers its item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeDelivery {
    /// The original file, byte-ranged.
    Original,
    /// Lossless 24-bit PCM WAV at the source rate (byte-exact seeking; needs a known duration).
    Wav,
    /// An internet radio station through the server's relay (live: no seeking, no length).
    Radio,
}

impl NativeDelivery {
    /// The `decode` value the desktop app reads: who decodes the source.
    pub fn decode(self) -> &'static str {
        match self {
            NativeDelivery::Original => "native",
            NativeDelivery::Wav => "server_wav",
            NativeDelivery::Radio => "live",
        }
    }
}

/// What a link serves, and to whom (for the Activity page).
#[derive(Debug, Clone, PartialEq)]
pub struct NativeLink {
    /// [`session_key`] of the sign-in session that asked for it.
    pub session: String,
    /// Signed-in user.
    pub user_id: String,
    /// Signed-in user's name.
    pub username: String,
    /// Machine name the desktop app sent (may be empty).
    pub client: String,
    /// Track id, `mx-{outputId}` for a BoogieMix (Activity item id), or the station id for radio.
    pub item_id: String,
    /// Absolute file path (authorized when the link was issued).
    pub file_path: PathBuf,
    /// MIME of the original file.
    pub mime: &'static str,
    /// Duration (s), when known.
    pub duration: Option<f64>,
    /// Source sample rate, when known.
    pub sample_rate: Option<u32>,
    /// Source channel count, when known.
    pub channels: Option<u16>,
    /// Original file or server WAV.
    pub delivery: NativeDelivery,
    /// Expiry.
    pub expires_at: Instant,
}

/// Result of resolving a token.
#[derive(Debug, Clone, PartialEq)]
pub enum Lookup {
    /// A live link.
    Live(Box<NativeLink>),
    /// The link expired recently: the app should ask for a new one.
    Expired,
    /// Unknown token.
    Missing,
}

/// Registry of native stream links.
#[derive(Debug, Clone, Default)]
pub struct NativeLinks {
    inner: Arc<Mutex<HashMap<String, NativeLink>>>,
}

fn new_token() -> String {
    let mut bytes = [0u8; TOKEN_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// True for strings shaped like a token (rejects anything else before touching the registry).
pub fn is_token_shaped(s: &str) -> bool {
    s.len() == TOKEN_BYTES * 2 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Stable key for a sign-in session, so the session token itself is never kept here.
pub fn session_key(session_token: &str) -> String {
    let digest = <sha2::Sha256 as sha2::Digest>::digest(session_token.as_bytes());
    hex::encode(&digest[..16])
}

/// Link lifetime for an item of `duration` seconds (the item's length plus 6 hours, as for Play On).
pub fn link_ttl(duration: Option<f64>) -> Duration {
    crate::remote::media::token_ttl(duration)
}

/// Formats the engine can't decode itself (no symphonia codec): Opus (also inside `.ogg`) and WMA.
pub fn needs_server_decode(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("opus" | "wma") => true,
        Some("ogg") => ogg_is_opus(path),
        _ => false,
    }
}

fn ogg_is_opus(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 128];
    let Ok(n) = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)) else {
        return false;
    };
    head[..n].windows(8).any(|w| w == b"OpusHead")
}

impl NativeLinks {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, NativeLink>> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Stores `link` and returns its token. Keeps each session under [`MAX_LINKS_PER_SESSION`].
    pub fn issue(&self, link: NativeLink) -> String {
        let token = new_token();
        let session = link.session.clone();
        let mut map = self.map();
        map.insert(token.clone(), link);
        let mut owned: Vec<(Instant, String)> = map
            .iter()
            .filter(|(_, l)| l.session == session)
            .map(|(t, l)| (l.expires_at, t.clone()))
            .collect();
        if owned.len() > MAX_LINKS_PER_SESSION {
            owned.sort();
            for (_, t) in owned.iter().take(owned.len() - MAX_LINKS_PER_SESSION) {
                map.remove(t);
            }
        }
        token
    }

    /// Resolves a token at `now`.
    pub fn lookup(&self, token: &str, now: Instant) -> Lookup {
        match self.map().get(token) {
            Some(link) if link.expires_at > now => Lookup::Live(Box::new(link.clone())),
            Some(_) => Lookup::Expired,
            None => Lookup::Missing,
        }
    }

    /// Revokes every link a sign-in session asked for (sign-out).
    pub fn revoke_session(&self, session: &str) {
        self.map().retain(|_, l| l.session != session);
    }

    /// Drops links that expired more than [`EXPIRED_KEEP`] ago; returns how many.
    pub fn sweep(&self, now: Instant) -> usize {
        let mut map = self.map();
        let before = map.len();
        map.retain(|_, l| l.expires_at + EXPIRED_KEEP > now);
        before - map.len()
    }

    /// Forgets everything (database switch).
    pub fn reset(&self) {
        self.map().clear();
    }

    /// Number of links held (tests/diagnostics).
    pub fn len(&self) -> usize {
        self.map().len()
    }

    /// True when no links are held.
    pub fn is_empty(&self) -> bool {
        self.map().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(session: &str, expires_in: Duration) -> NativeLink {
        NativeLink {
            session: session.into(),
            user_id: "u1".into(),
            username: "alice".into(),
            client: "STUDIO-PC".into(),
            item_id: "t1".into(),
            file_path: PathBuf::from("/music/a.flac"),
            mime: "audio/flac",
            duration: Some(200.0),
            sample_rate: Some(96_000),
            channels: Some(2),
            delivery: NativeDelivery::Original,
            expires_at: Instant::now() + expires_in,
        }
    }

    #[test]
    fn links_resolve_expire_and_are_revoked_per_session() {
        let links = NativeLinks::default();
        let now = Instant::now();
        let live = links.issue(link("s1", Duration::from_secs(60)));
        let other = links.issue(link("s2", Duration::from_secs(60)));
        assert!(is_token_shaped(&live));
        assert!(matches!(links.lookup(&live, now), Lookup::Live(l) if l.item_id == "t1"));
        assert_eq!(links.lookup(&"0".repeat(48), now), Lookup::Missing);

        // After expiry: 410 for a while, then gone.
        let later = now + Duration::from_secs(61);
        assert_eq!(links.lookup(&live, later), Lookup::Expired);
        assert_eq!(links.sweep(later), 0, "kept to answer 410");
        assert_eq!(links.sweep(later + EXPIRED_KEEP), 2);
        assert_eq!(links.lookup(&live, later), Lookup::Missing);

        // Sign-out revokes only that session's links.
        let a = links.issue(link("s1", Duration::from_secs(60)));
        let b = links.issue(link("s2", Duration::from_secs(60)));
        links.revoke_session("s1");
        assert_eq!(links.lookup(&a, now), Lookup::Missing);
        assert!(matches!(links.lookup(&b, now), Lookup::Live(_)));
        assert_ne!(other, b);
    }

    #[test]
    fn a_session_keeps_at_most_the_cap_dropping_the_soonest_to_expire() {
        let links = NativeLinks::default();
        let first = links.issue(link("s1", Duration::from_secs(1)));
        for _ in 0..MAX_LINKS_PER_SESSION {
            links.issue(link("s1", Duration::from_secs(3_600)));
        }
        links.issue(link("s2", Duration::from_secs(1)));
        assert_eq!(links.len(), MAX_LINKS_PER_SESSION + 1);
        assert_eq!(links.lookup(&first, Instant::now()), Lookup::Missing);
    }

    #[test]
    fn session_keys_are_stable_and_hide_the_token() {
        let key = session_key("secret-session-token");
        assert_eq!(key, session_key("secret-session-token"));
        assert_ne!(key, session_key("another"));
        assert_eq!(key.len(), 32);
        assert!(!key.contains("secret"));
    }

    #[test]
    fn opus_and_wma_need_the_server_to_decode() {
        let dir = std::env::temp_dir().join(format!("native-decode-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let opus_ogg = dir.join("a.ogg");
        std::fs::write(
            &opus_ogg,
            b"OggS\0\x02\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x01\x13OpusHead",
        )
        .unwrap();
        let vorbis_ogg = dir.join("b.ogg");
        std::fs::write(
            &vorbis_ogg,
            b"OggS\0\x02\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x01\x1e\x01vorbis",
        )
        .unwrap();
        assert!(needs_server_decode(&opus_ogg));
        assert!(!needs_server_decode(&vorbis_ogg));
        assert!(needs_server_decode(Path::new("x.OPUS")));
        assert!(needs_server_decode(Path::new("x.wma")));
        for native in ["x.flac", "x.m4a", "x.wav", "x.aiff", "x.mp3", "x.aac"] {
            assert!(!needs_server_decode(Path::new(native)), "{native}");
        }
        assert_eq!(NativeDelivery::Wav.decode(), "server_wav");
    }
}
