//! Defines Rust server support logic for Artwork Cache.

use sha1::{Digest, Sha1};
use std::{
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

const CACHE_EXTENSIONS: &[&str] = &[".jpg", ".jpeg", ".png", ".webp"];

fn cache_key_hash(cache_key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(cache_key.as_bytes());
    hex::encode(hasher.finalize())
}

/// Documents the Cache Item Dir public API surface.
pub fn cache_item_dir(root_dir: &Path, cache_key: &str) -> PathBuf {
    let hash = cache_key_hash(cache_key);
    root_dir.join(&hash[..2]).join(&hash)
}

/// Documents the Find Existing Cached Image public API surface.
pub fn find_existing_cached_image(item_dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(item_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e.to_lowercase()))
            .unwrap_or_default();
        if CACHE_EXTENSIONS.contains(&ext.as_str()) {
            return Some(path);
        }
    }
    None
}

/// Documents the Clear Cached Image Files public API surface.
pub fn clear_cached_image_files(item_dir: &Path) {
    let Ok(entries) = fs::read_dir(item_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e.to_lowercase()))
            .unwrap_or_default();
        if CACHE_EXTENSIONS.contains(&ext.as_str()) {
            let _ = fs::remove_file(&path);
        }
    }
}

fn marker_path(item_dir: &Path, slot: &str) -> PathBuf {
    item_dir.join(format!(".{slot}.uuid"))
}

fn read_assigned_uuid(item_dir: &Path, slot: &str) -> Option<String> {
    let value = fs::read_to_string(marker_path(item_dir, slot)).ok()?;
    let trimmed = value.trim().to_owned();
    if trimmed.len() >= 32 {
        Some(trimmed)
    } else {
        None
    }
}

/// Returns the path to the assigned cache file, creating a UUID v7 marker if needed.
/// Documents the Get Assigned Cache File Path public API surface.
pub fn get_assigned_cache_file_path(
    root_dir: &Path,
    cache_key: &str,
    slot: &str,
    extension: &str,
    allocate: bool,
) -> Option<PathBuf> {
    let item_dir = cache_item_dir(root_dir, cache_key);
    let file_uuid = match read_assigned_uuid(&item_dir, slot) {
        Some(uuid) => uuid,
        None => {
            if !allocate {
                return None;
            }
            fs::create_dir_all(&item_dir).ok()?;
            let new_uuid = Uuid::now_v7().to_string();
            fs::write(marker_path(&item_dir, slot), &new_uuid).ok()?;
            new_uuid
        }
    };
    Some(item_dir.join(format!("{file_uuid}{extension}")))
}

/// Documents the Build Album Art Cache Key public API surface.
pub fn build_album_art_cache_key(album_id: &str) -> String {
    format!("music-art:album:{}", encode_key_part(album_id))
}

/// Documents the Build Artist Art Cache Key public API surface.
pub fn build_artist_art_cache_key(artist_id: &str) -> String {
    format!("music-art:artist:{}", encode_key_part(artist_id))
}

fn encode_key_part(value: &str) -> String {
    let trimmed = value.trim();
    url_encode(trimmed)
}

fn url_encode(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_ascii_alphanumeric() || "-_.~".contains(c) {
                vec![c]
            } else {
                let bytes = c.to_string();
                bytes
                    .as_bytes()
                    .iter()
                    .flat_map(|b| format!("%{b:02X}").chars().collect::<Vec<_>>())
                    .collect()
            }
        })
        .collect()
}

/// Returns extension from content-type header value.
/// Documents the Ext From Content Type public API surface.
pub fn ext_from_content_type(ct: &str) -> &'static str {
    let ct = ct.to_lowercase();
    if ct.contains("image/png") {
        ".png"
    } else if ct.contains("image/webp") {
        ".webp"
    } else {
        ".jpg"
    }
}

/// Returns MIME type from file extension.
/// Documents the Mime From Path public API surface.
pub fn mime_from_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        _ => "image/jpeg",
    }
}

/// Documents the Find Folder Cover Image public API surface.
pub fn find_folder_cover_image(track_file_path: &Path) -> Option<PathBuf> {
    let album_dir = track_file_path.parent()?;
    for name in &["folder.jpg", "folder.png", "Folder.jpg", "Folder.png"] {
        let full = album_dir.join(name);
        if full.is_file() {
            return Some(full);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("artwork-cache-test-{prefix}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn cache_item_dir_shards_by_first_two_hash_chars_and_is_deterministic() {
        let root = PathBuf::from("/cache-root");
        let a = cache_item_dir(&root, "music-art:album:123");
        let b = cache_item_dir(&root, "music-art:album:123");
        assert_eq!(a, b, "same cache key must hash to the same dir");

        let hash = a.file_name().unwrap().to_str().unwrap().to_owned();
        let shard = a.parent().unwrap().file_name().unwrap().to_str().unwrap();
        assert_eq!(shard, &hash[..2]);
        assert_eq!(hash.len(), 40, "sha1 hex digest is 40 chars");
    }

    #[test]
    fn cache_item_dir_differs_for_different_keys() {
        let root = PathBuf::from("/cache-root");
        assert_ne!(
            cache_item_dir(&root, "key-a"),
            cache_item_dir(&root, "key-b")
        );
    }

    #[test]
    fn find_existing_cached_image_returns_none_for_missing_dir() {
        let missing = PathBuf::from("/does/not/exist/at/all");
        assert!(find_existing_cached_image(&missing).is_none());
    }

    #[test]
    fn find_existing_cached_image_finds_supported_extension_and_ignores_others() {
        let dir = temp_dir("find-existing");
        fs::write(dir.join("notes.txt"), b"nope").unwrap();
        fs::write(dir.join("cover.webp"), b"fake-image-bytes").unwrap();

        let found = find_existing_cached_image(&dir).expect("should find the webp");
        assert_eq!(found.extension().unwrap(), "webp");
    }

    #[test]
    fn find_existing_cached_image_returns_none_when_only_unsupported_files_present() {
        let dir = temp_dir("find-existing-none");
        fs::write(dir.join("readme.md"), b"nope").unwrap();
        assert!(find_existing_cached_image(&dir).is_none());
    }

    #[test]
    fn clear_cached_image_files_removes_only_supported_extensions() {
        let dir = temp_dir("clear-cache");
        fs::write(dir.join("cover.jpg"), b"a").unwrap();
        fs::write(dir.join("cover.PNG"), b"b").unwrap();
        fs::write(dir.join("keep.txt"), b"c").unwrap();

        clear_cached_image_files(&dir);

        assert!(!dir.join("cover.jpg").exists());
        assert!(!dir.join("cover.PNG").exists());
        assert!(dir.join("keep.txt").exists());
    }

    #[test]
    fn clear_cached_image_files_on_missing_dir_does_not_panic() {
        let missing = PathBuf::from("/does/not/exist/either");
        clear_cached_image_files(&missing); // should just return early
    }

    #[test]
    fn get_assigned_cache_file_path_allocates_and_is_stable_across_calls() {
        let root = temp_dir("assign-root");
        let key = "music-art:artist:abc";

        let first = get_assigned_cache_file_path(&root, key, "orig", ".jpg", true)
            .expect("allocation should succeed");
        let second = get_assigned_cache_file_path(&root, key, "orig", ".jpg", true)
            .expect("second call reuses the marker");
        assert_eq!(
            first, second,
            "re-reading the marker must return the same uuid path"
        );
    }

    #[test]
    fn get_assigned_cache_file_path_without_allocate_returns_none_when_unassigned() {
        let root = temp_dir("assign-no-allocate");
        let result = get_assigned_cache_file_path(&root, "unassigned-key", "orig", ".jpg", false);
        assert!(result.is_none());
    }

    #[test]
    fn get_assigned_cache_file_path_different_slots_get_different_uuids() {
        let root = temp_dir("assign-slots");
        let key = "music-art:album:xyz";
        let orig = get_assigned_cache_file_path(&root, key, "orig", ".jpg", true).unwrap();
        let thumb = get_assigned_cache_file_path(&root, key, "thumb", ".jpg", true).unwrap();
        assert_ne!(orig, thumb);
    }

    #[test]
    fn build_album_and_artist_art_cache_keys_are_namespaced_and_url_encoded() {
        assert_eq!(
            build_album_art_cache_key("abc 123"),
            "music-art:album:abc%20123"
        );
        assert_eq!(
            build_artist_art_cache_key("abc/123"),
            "music-art:artist:abc%2F123"
        );
    }

    #[test]
    fn build_cache_key_trims_whitespace() {
        assert_eq!(build_album_art_cache_key("  abc  "), "music-art:album:abc");
    }

    #[test]
    fn ext_from_content_type_maps_known_and_falls_back_to_jpg() {
        assert_eq!(ext_from_content_type("image/png"), ".png");
        assert_eq!(ext_from_content_type("IMAGE/WEBP"), ".webp");
        assert_eq!(ext_from_content_type("image/gif"), ".jpg");
        assert_eq!(ext_from_content_type("text/plain"), ".jpg");
    }

    #[test]
    fn mime_from_path_maps_known_extensions_and_falls_back_to_jpeg() {
        assert_eq!(mime_from_path(Path::new("a.png")), "image/png");
        assert_eq!(mime_from_path(Path::new("a.WEBP")), "image/webp");
        assert_eq!(mime_from_path(Path::new("a.jpg")), "image/jpeg");
        assert_eq!(mime_from_path(Path::new("a")), "image/jpeg");
    }

    #[test]
    fn find_folder_cover_image_finds_first_matching_name() {
        let dir = temp_dir("folder-cover");
        fs::write(dir.join("folder.jpg"), b"cover").unwrap();
        let track_path = dir.join("01 - Track.mp3");
        fs::write(&track_path, b"audio").unwrap();

        let found = find_folder_cover_image(&track_path).expect("should find folder.jpg");
        assert_eq!(found, dir.join("folder.jpg"));
    }

    #[test]
    fn find_folder_cover_image_returns_none_when_absent() {
        let dir = temp_dir("folder-cover-none");
        let track_path = dir.join("01 - Track.mp3");
        fs::write(&track_path, b"audio").unwrap();
        assert!(find_folder_cover_image(&track_path).is_none());
    }

    #[test]
    fn find_folder_cover_image_returns_none_when_track_path_has_no_parent() {
        // A bare filename with no directory component has `parent()` == Some(""),
        // which is a valid (relative, current-dir) path — exercise that branch too
        // without relying on filesystem state.
        let bare = PathBuf::from("track.mp3");
        assert!(find_folder_cover_image(&bare).is_none());
    }
}
