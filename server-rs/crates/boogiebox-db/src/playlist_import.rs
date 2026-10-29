//! M3U/M3U8 playlist import: decoding, parsing, and matching entries to library tracks.
//!
//! Matching runs on a [`LibrarySnapshot`] taken once under the database lock, so the (possibly
//! 10,000-entry) work happens without holding it.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

use crate::backup::{looks_windows, remap_path, PathRemap};

/// Largest playlist file accepted.
pub const MAX_IMPORT_BYTES: usize = 2 * 1024 * 1024;
/// Most entries one import may hold.
pub const MAX_IMPORT_ENTRIES: usize = 10_000;
const MAX_CANDIDATES: usize = 5;
const MAX_SUGGESTED_REMAPS: usize = 3;
const DURATION_TOLERANCE_SECS: f64 = 3.0;

// ── Decoding and parsing ──────────────────────────────────────────────────────

/// Decodes playlist bytes: UTF-8 (BOM stripped), falling back to Windows-1252 for legacy `.m3u`.
pub fn decode_playlist(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|&b| cp1252_char(b)).collect(),
    }
}

fn cp1252_char(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž',
        '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}',
        'ž', 'Ÿ',
    ];
    match byte {
        0x80..=0x9F => HIGH[usize::from(byte - 0x80)],
        _ => char::from(byte),
    }
}

/// `#EXTINF:<seconds>,<artist> - <title>`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtInf {
    pub duration: Option<f64>,
    pub artist: Option<String>,
    pub title: Option<String>,
}

/// One playlist entry: a location line, or an `#EXTINF` with no location after it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistEntry {
    /// 1-based line number in the file.
    pub line: usize,
    pub raw: String,
    /// Empty for an `#EXTINF` line with no location after it.
    pub location: String,
    pub ext: Option<ExtInf>,
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then_some(&text[prefix.len()..])
}

fn parse_extinf(rest: &str) -> ExtInf {
    let (head, display) = rest.split_once(',').unwrap_or((rest, ""));
    let duration = head
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| *value > 0.0);
    let display = display.trim();
    let split = [" - ", " – ", " — "]
        .iter()
        .find_map(|sep| display.split_once(sep));
    let (artist, title) = match split {
        Some((artist, title)) => (Some(artist.trim()), title.trim()),
        None => (None, display),
    };
    ExtInf {
        duration,
        artist: artist.filter(|a| !a.is_empty()).map(str::to_owned),
        title: (!title.is_empty()).then(|| title.to_owned()),
    }
}

/// Parses `#EXTM3U`, `#EXTINF` and location lines; other directives and comments are ignored.
pub fn parse_playlist(text: &str) -> Vec<PlaylistEntry> {
    let text = text.trim_start_matches('\u{feff}');
    let mut entries = Vec::new();
    let mut pending: Option<(usize, String, ExtInf)> = None;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = strip_prefix_ci(trimmed, "#EXTINF:") {
            if let Some((line, raw, ext)) = pending.take() {
                entries.push(PlaylistEntry {
                    line,
                    raw,
                    location: String::new(),
                    ext: Some(ext),
                });
            }
            pending = Some((index + 1, trimmed.to_owned(), parse_extinf(rest)));
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        entries.push(PlaylistEntry {
            line: index + 1,
            raw: trimmed.to_owned(),
            location: trimmed.to_owned(),
            ext: pending.take().map(|(_, _, ext)| ext),
        });
    }
    if let Some((line, raw, ext)) = pending {
        entries.push(PlaylistEntry {
            line,
            raw,
            location: String::new(),
            ext: Some(ext),
        });
    }
    entries
}

// ── Library snapshot ──────────────────────────────────────────────────────────

/// A track as the matcher sees it.
#[derive(Debug, Clone, Default)]
pub struct LibraryTrack {
    pub id: String,
    pub file_path: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<i64>,
    pub duration: Option<f64>,
}

/// Every track plus the library root folders, read once.
#[derive(Debug, Clone, Default)]
pub struct LibrarySnapshot {
    pub tracks: Vec<LibraryTrack>,
    pub roots: Vec<String>,
}

/// Reads the snapshot the matcher works on.
pub fn load_library_snapshot(conn: &Connection) -> rusqlite::Result<LibrarySnapshot> {
    let tracks = conn
        .prepare(
            "SELECT t.id, t.file_path, COALESCE(NULLIF(t.title, ''), t.file_name), ar.name, al.title,
                    COALESCE(t.year, al.year), t.duration
             FROM tracks t
             LEFT JOIN artists ar ON ar.id = t.artist_id
             LEFT JOIN albums al ON al.id = t.album_id",
        )?
        .query_map([], |row| {
            Ok(LibraryTrack {
                id: row.get(0)?,
                file_path: row.get(1)?,
                title: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                artist: row.get(3)?,
                album: row.get(4)?,
                year: row.get(5)?,
                duration: row.get::<_, Option<f64>>(6)?.filter(|d| *d > 0.0),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut roots: Vec<String> = Vec::new();
    for sql in [
        "SELECT path FROM library_folders ORDER BY position",
        "SELECT path FROM libraries",
    ] {
        let Ok(mut stmt) = conn.prepare(sql) else {
            continue;
        };
        let paths = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for path in paths {
            if !roots.contains(&path) {
                roots.push(path);
            }
        }
    }
    Ok(LibrarySnapshot { tracks, roots })
}

// ── Preview output ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryStatus {
    Matched,
    Ambiguous,
    Unmatched,
    Remote,
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub track_id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<i64>,
    pub duration: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewEntry {
    pub line: usize,
    pub raw: String,
    pub status: EntryStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,
    /// "Artist – Title" from `#EXTINF`, else the file name.
    pub label: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<Candidate>,
    /// The candidate closest in duration, when the file gives one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_track_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PreviewTotals {
    pub matched: usize,
    pub ambiguous: usize,
    pub unmatched: usize,
    pub remote: usize,
    pub duplicate: usize,
}

/// A folder swap: entries under `from` are looked up under `to`; `gain` is how many it finds.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemapSuggestion {
    pub from: String,
    pub to: String,
    pub gain: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub entries: Vec<PreviewEntry>,
    pub totals: PreviewTotals,
    pub suggested_remaps: Vec<RemapSuggestion>,
    pub applied_remaps: Vec<RemapSuggestion>,
}

// ── Matching ──────────────────────────────────────────────────────────────────

/// NFC, `/` separators, no repeated or trailing separators (a leading `//` for UNC stays).
fn path_key(path: &str) -> String {
    let unified: String = path
        .nfc()
        .map(|c| if c == '\\' { '/' } else { c })
        .collect();
    let unc = unified.starts_with("//");
    let mut key = String::with_capacity(unified.len());
    for part in unified.split('/') {
        if part.is_empty() {
            continue;
        }
        if !key.is_empty() || unified.starts_with('/') {
            key.push('/');
        }
        key.push_str(part);
    }
    if unc {
        key.insert(0, '/');
    }
    key
}

fn key_is_windows(key: &str) -> bool {
    looks_windows(&key.replace('/', "\\"))
}

fn is_absolute_key(key: &str) -> bool {
    key.starts_with('/') || key_is_windows(key)
}

fn text_key(text: &str) -> String {
    text.nfc()
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn percent_decode(text: &str) -> String {
    let hex = |b: u8| char::from(b).to_digit(16).map(|d| d as u8);
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(high << 4 | low);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The local path a location names, or `None` for a web (non-`file:`) URL.
fn location_path(location: &str) -> Option<String> {
    if let Some(rest) = strip_prefix_ci(location, "file://") {
        let rest = percent_decode(rest);
        let rest = rest.strip_prefix("localhost").unwrap_or(&rest).to_owned();
        let bytes = rest.as_bytes();
        // file:///C:/Music → C:/Music; file://server/share → //server/share.
        if bytes.len() > 2 && bytes[0] == b'/' && bytes[2] == b':' {
            return Some(rest[1..].to_owned());
        }
        if !rest.starts_with('/') {
            return Some(format!("//{rest}"));
        }
        return Some(rest);
    }
    if let Some((scheme, _)) = location.split_once("://") {
        if scheme.len() >= 2
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        {
            return None;
        }
    }
    Some(location.to_owned())
}

fn components_lower(key: &str) -> Vec<String> {
    key.split('/')
        .filter(|part| !part.is_empty() && *part != "." && *part != "..")
        .map(str::to_lowercase)
        .collect()
}

struct Index<'a> {
    snapshot: &'a LibrarySnapshot,
    keys: Vec<String>,
    components: Vec<Vec<String>>,
    by_lower_key: HashMap<String, Vec<usize>>,
    by_file_name: HashMap<String, Vec<usize>>,
    by_artist_title: HashMap<(String, String), Vec<usize>>,
    by_title: HashMap<String, Vec<usize>>,
}

impl<'a> Index<'a> {
    fn new(snapshot: &'a LibrarySnapshot) -> Self {
        let mut index = Index {
            snapshot,
            keys: Vec::with_capacity(snapshot.tracks.len()),
            components: Vec::with_capacity(snapshot.tracks.len()),
            by_lower_key: HashMap::new(),
            by_file_name: HashMap::new(),
            by_artist_title: HashMap::new(),
            by_title: HashMap::new(),
        };
        for (i, track) in snapshot.tracks.iter().enumerate() {
            let key = path_key(&track.file_path);
            let components = components_lower(&key);
            index
                .by_lower_key
                .entry(key.to_lowercase())
                .or_default()
                .push(i);
            if let Some(name) = components.last() {
                index.by_file_name.entry(name.clone()).or_default().push(i);
            }
            let title = text_key(&track.title);
            if !title.is_empty() {
                if let Some(artist) = track.artist.as_deref().map(text_key) {
                    index
                        .by_artist_title
                        .entry((artist, title.clone()))
                        .or_default()
                        .push(i);
                }
                index.by_title.entry(title).or_default().push(i);
            }
            index.keys.push(key);
            index.components.push(components);
        }
        index
    }

    /// Tracks at exactly `key`; case-insensitive when either side is a Windows path.
    fn exact(&self, key: &str) -> Vec<usize> {
        let Some(found) = self.by_lower_key.get(&key.to_lowercase()) else {
            return Vec::new();
        };
        let same_case: Vec<usize> = found
            .iter()
            .copied()
            .filter(|&i| self.keys[i] == key)
            .collect();
        if !same_case.is_empty() {
            return same_case;
        }
        if key_is_windows(key) {
            return found.clone();
        }
        found
            .iter()
            .copied()
            .filter(|&i| key_is_windows(&self.keys[i]))
            .collect()
    }

    /// Tracks sharing the longest run of trailing path components (file name first).
    fn by_suffix(&self, key: &str) -> Vec<usize> {
        let wanted = components_lower(key);
        let Some(name) = wanted.last() else {
            return Vec::new();
        };
        let Some(found) = self.by_file_name.get(name) else {
            return Vec::new();
        };
        let score = |i: usize| {
            self.components[i]
                .iter()
                .rev()
                .zip(wanted.iter().rev())
                .take_while(|(a, b)| a == b)
                .count()
        };
        let best = found.iter().map(|&i| score(i)).max().unwrap_or(0);
        found
            .iter()
            .copied()
            .filter(|&i| score(i) == best)
            .collect()
    }

    fn by_tags(&self, ext: &ExtInf) -> Vec<usize> {
        let Some(title) = ext.title.as_deref().map(text_key) else {
            return Vec::new();
        };
        let found = match ext.artist.as_deref().map(text_key) {
            Some(artist) => self.by_artist_title.get(&(artist, title)),
            None => self.by_title.get(&title),
        };
        let found = found.cloned().unwrap_or_default();
        match ext.duration {
            Some(_) => found
                .into_iter()
                .filter(|&i| self.duration_close(i, ext))
                .collect(),
            None => found,
        }
    }

    fn duration_close(&self, i: usize, ext: &ExtInf) -> bool {
        match (ext.duration, self.snapshot.tracks[i].duration) {
            (Some(want), Some(have)) => (want - have).abs() <= DURATION_TOLERANCE_SECS,
            _ => true,
        }
    }

    /// Narrows path ties with the `#EXTINF` title and duration.
    fn narrow(&self, ties: Vec<usize>, ext: Option<&ExtInf>) -> Vec<usize> {
        let Some(ext) = ext else {
            return ties;
        };
        let title = ext.title.as_deref().map(text_key);
        let narrowed: Vec<usize> = ties
            .iter()
            .copied()
            .filter(|&i| {
                title
                    .as_ref()
                    .is_none_or(|t| text_key(&self.snapshot.tracks[i].title) == *t)
                    && self.duration_close(i, ext)
            })
            .collect();
        if narrowed.is_empty() {
            ties
        } else {
            narrowed
        }
    }

    fn candidate(&self, i: usize) -> Candidate {
        let track = &self.snapshot.tracks[i];
        Candidate {
            track_id: track.id.clone(),
            title: track.title.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            year: track.year,
            duration: track.duration,
        }
    }
}

enum Resolution {
    Found(usize, Option<usize>),
    Ties(Vec<usize>),
    Missing,
    Remote,
}

fn resolve(
    index: &Index<'_>,
    entry: &PlaylistEntry,
    remaps: &[PathRemap],
) -> (Resolution, Option<String>) {
    let ext = entry.ext.as_ref();
    let key = if entry.location.is_empty() {
        None
    } else {
        match location_path(&entry.location) {
            Some(path) => Some(path_key(&path)),
            None => return (Resolution::Remote, None),
        }
    };
    let mut ties: Vec<usize> = Vec::new();
    if let Some(key) = key.as_deref() {
        if is_absolute_key(key) {
            for (r, remap) in remaps.iter().enumerate() {
                let Some(moved) = remap_path(key, &remap.from, &remap.to) else {
                    continue;
                };
                let hits = index.narrow(index.exact(&path_key(&moved)), ext);
                if hits.len() == 1 {
                    return (Resolution::Found(hits[0], Some(r)), Some(key.to_owned()));
                }
            }
            let hits = index.narrow(index.exact(key), ext);
            if hits.len() == 1 {
                return (Resolution::Found(hits[0], None), Some(key.to_owned()));
            }
            ties = hits;
        }
        if ties.is_empty() {
            ties = index.narrow(index.by_suffix(key), ext);
            if ties.len() == 1 {
                return (Resolution::Found(ties[0], None), Some(key.to_owned()));
            }
        }
    }
    if ties.is_empty() {
        if let Some(ext) = ext {
            ties = index.by_tags(ext);
            if ties.len() == 1 {
                return (Resolution::Found(ties[0], None), key);
            }
        }
    }
    if ties.is_empty() {
        (Resolution::Missing, key)
    } else {
        (Resolution::Ties(ties), key)
    }
}

fn entry_label(entry: &PlaylistEntry) -> String {
    if let Some(ext) = &entry.ext {
        match (&ext.artist, &ext.title) {
            (Some(artist), Some(title)) => return format!("{artist} – {title}"),
            (None, Some(title)) => return title.clone(),
            _ => {}
        }
    }
    let name = entry
        .location
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&entry.location);
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    if stem.is_empty() {
        entry.raw.clone()
    } else {
        stem.to_owned()
    }
}

/// Matches every entry and suggests folder swaps for the ones not found.
pub fn preview_import(
    snapshot: &LibrarySnapshot,
    entries: &[PlaylistEntry],
    remaps: &[PathRemap],
) -> ImportPreview {
    let index = Index::new(snapshot);
    let mut out = Vec::with_capacity(entries.len());
    let mut totals = PreviewTotals::default();
    let mut seen: HashSet<String> = HashSet::new();
    let mut remap_gain = vec![0usize; remaps.len()];
    let mut open_keys: Vec<(usize, String)> = Vec::new();
    for entry in entries {
        let (resolution, key) = resolve(&index, entry, remaps);
        let mut item = PreviewEntry {
            line: entry.line,
            raw: entry.raw.clone(),
            status: EntryStatus::Unmatched,
            track_id: None,
            label: entry_label(entry),
            candidates: Vec::new(),
            suggested_track_id: None,
        };
        match resolution {
            Resolution::Remote => item.status = EntryStatus::Remote,
            Resolution::Found(i, via) => {
                let id = snapshot.tracks[i].id.clone();
                item.status = if seen.insert(id.clone()) {
                    EntryStatus::Matched
                } else {
                    EntryStatus::Duplicate
                };
                if let Some(r) = via {
                    remap_gain[r] += 1;
                }
                item.track_id = Some(id);
            }
            Resolution::Ties(mut ties) => {
                item.status = EntryStatus::Ambiguous;
                if let Some(want) = entry.ext.as_ref().and_then(|e| e.duration) {
                    let gap = |i: &usize| {
                        snapshot.tracks[*i]
                            .duration
                            .map_or(f64::MAX, |d| (d - want).abs())
                    };
                    ties.sort_by(|a, b| gap(a).total_cmp(&gap(b)));
                    item.suggested_track_id = Some(snapshot.tracks[ties[0]].id.clone());
                }
                ties.truncate(MAX_CANDIDATES);
                item.candidates = ties.into_iter().map(|i| index.candidate(i)).collect();
            }
            Resolution::Missing => {}
        }
        if matches!(item.status, EntryStatus::Unmatched | EntryStatus::Ambiguous) {
            if let Some(key) = key.filter(|k| is_absolute_key(k)) {
                open_keys.push((out.len(), key));
            }
        }
        match item.status {
            EntryStatus::Matched => totals.matched += 1,
            EntryStatus::Ambiguous => totals.ambiguous += 1,
            EntryStatus::Unmatched => totals.unmatched += 1,
            EntryStatus::Remote => totals.remote += 1,
            EntryStatus::Duplicate => totals.duplicate += 1,
        }
        out.push(item);
    }
    let applied_remaps = remaps
        .iter()
        .zip(remap_gain)
        .map(|(remap, gain)| RemapSuggestion {
            from: remap.from.clone(),
            to: remap.to.clone(),
            gain,
        })
        .collect();
    ImportPreview {
        suggested_remaps: suggest_remaps(&index, &open_keys, remaps),
        entries: out,
        totals,
        applied_remaps,
    }
}

/// For entries still open, finds an old folder whose swap for a library root gives an exact match.
fn suggest_remaps(
    index: &Index<'_>,
    open_keys: &[(usize, String)],
    applied: &[PathRemap],
) -> Vec<RemapSuggestion> {
    let roots: Vec<(String, String)> = index
        .snapshot
        .roots
        .iter()
        .map(|root| (root.clone(), path_key(root)))
        .collect();
    let mut found: HashMap<(String, String), HashSet<usize>> = HashMap::new();
    for (entry, key) in open_keys {
        let windows = key_is_windows(key);
        let parts: Vec<&str> = key.split('/').filter(|p| !p.is_empty()).collect();
        'rest: for keep in (1..parts.len()).rev() {
            let rest = parts[parts.len() - keep..].join("/");
            for (root, root_key) in &roots {
                if index.exact(&format!("{root_key}/{rest}")).len() != 1 {
                    continue;
                }
                let head = &parts[..parts.len() - keep];
                let from = if windows {
                    let joined = head.join("\\");
                    if key.starts_with("//") {
                        format!("\\\\{joined}")
                    } else {
                        joined
                    }
                } else {
                    format!("/{}", head.join("/"))
                };
                let taken = applied.iter().any(|a| {
                    a.from.trim_end_matches(['/', '\\']).to_lowercase() == from.to_lowercase()
                });
                if !taken {
                    found
                        .entry((from, root.clone()))
                        .or_default()
                        .insert(*entry);
                }
                break 'rest;
            }
        }
    }
    let mut ranked: Vec<RemapSuggestion> = found
        .into_iter()
        .map(|((from, to), entries)| RemapSuggestion {
            from,
            to,
            gain: entries.len(),
        })
        .collect();
    ranked.sort_by(|a, b| b.gain.cmp(&a.gain).then_with(|| a.from.cmp(&b.from)));
    ranked.truncate(MAX_SUGGESTED_REMAPS);
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, path: &str, artist: &str, title: &str, duration: f64) -> LibraryTrack {
        LibraryTrack {
            id: id.into(),
            file_path: path.into(),
            title: title.into(),
            artist: Some(artist.into()),
            album: Some("Album".into()),
            year: Some(1998),
            duration: Some(duration),
        }
    }

    fn snapshot() -> LibrarySnapshot {
        LibrarySnapshot {
            tracks: vec![
                track(
                    "t1",
                    "/mnt/music/Air/Moon Safari/01 La femme d'argent.flac",
                    "Air",
                    "La femme d'argent",
                    431.0,
                ),
                track(
                    "t2",
                    "/mnt/music/Air/Moon Safari/02 Sexy Boy.flac",
                    "Air",
                    "Sexy Boy",
                    298.0,
                ),
                track(
                    "t3",
                    "/mnt/music/Massive Attack/Mezzanine/03 Teardrop.flac",
                    "Massive Attack",
                    "Teardrop",
                    329.0,
                ),
                track(
                    "t4",
                    "/mnt/music/Massive Attack/Collected/05 Teardrop.flac",
                    "Massive Attack",
                    "Teardrop",
                    331.0,
                ),
                track(
                    "t5",
                    "/mnt/music/Bj\u{f6}rk/Post/01 Army of Me.flac",
                    "Bj\u{f6}rk",
                    "Army of Me",
                    234.0,
                ),
                track("t6", "/mnt/music/A/One/Intro.mp3", "A", "Intro", 60.0),
                track("t7", "/mnt/music/B/Two/Intro.mp3", "B", "Intro", 90.0),
            ],
            roots: vec!["/mnt/music".into()],
        }
    }

    fn statuses(preview: &ImportPreview) -> Vec<EntryStatus> {
        preview.entries.iter().map(|e| e.status).collect()
    }

    #[test]
    fn decodes_bom_utf8_and_windows_1252() {
        assert_eq!(decode_playlist(b"\xEF\xBB\xBFabc"), "abc");
        assert_eq!(decode_playlist("Bj\u{f6}rk".as_bytes()), "Bj\u{f6}rk");
        assert_eq!(decode_playlist(b"Bj\xF6rk \x96 \x80"), "Bj\u{f6}rk – €");
    }

    #[test]
    fn parses_crlf_extinf_comments_and_trailing_extinf() {
        let text = "#EXTM3U\r\n# a comment\r\n#EXTINF:298,Air - Sexy Boy\r\nAir/02 Sexy Boy.flac\r\n\r\n#EXTINF:-1,Radio\r\nhttp://radio.example/stream\r\n#PLAYLIST:x\r\n#extinf:60 tvg-id=\"1\",Intro\r\n";
        let entries = parse_playlist(text);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].line, 4);
        assert_eq!(entries[0].location, "Air/02 Sexy Boy.flac");
        assert_eq!(
            entries[0].ext,
            Some(ExtInf {
                duration: Some(298.0),
                artist: Some("Air".into()),
                title: Some("Sexy Boy".into())
            })
        );
        assert_eq!(entries[1].ext.as_ref().unwrap().duration, None);
        assert_eq!(entries[2].location, "");
        assert_eq!(entries[2].line, 9);
        assert_eq!(
            entries[2].ext.as_ref().unwrap().title.as_deref(),
            Some("Intro")
        );
    }

    #[test]
    fn exact_path_and_file_urls_match() {
        let entries = parse_playlist(
            "/mnt/music/Air/Moon Safari/02 Sexy Boy.flac\nfile:///mnt/music/Air/Moon%20Safari/01%20La%20femme%20d'argent.flac\n",
        );
        let preview = preview_import(&snapshot(), &entries, &[]);
        assert_eq!(statuses(&preview), vec![EntryStatus::Matched; 2]);
        assert_eq!(preview.entries[0].track_id.as_deref(), Some("t2"));
        assert_eq!(preview.entries[1].track_id.as_deref(), Some("t1"));
    }

    #[test]
    fn relative_and_foreign_paths_match_by_suffix() {
        let entries = parse_playlist(
            "..\\Air\\Moon Safari\\02 Sexy Boy.flac\nD:\\Old\\Music\\Massive Attack\\Mezzanine\\03 Teardrop.flac\n",
        );
        let preview = preview_import(&snapshot(), &entries, &[]);
        assert_eq!(statuses(&preview), vec![EntryStatus::Matched; 2]);
        assert_eq!(preview.entries[1].track_id.as_deref(), Some("t3"));
    }

    #[test]
    fn nfd_paths_match_nfc_library() {
        let nfd = "/mnt/music/Bjo\u{308}rk/Post/01 Army of Me.flac";
        let preview = preview_import(&snapshot(), &parse_playlist(nfd), &[]);
        assert_eq!(preview.entries[0].track_id.as_deref(), Some("t5"));
    }

    #[test]
    fn windows_library_matches_case_insensitively_and_unc() {
        let snap = LibrarySnapshot {
            tracks: vec![
                track("w1", r"D:\Music\Air\01.flac", "Air", "One", 100.0),
                track("w2", r"\\NAS\Share\Air\02.flac", "Air", "Two", 100.0),
            ],
            roots: vec![r"D:\Music".into()],
        };
        let entries = parse_playlist("d:/music/AIR/01.flac\n\\\\nas\\share\\air\\02.flac\n");
        let preview = preview_import(&snap, &entries, &[]);
        assert_eq!(statuses(&preview), vec![EntryStatus::Matched; 2]);
    }

    #[test]
    fn ties_are_ambiguous_with_the_closest_duration_suggested() {
        let entries = parse_playlist(
            "#EXTINF:331,Massive Attack - Teardrop\nE:\\gone\\Teardrop.mp3\nIntro.mp3\n",
        );
        let preview = preview_import(&snapshot(), &entries, &[]);
        assert_eq!(statuses(&preview), vec![EntryStatus::Ambiguous; 2]);
        assert_eq!(preview.entries[0].suggested_track_id.as_deref(), Some("t4"));
        assert_eq!(preview.entries[0].candidates.len(), 2);
        assert_eq!(preview.entries[0].label, "Massive Attack – Teardrop");
        assert_eq!(preview.entries[1].suggested_track_id, None);
        assert_eq!(preview.entries[1].label, "Intro");
    }

    #[test]
    fn extinf_narrows_path_ties_and_falls_back_with_duration() {
        let entries = parse_playlist(
            "#EXTINF:90,B - Intro\nIntro.mp3\n#EXTINF:300,Air - Sexy Boy\nmissing.mp3\n#EXTINF:500,Air - Sexy Boy\nmissing2.mp3\n",
        );
        let preview = preview_import(&snapshot(), &entries, &[]);
        assert_eq!(preview.entries[0].track_id.as_deref(), Some("t7"));
        assert_eq!(preview.entries[1].track_id.as_deref(), Some("t2"));
        assert_eq!(preview.entries[2].status, EntryStatus::Unmatched);
    }

    #[test]
    fn remote_and_duplicate_entries() {
        let entries = parse_playlist(
            "https://example.com/a.mp3\n/mnt/music/Air/Moon Safari/02 Sexy Boy.flac\n/mnt/music/Air/Moon Safari/02 Sexy Boy.flac\n",
        );
        let preview = preview_import(&snapshot(), &entries, &[]);
        assert_eq!(
            statuses(&preview),
            vec![
                EntryStatus::Remote,
                EntryStatus::Matched,
                EntryStatus::Duplicate
            ]
        );
        assert_eq!(
            preview.totals,
            PreviewTotals {
                matched: 1,
                remote: 1,
                duplicate: 1,
                ..Default::default()
            }
        );
    }

    #[test]
    fn suggests_and_applies_a_folder_swap_windows_to_linux() {
        let entries =
            parse_playlist("D:\\Music\\A\\One\\Intro.mp3\nD:\\Music\\B\\Two\\Intro.mp3\n");
        let snap = LibrarySnapshot {
            tracks: vec![
                track("a", "/mnt/music/A/One/Intro.mp3", "A", "Intro", 60.0),
                track("b", "/mnt/music/B/Two/Intro.mp3", "B", "Intro", 90.0),
                track("c", "/srv/other/One/Intro.mp3", "C", "Intro", 70.0),
            ],
            roots: vec!["/mnt/music".into()],
        };
        let preview = preview_import(&snap, &entries, &[]);
        // Suffix matching already finds both (two folders deep).
        assert_eq!(preview.totals.matched, 2);

        let entries = parse_playlist("D:\\Music\\Intro.mp3\n");
        let snap = LibrarySnapshot {
            tracks: vec![
                track("a", "/mnt/music/Intro.mp3", "A", "Intro", 60.0),
                track("b", "/srv/music/Intro.mp3", "B", "Intro", 90.0),
            ],
            roots: vec!["/mnt/music".into()],
        };
        let preview = preview_import(&snap, &entries, &[]);
        assert_eq!(preview.totals.ambiguous, 1);
        assert_eq!(
            preview.suggested_remaps,
            vec![RemapSuggestion {
                from: r"D:\Music".into(),
                to: "/mnt/music".into(),
                gain: 1
            }]
        );
        let remap = PathRemap {
            from: r"D:\Music".into(),
            to: "/mnt/music".into(),
        };
        let preview = preview_import(&snap, &entries, &[remap]);
        assert_eq!(preview.entries[0].track_id.as_deref(), Some("a"));
        assert_eq!(preview.applied_remaps[0].gain, 1);
        assert!(preview.suggested_remaps.is_empty());
    }

    #[test]
    fn ten_thousand_entries_preview_quickly() {
        let tracks: Vec<LibraryTrack> = (0..20_000)
            .map(|i| {
                track(
                    &format!("t{i}"),
                    &format!(
                        "/mnt/music/Artist {}/Album {}/{i:05}.flac",
                        i % 300,
                        i % 1000
                    ),
                    "X",
                    &format!("Song {i}"),
                    200.0,
                )
            })
            .collect();
        let snap = LibrarySnapshot {
            tracks,
            roots: vec!["/mnt/music".into()],
        };
        let text: String = (0..MAX_IMPORT_ENTRIES)
            .map(|i| {
                if i % 2 == 0 {
                    format!(
                        "D:\\Rips\\Artist {}\\Album {}\\{i:05}.flac\n",
                        i % 300,
                        i % 1000
                    )
                } else {
                    format!("#EXTINF:200,X - Missing {i}\n/nowhere/{i}.mp3\n")
                }
            })
            .collect();
        let entries = parse_playlist(&text);
        assert_eq!(entries.len(), MAX_IMPORT_ENTRIES);
        let started = std::time::Instant::now();
        let preview = preview_import(&snap, &entries, &[]);
        assert_eq!(preview.totals.matched, MAX_IMPORT_ENTRIES / 2);
        assert!(
            started.elapsed().as_secs() < 10,
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn snapshot_reads_tracks_and_roots() {
        let conn = Connection::open_in_memory().unwrap();
        crate::initialize_schema(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO libraries (id, path, name) VALUES ('lib-1', '/mnt/music', 'Music');
             INSERT INTO library_folders (id, library_id, path) VALUES ('f-1', 'lib-1', '/mnt/music');
             INSERT INTO artists (id, name) VALUES ('ar-1', 'Air');
             INSERT INTO albums (id, title, year) VALUES ('al-1', 'Moon Safari', 1998);
             INSERT INTO tracks (id, library_id, artist_id, album_id, title, file_path, duration)
               VALUES ('t-1', 'lib-1', 'ar-1', 'al-1', 'Sexy Boy', '/mnt/music/02.flac', 298);",
        )
        .unwrap();
        let snap = load_library_snapshot(&conn).unwrap();
        assert_eq!(snap.roots, vec!["/mnt/music".to_string()]);
        assert_eq!(snap.tracks[0].artist.as_deref(), Some("Air"));
        assert_eq!(snap.tracks[0].album.as_deref(), Some("Moon Safari"));
        assert_eq!(snap.tracks[0].year, Some(1998));
    }
}
