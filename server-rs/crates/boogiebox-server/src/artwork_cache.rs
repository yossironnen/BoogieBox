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
