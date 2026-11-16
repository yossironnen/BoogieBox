//! `HttpRangeSource`: a seekable symphonia [`MediaSource`] over a native stream link (`/media/n/{token}`).
//!
//! Reads go through a block cache (256 KiB blocks, LRU, ~64 MB per item); a miss fetches the block and the
//! next two in one `Range` request. Network errors and 5xx answers are retried three times with backoff. A
//! 401/410 means the link expired: the source asks the host for a new one (through `on_expired`) and waits
//! for [`UrlSlot::set`] before carrying on.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use symphonia::core::io::MediaSource;

/// Cache block size.
pub const BLOCK: u64 = 256 * 1024;
/// Blocks fetched per request (the missing one plus read-ahead).
pub const FETCH_BLOCKS: u64 = 3;
/// Blocks kept per item (~64 MB).
pub const MAX_BLOCKS: usize = 256;
/// Network attempts per request.
pub const ATTEMPTS: u32 = 3;
/// How long to wait for a fresh link after the old one expired.
pub const REFRESH_WAIT: Duration = Duration::from_secs(15);

/// Why a read failed, carried inside the `io::Error` symphonia hands back.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// The link expired and no new one arrived in time.
    #[error("stream link expired")]
    TokenExpired,
    /// The server answered with a status that retrying won't fix.
    #[error("server answered {0}")]
    Status(u16),
    /// The server could not be reached (after retries).
    #[error("network: {0}")]
    Network(String),
    /// The server ignored the `Range` request.
    #[error("server does not support ranged reads")]
    NoRange,
    /// The engine dropped this item.
    #[error("cancelled")]
    Cancelled,
}

impl SourceError {
    /// The [`SourceError`] inside an `io::Error`, if it carries one.
    pub fn from_io(err: &io::Error) -> Option<&SourceError> {
        err.get_ref().and_then(|e| e.downcast_ref::<SourceError>())
    }
}

fn io_err(e: SourceError) -> io::Error {
    io::Error::other(e)
}

/// The current link for an item; replaced when the host sends a fresh one.
#[derive(Debug)]
pub struct UrlSlot {
    inner: Mutex<(String, u64)>,
    changed: Condvar,
}

impl UrlSlot {
    /// A slot holding `url`.
    pub fn new(url: &str) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new((url.to_owned(), 0)),
            changed: Condvar::new(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, (String, u64)> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Current URL and its version.
    pub fn get(&self) -> (String, u64) {
        self.lock().clone()
    }

    /// Installs a fresh URL and wakes a reader waiting for one.
    pub fn set(&self, url: &str) {
        let mut g = self.lock();
        g.0 = url.to_owned();
        g.1 += 1;
        self.changed.notify_all();
    }

    /// Waits until the version moves past `version` (or `timeout`, or `cancel`).
    fn wait_newer(&self, version: u64, timeout: Duration, cancel: &AtomicBool) -> Option<String> {
        let deadline = Instant::now() + timeout;
        let mut g = self.lock();
        while g.1 == version {
            let now = Instant::now();
            if now >= deadline || cancel.load(Ordering::Relaxed) {
                return None;
            }
            let wait = (deadline - now).min(Duration::from_millis(100));
            g = match self.changed.wait_timeout(g, wait) {
                Ok((g, _)) => g,
                Err(p) => p.into_inner().0,
            };
        }
        Some(g.0.clone())
    }
}

/// Called when an item's link expired (the engine turns it into [`crate::Event::TokenExpired`]).
pub type ExpiredHook = Arc<dyn Fn(&str) + Send + Sync>;

/// Options for [`HttpRangeSource::open`].
#[derive(Clone)]
pub struct SourceOptions {
    /// Item id (for the expiry hook).
    pub track_id: String,
    /// Current link.
    pub url: Arc<UrlSlot>,
    /// Asks the host for a fresh link.
    pub on_expired: ExpiredHook,
    /// Set when the engine drops the item: reads fail fast.
    pub cancel: Arc<AtomicBool>,
    /// First retry delay (doubles each time).
    pub backoff: Duration,
}

/// A seekable byte source over HTTP `Range` requests.
pub struct HttpRangeSource {
    client: reqwest::blocking::Client,
    opts: SourceOptions,
    len: u64,
    pos: u64,
    blocks: HashMap<u64, Arc<Vec<u8>>>,
    lru: VecDeque<u64>,
    requests: u64,
}

impl std::fmt::Debug for HttpRangeSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpRangeSource")
            .field("track_id", &self.opts.track_id)
            .field("len", &self.len)
            .field("pos", &self.pos)
            .field("cached_blocks", &self.blocks.len())
            .finish()
    }
}

fn http_client() -> io::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| io_err(SourceError::Network(e.to_string())))
}

impl HttpRangeSource {
    /// Opens the link: fetches the first blocks and learns the length from `Content-Range`.
    pub fn open(opts: SourceOptions) -> io::Result<Self> {
        let mut src = Self {
            client: http_client()?,
            opts,
            len: 0,
            pos: 0,
            blocks: HashMap::new(),
            lru: VecDeque::new(),
            requests: 0,
        };
        let (body, total) = src.get_range(0, BLOCK * FETCH_BLOCKS - 1)?;
        src.len = total;
        src.store(0, body);
        Ok(src)
    }

    /// Total length in bytes.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// True for an empty file.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Range requests sent so far.
    pub fn requests(&self) -> u64 {
        self.requests
    }

    /// Warms the cache with the first `bytes` of the file (gapless prefetch of the next item).
    pub fn prefetch(&mut self, bytes: u64) -> io::Result<()> {
        let last = bytes.min(self.len).div_ceil(BLOCK);
        let mut block = 0;
        while block < last {
            if !self.blocks.contains_key(&block) {
                self.fetch(block)?;
            }
            block += 1;
        }
        Ok(())
    }

    fn store(&mut self, first_block: u64, body: Vec<u8>) {
        for (i, chunk) in body.chunks(BLOCK as usize).enumerate() {
            let idx = first_block + i as u64;
            if self.blocks.insert(idx, Arc::new(chunk.to_vec())).is_none() {
                self.lru.push_back(idx);
            }
        }
        while self.blocks.len() > MAX_BLOCKS {
            match self.lru.pop_front() {
                Some(old) => {
                    self.blocks.remove(&old);
                }
                None => break,
            }
        }
    }

    fn fetch(&mut self, block: u64) -> io::Result<Arc<Vec<u8>>> {
        let start = block * BLOCK;
        let end = ((block + FETCH_BLOCKS) * BLOCK)
            .min(self.len)
            .saturating_sub(1);
        let (body, _) = self.get_range(start, end)?;
        self.store(block, body);
        self.touch(block);
        self.blocks
            .get(&block)
            .cloned()
            .ok_or_else(|| io_err(SourceError::Status(416)))
    }

    fn touch(&mut self, block: u64) {
        if let Some(i) = self.lru.iter().position(|b| *b == block) {
            self.lru.remove(i);
            self.lru.push_back(block);
        }
    }

    /// One `Range` request with retries and link refresh; returns the body and the file's total length.
    fn get_range(&mut self, start: u64, end: u64) -> io::Result<(Vec<u8>, u64)> {
        let mut attempt = 0;
        loop {
            if self.opts.cancel.load(Ordering::Relaxed) {
                return Err(io_err(SourceError::Cancelled));
            }
            let (url, version) = self.opts.url.get();
            self.requests += 1;
            let result = self
                .client
                .get(&url)
                .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
                .send();
            let retryable = match result {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    match status {
                        206 => {
                            let total = resp
                                .headers()
                                .get(reqwest::header::CONTENT_RANGE)
                                .and_then(|v| v.to_str().ok())
                                .and_then(|v| v.rsplit('/').next())
                                .and_then(|v| v.parse::<u64>().ok())
                                .ok_or_else(|| io_err(SourceError::NoRange))?;
                            match resp.bytes() {
                                Ok(body) => return Ok((body.to_vec(), total)),
                                Err(e) => SourceError::Network(e.to_string()),
                            }
                        }
                        200 => return Err(io_err(SourceError::NoRange)),
                        401 | 410 => {
                            (self.opts.on_expired)(&self.opts.track_id);
                            match self
                                .opts
                                .url
                                .wait_newer(version, REFRESH_WAIT, &self.opts.cancel)
                            {
                                Some(_) => continue,
                                None => return Err(io_err(SourceError::TokenExpired)),
                            }
                        }
                        s if s >= 500 => SourceError::Status(s),
                        s => return Err(io_err(SourceError::Status(s))),
                    }
                }
                Err(e) => SourceError::Network(e.to_string()),
            };
            attempt += 1;
            if attempt >= ATTEMPTS {
                return Err(io_err(retryable));
            }
            std::thread::sleep(self.opts.backoff * 2u32.saturating_pow(attempt - 1));
        }
    }
}

impl Read for HttpRangeSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let block = self.pos / BLOCK;
        let data = match self.blocks.get(&block).cloned() {
            Some(d) => {
                self.touch(block);
                d
            }
            None => self.fetch(block)?,
        };
        let offset = (self.pos % BLOCK) as usize;
        if offset >= data.len() {
            return Ok(0);
        }
        let n = (data.len() - offset).min(buf.len());
        buf[..n].copy_from_slice(&data[offset..offset + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for HttpRangeSource {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(p) => Some(p),
            SeekFrom::End(d) => self.len.checked_add_signed(d),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        };
        match target {
            Some(p) => {
                self.pos = p;
                Ok(p)
            }
            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before start",
            )),
        }
    }
}

impl MediaSource for HttpRangeSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

/// A live stream (internet radio): one plain GET read from the start, not seekable, no length. Reads fail
/// fast once the engine drops the item.
pub struct LiveStream {
    body: Box<dyn Read + Send + Sync>,
    cancel: Arc<AtomicBool>,
}

impl Read for LiveStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io_err(SourceError::Cancelled));
        }
        self.body.read(buf)
    }
}

/// Opens a live stream for symphonia (no `Range`; the connection stays open while playing).
pub fn open_live(
    opts: &SourceOptions,
) -> io::Result<symphonia::core::io::ReadOnlySource<LiveStream>> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(None)
        .build()
        .map_err(|e| io_err(SourceError::Network(e.to_string())))?;
    let (url, _) = opts.url.get();
    let resp = client
        .get(&url)
        .send()
        .map_err(|e| io_err(SourceError::Network(e.to_string())))?;
    let status = resp.status().as_u16();
    if status == 401 || status == 410 {
        return Err(io_err(SourceError::TokenExpired));
    }
    if !resp.status().is_success() {
        return Err(io_err(SourceError::Status(status)));
    }
    Ok(symphonia::core::io::ReadOnlySource::new(LiveStream {
        body: Box::new(resp),
        cancel: opts.cancel.clone(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_server::{Behaviour, TestServer};
    use std::sync::atomic::AtomicUsize;

    fn opts(server: &TestServer, path: &str) -> (SourceOptions, Arc<AtomicUsize>) {
        let expired = Arc::new(AtomicUsize::new(0));
        let hits = expired.clone();
        (
            SourceOptions {
                track_id: "t1".into(),
                url: UrlSlot::new(&server.url(path)),
                on_expired: Arc::new(move |_| {
                    hits.fetch_add(1, Ordering::Relaxed);
                }),
                cancel: Arc::new(AtomicBool::new(false)),
                backoff: Duration::from_millis(5),
            },
            expired,
        )
    }

    fn data(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn reads_and_seeks_across_blocks_with_few_requests() {
        let body = data(BLOCK as usize * 5 + 123);
        let server = TestServer::start();
        server.put("/a", body.clone(), Behaviour::Normal);
        let (o, _) = opts(&server, "/a");
        let mut src = HttpRangeSource::open(o).unwrap();
        assert_eq!(src.len(), body.len() as u64);
        let mut all = Vec::new();
        src.read_to_end(&mut all).unwrap();
        assert_eq!(all, body);
        assert_eq!(src.requests(), 2, "first fetch + one more (3 blocks each)");

        src.seek(SeekFrom::End(-10)).unwrap();
        let mut tail = [0u8; 10];
        src.read_exact(&mut tail).unwrap();
        assert_eq!(&tail[..], &body[body.len() - 10..]);
        src.seek(SeekFrom::Start(BLOCK + 7)).unwrap();
        let mut one = [0u8; 1];
        src.read_exact(&mut one).unwrap();
        assert_eq!(one[0], body[BLOCK as usize + 7]);
        assert!(src.seek(SeekFrom::Current(-(1 << 40))).is_err());
        assert!(src.is_seekable());
        assert_eq!(src.byte_len(), Some(body.len() as u64));
    }

    #[test]
    fn retries_server_errors_then_gives_up() {
        let server = TestServer::start();
        server.put("/flaky", data(1000), Behaviour::FailFirst(2));
        let (o, _) = opts(&server, "/flaky");
        let mut src = HttpRangeSource::open(o).unwrap();
        let mut all = Vec::new();
        src.read_to_end(&mut all).unwrap();
        assert_eq!(all.len(), 1000);

        server.put("/down", data(1000), Behaviour::FailFirst(10));
        let (o, _) = opts(&server, "/down");
        let err = HttpRangeSource::open(o).unwrap_err();
        assert_eq!(SourceError::from_io(&err), Some(&SourceError::Status(503)));

        let (o, _) = opts(&server, "/missing");
        let err = HttpRangeSource::open(o).unwrap_err();
        assert_eq!(SourceError::from_io(&err), Some(&SourceError::Status(404)));
    }

    #[test]
    fn an_expired_link_waits_for_a_fresh_one() {
        let server = TestServer::start();
        server.put("/old", data(BLOCK as usize * 4), Behaviour::ExpireAfter(1));
        server.put("/new", data(BLOCK as usize * 4), Behaviour::Normal);
        let (o, expired) = opts(&server, "/old");
        let slot = o.url.clone();
        let mut src = HttpRangeSource::open(o).unwrap();
        let fresh = server.url("/new");
        let refresher = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            slot.set(&fresh);
        });
        let mut all = Vec::new();
        src.read_to_end(&mut all).unwrap();
        refresher.join().unwrap();
        assert_eq!(all.len(), BLOCK as usize * 4);
        assert_eq!(expired.load(Ordering::Relaxed), 1, "asked the host once");
    }

    #[test]
    fn no_fresh_link_or_a_cancel_fails_the_read() {
        let server = TestServer::start();
        server.put("/old", data(BLOCK as usize * 4), Behaviour::ExpireAfter(1));
        let (o, _) = opts(&server, "/old");
        let cancel = o.cancel.clone();
        let mut src = HttpRangeSource::open(o).unwrap();
        cancel.store(true, Ordering::Relaxed);
        src.seek(SeekFrom::Start(BLOCK * 3)).unwrap();
        let err = src.read(&mut [0u8; 4]).unwrap_err();
        assert_eq!(SourceError::from_io(&err), Some(&SourceError::Cancelled));
    }

    #[test]
    fn prefetch_and_lru_keep_the_cache_bounded() {
        let server = TestServer::start();
        server.put("/big", data(BLOCK as usize * 9), Behaviour::Normal);
        let (o, _) = opts(&server, "/big");
        let mut src = HttpRangeSource::open(o).unwrap();
        src.prefetch(BLOCK * 6).unwrap();
        assert_eq!(src.requests(), 2);
        assert!(src.blocks.len() <= MAX_BLOCKS);
        server.put("/plain", data(10), Behaviour::NoRange);
        let (o, _) = opts(&server, "/plain");
        let err = HttpRangeSource::open(o).unwrap_err();
        assert_eq!(SourceError::from_io(&err), Some(&SourceError::NoRange));
    }
}
