//! Admin log viewer (wip/admin-log-viewer-plan.md): lists the log files, reads them backwards a page at a time,
//! masks secrets and reads what was appended since a byte offset (for following).
//!
//! Only three kinds of file are ever opened, all in the folder of the server log: the server log itself (and
//! its `.1`), and the dated scan / deep-analysis debug files. Clients name a file by a short key
//! (`server`, `server.1`, `scan.2026-10-31`, `deep.2026-10-31`); a path from a client is never used.

use std::borrow::Cow;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::logging::{DEEP_DEBUG_PREFIX, SCAN_DEBUG_PREFIX};

/// Most bytes one page request reads before it stops (and says so).
pub const MAX_READ_BYTES: u64 = 8 * 1024 * 1024;
pub const DEFAULT_LIMIT: usize = 500;
pub const MAX_LIMIT: usize = 2000;
/// Longer lines are cut, with an ellipsis.
pub const MAX_LINE_BYTES: usize = 8 * 1024;
pub const MAX_QUERY_CHARS: usize = 200;
/// Most bytes one follow tick sends.
pub const MAX_FOLLOW_BYTES: u64 = 1024 * 1024;
const BLOCK: u64 = 256 * 1024;

// ── Files ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceId {
    Server,
    Scan,
    Deep,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFileInfo {
    pub key: String,
    pub name: String,
    pub size: u64,
    pub modified_ms: Option<u64>,
    /// The file being written now (the server log, or the newest debug file).
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogSource {
    pub id: SourceId,
    /// Debug logs: whether their toggle is on. The server log is always on.
    pub enabled: bool,
    /// Newest first.
    pub files: Vec<LogFileInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogList {
    pub folder: Option<String>,
    pub sources: Vec<LogSource>,
}

fn file_info(path: &Path, key: String, current: bool) -> Option<LogFileInfo> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64);
    Some(LogFileInfo {
        key,
        name: path.file_name()?.to_string_lossy().into_owned(),
        size: meta.len(),
        modified_ms,
        current,
    })
}

fn rotated_path(server_log: &Path) -> PathBuf {
    PathBuf::from(format!("{}.1", server_log.display()))
}

/// `YYYY-MM-DD` and nothing else.
fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// The dates of a debug log's files in `folder`, newest first.
fn debug_dates(folder: &Path, prefix: &str) -> Vec<String> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut dates: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let date = name.strip_prefix(prefix)?.strip_suffix(".log")?.to_owned();
            is_date(&date).then_some(date)
        })
        .collect();
    dates.sort_unstable_by(|a, b| b.cmp(a));
    dates
}

/// Every log file, by source.
pub fn list(server_log: &Path, scan_enabled: bool, deep_enabled: bool) -> LogList {
    let folder = server_log.parent();
    let mut server_files = Vec::new();
    server_files.extend(file_info(server_log, "server".into(), true));
    server_files.extend(file_info(
        &rotated_path(server_log),
        "server.1".into(),
        false,
    ));
    let debug = |id: SourceId, prefix: &str, key: &str, enabled: bool| {
        let files = folder
            .map(|folder| {
                debug_dates(folder, prefix)
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, date)| {
                        file_info(
                            &folder.join(format!("{prefix}{date}.log")),
                            format!("{key}.{date}"),
                            i == 0,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        LogSource { id, enabled, files }
    };
    LogList {
        folder: folder.map(|f| f.display().to_string()),
        sources: vec![
            LogSource {
                id: SourceId::Server,
                enabled: true,
                files: server_files,
            },
            debug(SourceId::Scan, SCAN_DEBUG_PREFIX, "scan", scan_enabled),
            debug(SourceId::Deep, DEEP_DEBUG_PREFIX, "deep", deep_enabled),
        ],
    }
}

/// The file a key names, or `None` for anything else (including a symlink or a file that isn't there).
pub fn resolve(server_log: &Path, key: &str) -> Option<PathBuf> {
    let path = match key {
        "server" => server_log.to_path_buf(),
        "server.1" => rotated_path(server_log),
        _ => {
            let (source, date) = key.split_once('.')?;
            let prefix = match source {
                "scan" => SCAN_DEBUG_PREFIX,
                "deep" => DEEP_DEBUG_PREFIX,
                _ => return None,
            };
            if !is_date(date) {
                return None;
            }
            server_log.parent()?.join(format!("{prefix}{date}.log"))
        }
    };
    fs::symlink_metadata(&path)
        .ok()
        .filter(|m| m.is_file())
        .map(|_| path)
}

/// For following a debug log: the key of a newer file of the same source, once the day rolls over.
pub fn newer_key(server_log: &Path, key: &str) -> Option<String> {
    let (source, date) = key.split_once('.')?;
    let prefix = match source {
        "scan" => SCAN_DEBUG_PREFIX,
        "deep" => DEEP_DEBUG_PREFIX,
        _ => return None,
    };
    let newest = debug_dates(server_log.parent()?, prefix)
        .into_iter()
        .next()?;
    (newest.as_str() > date).then(|| format!("{source}.{newest}"))
}

// ── Lines ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "TRACE" => Level::Trace,
            "DEBUG" => Level::Debug,
            "INFO" => Level::Info,
            "WARN" => Level::Warn,
            "ERROR" => Level::Error,
            _ => return None,
        })
    }

    pub fn from_query(s: &str) -> Option<Self> {
        Self::parse(&s.to_ascii_uppercase())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    /// Byte offset of the line's start in the file.
    pub offset: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<Level>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// The message (the whole line when it isn't a `tracing` event), secrets masked.
    pub text: String,
}

/// Splits a `tracing` fmt line (`2026-09-04T21:19:03.048536Z  INFO boogiebox_server::scanner: message`) into its
/// parts. Anything else is plain text.
fn parse(offset: u64, raw: &str) -> LogLine {
    let raw = cut(raw.trim_end_matches(['\r', '\n']));
    let plain = |text: &str| LogLine {
        offset,
        ts: None,
        level: None,
        target: None,
        text: redact(text).into_owned(),
    };
    let Some((ts, rest)) = raw.split_once(' ') else {
        return plain(&raw);
    };
    let ts_ok =
        ts.len() >= 20 && ts.as_bytes()[..4].iter().all(u8::is_ascii_digit) && ts.contains('T');
    if !ts_ok {
        return plain(&raw);
    }
    let rest = rest.trim_start();
    let Some((level, rest)) = rest.split_once(' ') else {
        return plain(&raw);
    };
    let Some(level) = Level::parse(level) else {
        return plain(&raw);
    };
    let rest = rest.trim_start();
    let (target, text) = match rest.split_once(": ") {
        Some((target, text)) if !target.is_empty() && !target.contains(' ') => {
            (Some(target.to_owned()), text)
        }
        _ => (None, rest),
    };
    LogLine {
        offset,
        ts: Some(ts.to_owned()),
        level: Some(level),
        target,
        text: redact(text).into_owned(),
    }
}

fn cut(line: &str) -> Cow<'_, str> {
    if line.len() <= MAX_LINE_BYTES {
        return Cow::Borrowed(line);
    }
    let mut end = MAX_LINE_BYTES;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    Cow::Owned(format!("{}…", &line[..end]))
}

/// Startup markers written outside `tracing`; never the continuation of the event above them.
fn is_marker(text: &str) -> bool {
    text.starts_with("BoogieBox ")
}

// ── Secrets ────────────────────────────────────────────────────────────────────

/// Keys whose value is masked wherever they appear as `key=value` or `key: value`.
const SECRET_KEYS: &[&str] = &[
    "api_key",
    "apikey",
    "api-key",
    "token",
    "access_token",
    "session_key",
    "sk",
    "password",
    "passwd",
    "secret",
    "client_secret",
];
/// Subsonic query parameters, masked only inside a URL query (`?p=` / `&t=`).
const QUERY_KEYS: &[&str] = &["p", "t", "s"];
/// Header names whose whole value is masked.
const SECRET_HEADERS: &[&str] = &["authorization", "cookie", "set-cookie"];
const MASK: &str = "•••••";

fn value_end(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|c| {
            matches!(
                c,
                b' ' | b'\t'
                    | b'\r'
                    | b'\n'
                    | b'&'
                    | b'"'
                    | b'\''
                    | b','
                    | b';'
                    | b')'
                    | b']'
                    | b'}'
            )
        })
        .map_or(bytes.len(), |n| start + n)
}

/// Masks passwords, keys and tokens. Kept as a safeguard: nothing logs them on purpose today.
pub fn redact(line: &str) -> Cow<'_, str> {
    let lower = line.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    // Header values run to the end of the line (its line break is kept).
    for header in SECRET_HEADERS {
        if let Some(i) = lower.find(&format!("{header}:")) {
            let boundary = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            if boundary {
                let start = i + header.len() + 1;
                let body = line.trim_end_matches(['\r', '\n']);
                return Cow::Owned(format!(
                    "{}{} {MASK}{}",
                    &line[..i],
                    &line[i..start],
                    &line[body.len()..]
                ));
            }
        }
    }
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let at_word = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        if at_word {
            if bytes[i..].starts_with(b"bearer ") {
                let start = i + 7;
                let end = value_end(bytes, start);
                if end > start {
                    spans.push((start, end));
                }
                i = end.max(i + 1);
                continue;
            }
            let in_query = i > 0 && matches!(bytes[i - 1], b'?' | b'&');
            let key = SECRET_KEYS
                .iter()
                .chain(QUERY_KEYS.iter().filter(|_| in_query))
                .filter(|k| bytes[i..].starts_with(k.as_bytes()))
                .max_by_key(|k| k.len());
            if let Some(key) = key {
                let after = i + key.len();
                let sep = match bytes.get(after) {
                    Some(b'=') => Some(after + 1),
                    Some(b':') => {
                        Some(after + 1 + usize::from(bytes.get(after + 1) == Some(&b' ')))
                    }
                    _ => None,
                };
                if let Some(start) = sep {
                    let end = value_end(bytes, start);
                    if end > start {
                        spans.push((start, end));
                    }
                    i = end.max(i + 1);
                    continue;
                }
            }
        }
        i += 1;
    }
    if spans.is_empty() {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut last = 0;
    for (start, end) in spans {
        out.push_str(&line[last..start]);
        out.push_str(MASK);
        last = end;
    }
    out.push_str(&line[last..]);
    Cow::Owned(out)
}

// ── Reading ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Lowest level shown; lines that aren't events show only without it.
    pub min_level: Option<Level>,
    /// Case-insensitive text search.
    pub query: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinesPage {
    /// Oldest first.
    pub lines: Vec<LogLine>,
    /// Offset of the first line returned (the next page's `before`), or of where reading stopped.
    pub start: u64,
    /// Offset just past the last complete line read (where following starts).
    pub end: u64,
    pub size: u64,
    /// More lines exist before `start`.
    pub more: bool,
    /// Reading stopped at [`MAX_READ_BYTES`] before finding `limit` lines.
    pub truncated: bool,
}

/// An event line and the plain lines that belong to it.
struct Entry {
    lines: Vec<LogLine>,
}

impl Entry {
    fn passes(&self, filter: &Filter) -> bool {
        if let Some(min) = filter.min_level {
            match self.lines[0].level {
                Some(level) if level >= min => {}
                _ => return false,
            }
        }
        match &filter.query {
            Some(q) => self.lines.iter().any(|l| {
                l.text.to_lowercase().contains(q)
                    || l.target
                        .as_deref()
                        .is_some_and(|t| t.to_lowercase().contains(q))
            }),
            None => true,
        }
    }
}

/// Groups lines (oldest first) into entries: a plain line belongs to the event above it, unless it is a
/// startup marker or there is no event above it in this window.
fn group(lines: Vec<LogLine>) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for line in lines {
        let attach = line.level.is_none()
            && !is_marker(&line.text)
            && entries.last().is_some_and(|e| e.lines[0].level.is_some());
        if attach {
            entries.last_mut().unwrap().lines.push(line);
        } else {
            entries.push(Entry { lines: vec![line] });
        }
    }
    entries
}

/// Reads the page of lines that ends at `before` (the end of the file when `None`), newest last.
pub fn read_page(
    path: &Path,
    before: Option<u64>,
    limit: usize,
    filter: &Filter,
) -> std::io::Result<LinesPage> {
    let limit = limit.clamp(1, MAX_LIMIT);
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let end_at = before.unwrap_or(size).min(size);

    // Read backwards in blocks; `buf` holds bytes [pos, end_at).
    let mut pos = end_at;
    let mut buf: Vec<u8> = Vec::new();
    let mut matches = 0usize;
    loop {
        if pos == 0 || end_at - pos >= MAX_READ_BYTES {
            break;
        }
        let from = pos
            .saturating_sub(BLOCK)
            .max(end_at.saturating_sub(MAX_READ_BYTES));
        let mut chunk = vec![0u8; (pos - from) as usize];
        file.seek(SeekFrom::Start(from))?;
        file.read_exact(&mut chunk)?;
        // Rough count on raw lines to know when to stop; grouping happens once at the end.
        matches += chunk
            .split(|b| *b == b'\n')
            .filter(|l| raw_maybe_passes(l, filter))
            .count();
        chunk.extend_from_slice(&buf);
        buf = chunk;
        pos = from;
        if matches > limit + 1 {
            break;
        }
    }

    // The end may hold a partial last line (still being written): leave it for following.
    let complete = match buf.iter().rposition(|b| *b == b'\n') {
        Some(i) => i + 1,
        None if end_at == size => 0,
        None => buf.len(),
    };
    let end = pos + complete as u64;
    let body = &buf[..complete];
    // Unless reading began at the file start, the first piece is a partial line: drop it.
    let skip = if pos == 0 {
        0
    } else {
        body.iter()
            .position(|b| *b == b'\n')
            .map_or(body.len(), |i| i + 1)
    };
    let first = pos + skip as u64;
    let mut lines = Vec::new();
    let mut offset = first;
    for raw in body[skip..].split_inclusive(|b| *b == b'\n') {
        let text = String::from_utf8_lossy(raw);
        if !text.trim().is_empty() {
            lines.push(parse(offset, &text));
        }
        offset += raw.len() as u64;
    }

    let entries = group(lines);
    let mut picked: Vec<Entry> = Vec::new();
    let mut count = 0;
    for entry in entries.into_iter().rev() {
        if !entry.passes(filter) {
            continue;
        }
        if count > 0 && count + entry.lines.len() > limit {
            break;
        }
        count += entry.lines.len();
        picked.push(entry);
    }
    picked.reverse();
    let lines: Vec<LogLine> = picked.into_iter().flat_map(|e| e.lines).collect();
    let start = lines.first().map_or(first, |l| l.offset);
    let read_everything_wanted = count >= limit;
    Ok(LinesPage {
        start,
        end,
        size,
        more: start > 0,
        truncated: !read_everything_wanted && pos > 0,
        lines,
    })
}

/// A cheap pre-check on a raw line (no parsing), used only to decide when enough has been read.
fn raw_maybe_passes(raw: &[u8], filter: &Filter) -> bool {
    if raw.is_empty() {
        return false;
    }
    if let Some(q) = &filter.query {
        let text = String::from_utf8_lossy(raw).to_lowercase();
        if !text.contains(q.as_str()) {
            return false;
        }
    }
    match filter.min_level {
        None => true,
        Some(min) => {
            let text = String::from_utf8_lossy(&raw[..raw.len().min(64)]);
            text.split_whitespace()
                .nth(1)
                .and_then(Level::parse)
                .is_some_and(|level| level >= min)
        }
    }
}

/// Lines appended since `from` (complete lines only, at most [`MAX_FOLLOW_BYTES`]), and the offset to continue
/// from. `None` when the file is now shorter than `from` (it was rotated or replaced).
pub fn read_since(path: &Path, from: u64) -> std::io::Result<Option<(Vec<LogLine>, u64)>> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    if size < from {
        return Ok(None);
    }
    if size == from {
        return Ok(Some((Vec::new(), from)));
    }
    let take = (size - from).min(MAX_FOLLOW_BYTES);
    let mut buf = vec![0u8; take as usize];
    file.seek(SeekFrom::Start(from))?;
    file.read_exact(&mut buf)?;
    let complete = buf.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    let mut lines = Vec::new();
    let mut offset = from;
    for raw in buf[..complete].split_inclusive(|b| *b == b'\n') {
        let text = String::from_utf8_lossy(raw);
        if !text.trim().is_empty() {
            lines.push(parse(offset, &text));
        }
        offset += raw.len() as u64;
    }
    Ok(Some((lines, from + complete as u64)))
}

/// The whole file with secrets masked, for download.
pub fn read_redacted(path: &Path) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        out.push_str(&redact(line));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bb-logs-{name}-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn event(i: usize, level: &str) -> String {
        format!(
            "2026-10-31T10:{:02}:{:02}.000000Z {level:>5} boogiebox_server::scanner: line {i}\n",
            i / 60 % 60,
            i % 60
        )
    }

    #[test]
    fn parses_events_and_plain_lines() {
        let l = parse(
            7,
            "2026-09-04T21:19:03.048536Z  INFO boogiebox_server::scanner: queued 1 scan job(s)\r\n",
        );
        assert_eq!(l.offset, 7);
        assert_eq!(l.ts.as_deref(), Some("2026-09-04T21:19:03.048536Z"));
        assert_eq!(l.level, Some(Level::Info));
        assert_eq!(l.target.as_deref(), Some("boogiebox_server::scanner"));
        assert_eq!(l.text, "queued 1 scan job(s)");

        let l = parse(0, "2026-09-04T21:19:03Z ERROR no target here");
        assert_eq!(
            (l.level, l.target.as_deref(), l.text.as_str()),
            (Some(Level::Error), None, "no target here")
        );

        for plain in [
            "BoogieBox server log initialized",
            "    stderr: boom",
            "2026 is a year: yes",
        ] {
            let l = parse(0, plain);
            assert_eq!((l.ts, l.level, l.text.as_str()), (None, None, plain));
        }
        let long = "x".repeat(MAX_LINE_BYTES + 50);
        assert!(parse(0, &long).text.ends_with('…'));
    }

    #[test]
    fn masks_secrets() {
        let cases = [
            (
                "GET /rest/ping?u=admin&p=hunter2&v=1",
                "GET /rest/ping?u=admin&p=•••••&v=1",
            ),
            (
                "url=https://x/?method=a&api_key=abc123&format=json",
                "url=https://x/?method=a&api_key=•••••&format=json",
            ),
            ("token=xyz rest", "token=••••• rest"),
            ("Authorization: Discogs token=abc", "Authorization: •••••"),
            (
                "got Bearer eyJhbGciOi and more",
                "got Bearer ••••• and more",
            ),
            ("password: secret1, user=bob", "password: •••••, user=bob"),
            ("&t=abc&s=salt", "&t=•••••&s=•••••"),
            ("Björk — Jóga… token=abc", "Björk — Jóga… token=•••••"),
            ("cookie: a=b\r\n", "cookie: •••••\r\n"),
        ];
        for (input, want) in cases {
            assert_eq!(redact(input), want, "{input}");
        }
        // Words that merely contain a key, and bare `p=`/`t=`/`s=` outside a query, stay.
        for keep in [
            "status=200 tokens=3",
            "elapsed_ms=12 sp=1",
            "p=1 t=2 s=3",
            "no secrets here",
        ] {
            assert_eq!(redact(keep), keep);
        }
    }

    #[test]
    fn keys_name_only_known_files() {
        let dir = temp_dir("keys");
        let server = dir.join("boogiebox-server.log");
        fs::write(&server, "a\n").unwrap();
        fs::write(dir.join("boogiebox-server.log.1"), "b\n").unwrap();
        fs::write(dir.join(format!("{SCAN_DEBUG_PREFIX}2026-10-30.log")), "").unwrap();
        fs::write(
            dir.join(format!("{SCAN_DEBUG_PREFIX}2026-10-31.log")),
            "c\n",
        )
        .unwrap();
        fs::write(dir.join(format!("{DEEP_DEBUG_PREFIX}notadate.log")), "").unwrap();
        fs::write(dir.join("other.log"), "").unwrap();

        assert_eq!(resolve(&server, "server"), Some(server.clone()));
        assert!(resolve(&server, "server.1").is_some());
        assert!(resolve(&server, "scan.2026-10-31").is_some());
        for bad in [
            "scan.2026-10-29",
            "deep.notadate",
            "scan.../x",
            "../server",
            "other",
            "scan.2026-10-31/../x",
            "",
        ] {
            assert!(resolve(&server, bad).is_none(), "{bad}");
        }

        let listing = list(&server, true, false);
        assert_eq!(
            listing.folder.as_deref(),
            Some(dir.display().to_string().as_str())
        );
        let keys = |i: usize| {
            listing.sources[i]
                .files
                .iter()
                .map(|f| (f.key.as_str(), f.current))
                .collect::<Vec<_>>()
        };
        assert_eq!(keys(0), [("server", true), ("server.1", false)]);
        assert_eq!(
            keys(1),
            [("scan.2026-10-31", true), ("scan.2026-10-30", false)]
        );
        assert!(keys(2).is_empty());
        assert!(listing.sources[1].enabled && !listing.sources[2].enabled);

        assert_eq!(
            newer_key(&server, "scan.2026-10-30").as_deref(),
            Some("scan.2026-10-31")
        );
        assert_eq!(newer_key(&server, "scan.2026-10-31"), None);
        assert_eq!(newer_key(&server, "server"), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed() {
        let dir = temp_dir("symlink");
        let server = dir.join("boogiebox-server.log");
        fs::write(dir.join("secret.txt"), "x").unwrap();
        std::os::unix::fs::symlink(dir.join("secret.txt"), &server).unwrap();
        assert!(resolve(&server, "server").is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pages_backwards_across_blocks() {
        let dir = temp_dir("pages");
        let path = dir.join("boogiebox-server.log");
        let mut text = String::new();
        // About 900 KB: several read blocks.
        for i in 0..12_000 {
            text.push_str(&event(i, "INFO"));
        }
        text.push_str("partial line still being writ");
        fs::write(&path, &text).unwrap();
        let all = Filter::default();

        let newest = read_page(&path, None, 500, &all).unwrap();
        assert_eq!(newest.lines.len(), 500);
        assert!(newest.lines.last().unwrap().text.ends_with("line 11999"));
        assert!(newest.lines[0].text.ends_with("line 11500"));
        assert!(newest.more && !newest.truncated);
        assert_eq!(
            newest.end,
            (text.len() - "partial line still being writ".len()) as u64
        );

        let older = read_page(&path, Some(newest.start), 500, &all).unwrap();
        assert!(older.lines.last().unwrap().text.ends_with("line 11499"));
        assert!(older.lines[0].text.ends_with("line 11000"));

        let first = read_page(&path, Some(older.lines[1].offset), 10, &all).unwrap();
        assert!(first.lines.last().unwrap().text.ends_with("line 11000"));
        assert_eq!(first.lines.len(), 10);
        let start = read_page(&path, Some(event(0, "INFO").len() as u64 * 3), 10, &all).unwrap();
        assert_eq!(start.lines.len(), 3);
        assert!(!start.more);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn filters_by_level_and_text_and_keeps_continuations() {
        let dir = temp_dir("filter");
        let path = dir.join("boogiebox-server.log");
        let text = [
            "BoogieBox server log initialized\n".to_owned(),
            event(1, "INFO"),
            "2026-10-31T10:00:02.000000Z ERROR boogiebox_server::deep_analysis: job failed\n"
                .into(),
            "    stderr: ModuleNotFoundError\n".into(),
            "BoogieBox Rust server starting; version=0.9.61\n".into(),
            "2026-10-31T10:00:03.000000Z  WARN boogiebox_server::post_scan: claim failed\n".into(),
            "2026-10-31T10:00:04.000000Z DEBUG boogiebox_server::scanner: detail\n".into(),
        ]
        .concat();
        fs::write(&path, text).unwrap();
        let texts = |filter: Filter| {
            read_page(&path, None, 100, &filter)
                .unwrap()
                .lines
                .into_iter()
                .map(|l| l.text)
                .collect::<Vec<_>>()
        };
        assert_eq!(texts(Filter::default()).len(), 7);
        assert_eq!(
            texts(Filter {
                min_level: Some(Level::Warn),
                query: None
            }),
            [
                "job failed",
                "    stderr: ModuleNotFoundError",
                "claim failed"
            ]
        );
        assert_eq!(
            texts(Filter {
                min_level: None,
                query: Some("modulenotfound".into())
            }),
            ["job failed", "    stderr: ModuleNotFoundError"]
        );
        assert_eq!(
            texts(Filter {
                min_level: None,
                query: Some("deep_analysis".into())
            }),
            ["job failed", "    stderr: ModuleNotFoundError"],
            "the module name is searchable"
        );
        assert!(texts(Filter {
            min_level: Some(Level::Error),
            query: Some("nothing".into())
        })
        .is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn stops_at_the_read_cap() {
        let dir = temp_dir("cap");
        let path = dir.join("boogiebox-server.log");
        let mut f = File::create(&path).unwrap();
        let line = event(1, "INFO");
        for _ in 0..(MAX_READ_BYTES as usize / line.len() + 2000) {
            f.write_all(line.as_bytes()).unwrap();
        }
        drop(f);
        let page = read_page(
            &path,
            None,
            100,
            &Filter {
                min_level: Some(Level::Error),
                query: None,
            },
        )
        .unwrap();
        assert!(page.lines.is_empty());
        assert!(page.truncated);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn follows_appends_partial_lines_and_rotation() {
        let dir = temp_dir("follow");
        let path = dir.join("boogiebox-server.log");
        fs::write(&path, event(1, "INFO")).unwrap();
        let start = fs::metadata(&path).unwrap().len();
        assert_eq!(read_since(&path, start).unwrap(), Some((Vec::new(), start)));

        // A writer keeps appending while we read (both platforms allow it).
        let mut writer = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writer.write_all(event(2, "WARN").as_bytes()).unwrap();
        writer.write_all(b"half a li").unwrap();
        writer.flush().unwrap();
        let (lines, next) = read_since(&path, start).unwrap().unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].level, Some(Level::Warn));
        writer.write_all(b"ne\n").unwrap();
        writer.flush().unwrap();
        let (lines, _) = read_since(&path, next).unwrap().unwrap();
        assert_eq!(lines[0].text, "half a line");
        drop(writer);

        fs::write(&path, "").unwrap();
        assert_eq!(
            read_since(&path, next).unwrap(),
            None,
            "a shorter file was rotated"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn download_is_masked() {
        let dir = temp_dir("download");
        let path = dir.join("boogiebox-server.log");
        fs::write(&path, "a token=abc\nb ok\n").unwrap();
        assert_eq!(read_redacted(&path).unwrap(), "a token=•••••\nb ok\n");
        let _ = fs::remove_dir_all(&dir);
    }
}
