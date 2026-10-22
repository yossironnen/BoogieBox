//! Subsonic password storage and checks. Subsonic token auth (`t = md5(password + salt)`) needs the
//! plaintext, so the password is stored AES-256-GCM encrypted (user id as associated data) with a key file
//! that lives next to `boogiebox.db`. A copy of the database alone does not reveal passwords.

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use md5::{Digest, Md5};
use rand::Rng;

/// Key file name, created beside `boogiebox.db`.
pub const KEY_FILE_NAME: &str = "subsonic.key";
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// Minimum Subsonic password length (characters).
pub const MIN_PASSWORD_CHARS: usize = 8;
/// Maximum Subsonic password length (characters).
pub const MAX_PASSWORD_CHARS: usize = 128;

/// Folder holding `boogiebox.db` for a configured database folder (which may name the DB file itself).
pub fn key_dir(db_folder: &Path) -> Option<PathBuf> {
    boogiebox_db::get_db_path(db_folder)
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
}

/// Loads the key file, creating it when missing or unreadable. The flag is true when a new key was written,
/// meaning previously stored passwords can no longer be decrypted.
pub fn load_or_create_key(dir: &Path) -> std::io::Result<([u8; KEY_LEN], bool)> {
    let path = dir.join(KEY_FILE_NAME);
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(bytes) = hex::decode(raw.trim()) {
            if let Ok(key) = <[u8; KEY_LEN]>::try_from(bytes.as_slice()) {
                return Ok((key, false));
            }
        }
    }
    let mut key = [0u8; KEY_LEN];
    rand::rng().fill_bytes(&mut key);
    std::fs::create_dir_all(dir)?;
    let mut file = std::fs::File::create(&path)?;
    file.write_all(hex::encode(key).as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    restrict_permissions(&path);
    Ok((key, true))
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {
    // The data folder's ACL (ProgramData or the user-chosen folder) applies on Windows.
}

/// Encrypts `password` for `user_id`; returns `(ciphertext, nonce)`.
pub fn encrypt(key: &[u8; KEY_LEN], user_id: &str, password: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::try_from(&nonce_bytes[..]).ok()?;
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: password.as_bytes(),
                aad: user_id.as_bytes(),
            },
        )
        .ok()?;
    Some((ciphertext, nonce_bytes.to_vec()))
}

/// Decrypts a stored password; `None` when the key, nonce or user binding does not match.
pub fn decrypt(
    key: &[u8; KEY_LEN],
    user_id: &str,
    ciphertext: &[u8],
    nonce: &[u8],
) -> Option<String> {
    if nonce.len() != NONCE_LEN {
        return None;
    }
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    let nonce = Nonce::try_from(nonce).ok()?;
    let plain = cipher
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad: user_id.as_bytes(),
            },
        )
        .ok()?;
    String::from_utf8(plain).ok()
}

/// Constant-time byte comparison.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Subsonic token: lowercase hex `md5(password + salt)`.
pub fn token(password: &str, salt: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(password.as_bytes());
    hasher.update(salt.as_bytes());
    hex::encode(hasher.finalize())
}

/// Prefix of BoogieBox API keys (recognisable in a password manager).
pub const API_KEY_PREFIX: &str = "bbk_";
/// Longest `apiKey` value worth hashing; anything longer cannot be a BoogieBox key.
pub const MAX_API_KEY_LEN: usize = 128;

/// A new OpenSubsonic API key: `bbk_` + 32 random bytes, base64url without padding. Only its hash is stored.
pub fn generate_api_key() -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    format!("{API_KEY_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes))
}

/// Lowercase hex SHA-256 of an API key (what the database stores and looks up).
pub fn hash_api_key(key: &str) -> String {
    hex::encode(<sha2::Sha256 as sha2::Digest>::digest(key.as_bytes()))
}

/// Why a Subsonic password was rejected (the `rule` field of the API error).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordRule {
    TooShort,
    TooLong,
    NeedsSymbol,
    InvalidCharacters,
    SameAsPin,
}

impl PasswordRule {
    pub fn code(self) -> &'static str {
        match self {
            PasswordRule::TooShort => "too_short",
            PasswordRule::TooLong => "too_long",
            PasswordRule::NeedsSymbol => "needs_symbol",
            PasswordRule::InvalidCharacters => "invalid_characters",
            PasswordRule::SameAsPin => "same_as_pin",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            PasswordRule::TooShort => "Password must be at least 8 characters",
            PasswordRule::TooLong => "Password must be at most 128 characters",
            PasswordRule::NeedsSymbol => "Password must include at least one symbol",
            PasswordRule::InvalidCharacters => "Password must not contain control characters",
            PasswordRule::SameAsPin => "Password must be different from your PIN",
        }
    }
}

/// Length/character rules (cheap). A symbol is any visible character that is not a letter or digit.
pub fn check_password_shape(password: &str) -> Result<(), PasswordRule> {
    if password.chars().any(char::is_control) {
        return Err(PasswordRule::InvalidCharacters);
    }
    let len = password.chars().count();
    if len < MIN_PASSWORD_CHARS {
        return Err(PasswordRule::TooShort);
    }
    if len > MAX_PASSWORD_CHARS {
        return Err(PasswordRule::TooLong);
    }
    if !password
        .chars()
        .any(|c| !c.is_alphanumeric() && !c.is_whitespace())
    {
        return Err(PasswordRule::NeedsSymbol);
    }
    Ok(())
}

/// Full rule check including "not the PIN" (PBKDF2 — run off the async runtime).
pub fn check_password(password: &str, pin_hash: Option<&str>) -> Result<(), PasswordRule> {
    check_password_shape(password)?;
    if pin_hash.is_some_and(|hash| crate::auth::verify_pin(hash, password)) {
        return Err(PasswordRule::SameAsPin);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_matches_the_subsonic_spec_example() {
        assert_eq!(
            token("sesame", "c19b2d"),
            "26719a1196d2a940705a59634eb18eab"
        );
    }

    #[test]
    fn encrypt_decrypt_round_trip_is_bound_to_user_and_key() {
        let key = [7u8; KEY_LEN];
        let (ct, nonce) = encrypt(&key, "u1", "s3cret!pass").unwrap();
        assert_ne!(ct, b"s3cret!pass");
        assert_eq!(
            decrypt(&key, "u1", &ct, &nonce).as_deref(),
            Some("s3cret!pass")
        );
        assert_eq!(decrypt(&key, "u2", &ct, &nonce), None);
        assert_eq!(decrypt(&[8u8; KEY_LEN], "u1", &ct, &nonce), None);
        assert_eq!(decrypt(&key, "u1", &ct, &nonce[..4]), None);
        let (ct2, nonce2) = encrypt(&key, "u1", "s3cret!pass").unwrap();
        assert_ne!((ct.clone(), nonce.clone()), (ct2, nonce2));
    }

    #[test]
    fn key_file_is_created_once_then_reused() {
        let dir = std::env::temp_dir().join(format!("bb-subsonic-key-{}", uuid::Uuid::now_v7()));
        let (k1, created1) = load_or_create_key(&dir).unwrap();
        let (k2, created2) = load_or_create_key(&dir).unwrap();
        assert!(created1 && !created2);
        assert_eq!(k1, k2);
        std::fs::write(dir.join(KEY_FILE_NAME), "garbage").unwrap();
        let (k3, created3) = load_or_create_key(&dir).unwrap();
        assert!(created3);
        assert_ne!(k1, k3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn key_dir_accepts_folder_or_db_file() {
        let folder = std::env::temp_dir().join("bb-data");
        assert_eq!(key_dir(&folder).unwrap(), folder);
        assert_eq!(key_dir(&folder.join("boogiebox.db")).unwrap(), folder);
    }

    #[test]
    fn password_rules() {
        assert_eq!(check_password_shape("Ab1!xyz"), Err(PasswordRule::TooShort));
        assert_eq!(
            check_password_shape("Abcdefg1"),
            Err(PasswordRule::NeedsSymbol)
        );
        assert_eq!(
            check_password_shape("Abcd efg1"),
            Err(PasswordRule::NeedsSymbol)
        );
        assert_eq!(
            check_password_shape(&format!("!{}", "a".repeat(128))),
            Err(PasswordRule::TooLong)
        );
        assert_eq!(
            check_password_shape("abc\u{7}defgh!"),
            Err(PasswordRule::InvalidCharacters)
        );
        assert_eq!(check_password_shape("Abcdefg!"), Ok(()));
        assert_eq!(check_password_shape("price€€€€"), Ok(()));
        // Accented letters are letters, not symbols; length counts characters, not bytes.
        assert_eq!(
            check_password_shape("éééééééé"),
            Err(PasswordRule::NeedsSymbol)
        );
        assert_eq!(check_password_shape("ééééééé!"), Ok(()));

        let pin = crate::auth::hash_pin("my-pin-123");
        assert_eq!(
            check_password("my-pin-123", Some(&pin)),
            Err(PasswordRule::SameAsPin)
        );
        assert_eq!(check_password("other-pass!", Some(&pin)), Ok(()));
        assert_eq!(check_password("other-pass!", None), Ok(()));
    }

    #[test]
    fn api_keys_are_random_prefixed_and_hash_to_hex() {
        let a = generate_api_key();
        let b = generate_api_key();
        assert_ne!(a, b);
        assert!(a.starts_with(API_KEY_PREFIX));
        assert_eq!(a.len(), API_KEY_PREFIX.len() + 43);
        assert!(a[API_KEY_PREFIX.len()..]
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
        // SHA-256("abc") known answer.
        assert_eq!(
            hash_api_key("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hash_api_key(&a), hash_api_key(&a));
    }

    #[test]
    fn constant_time_eq_basics() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
