//! A tiny HTTP/1.1 `Range` server for tests (std only): serves byte blobs by path, with failure modes.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

/// How a path answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behaviour {
    /// 206 with the asked range.
    Normal,
    /// 503 for the first `n` requests, then normal.
    FailFirst(usize),
    /// Normal for `n` requests, then 410 Gone.
    ExpireAfter(usize),
    /// Ignores `Range` and answers 200 with the whole body.
    NoRange,
}

struct Entry {
    body: Arc<Vec<u8>>,
    behaviour: Behaviour,
    hits: usize,
}

type Files = Arc<Mutex<HashMap<String, Entry>>>;

/// Server handle; stops accepting when dropped (the thread exits on the next connection error).
pub struct TestServer {
    port: u16,
    files: Files,
}

impl TestServer {
    /// Starts on a free localhost port.
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let files: Files = Arc::default();
        let shared = files.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let files = shared.clone();
                std::thread::spawn(move || {
                    let _ = serve(stream, &files);
                });
            }
        });
        Self { port, files }
    }

    /// Adds or replaces `path`.
    pub fn put(&self, path: &str, body: Vec<u8>, behaviour: Behaviour) {
        self.files.lock().unwrap().insert(
            path.to_owned(),
            Entry {
                body: Arc::new(body),
                behaviour,
                hits: 0,
            },
        );
    }

    /// Full URL for `path`.
    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

fn serve(stream: TcpStream, files: &Files) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
        let mut range = None;
        loop {
            let mut h = String::new();
            reader.read_line(&mut h)?;
            let h = h.trim();
            if h.is_empty() {
                break;
            }
            if let Some((name, value)) = h.split_once(':') {
                if name.eq_ignore_ascii_case("range") {
                    range = value.trim().strip_prefix("bytes=").map(str::to_owned);
                }
            }
        }
        let answer = {
            let mut map = files.lock().unwrap();
            map.get_mut(&path).map(|e| {
                e.hits += 1;
                (e.body.clone(), e.behaviour, e.hits)
            })
        };
        let Some((body, behaviour, hits)) = answer else {
            write!(
                writer,
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n"
            )?;
            continue;
        };
        match behaviour {
            Behaviour::FailFirst(n) if hits <= n => {
                write!(writer, "HTTP/1.1 503 Busy\r\nContent-Length: 0\r\n\r\n")?;
                continue;
            }
            Behaviour::ExpireAfter(n) if hits > n => {
                write!(writer, "HTTP/1.1 410 Gone\r\nContent-Length: 0\r\n\r\n")?;
                continue;
            }
            Behaviour::NoRange => {
                write!(
                    writer,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                )?;
                writer.write_all(&body)?;
                continue;
            }
            _ => {}
        }
        let last = body.len().saturating_sub(1);
        let (a, b) = range
            .as_deref()
            .and_then(|r| r.split_once('-'))
            .map(|(a, b)| (a.parse().unwrap_or(0), b.parse().unwrap_or(last).min(last)))
            .unwrap_or((0, last));
        write!(
            writer,
            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {a}-{b}/{}\r\n\r\n",
            b + 1 - a,
            body.len()
        )?;
        writer.write_all(&body[a..=b])?;
    }
}
