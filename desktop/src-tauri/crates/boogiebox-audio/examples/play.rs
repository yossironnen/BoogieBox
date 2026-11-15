//! Manual check on a real device: plays local files through the engine (WASAPI shared mode).
//!
//! `cargo run --release --example play -- [--device <name part>] [--start <s>] [--seconds <n>] <file> [<next file>]`
//!
//! The files are served over a throwaway localhost `Range` server, so the engine reads them exactly as it
//! reads `/media/n/{token}` links.

#[cfg(windows)]
fn main() {
    if let Err(e) = windows_main::run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("this example needs Windows (WASAPI)");
}

#[cfg(windows)]
mod windows_main {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use boogiebox_audio::{Command, EngineConfig, Event, OutputMode, PlayItem, Status};

    struct Args {
        device: Option<String>,
        start: f64,
        seconds: Option<f64>,
        files: Vec<PathBuf>,
    }

    fn args() -> Result<Args, String> {
        let mut a = Args {
            device: None,
            start: 0.0,
            seconds: None,
            files: Vec::new(),
        };
        let mut it = std::env::args().skip(1);
        while let Some(x) = it.next() {
            match x.as_str() {
                "--device" => a.device = it.next(),
                "--start" => {
                    a.start = it
                        .next()
                        .and_then(|v| v.parse().ok())
                        .ok_or("--start <s>")?
                }
                "--seconds" => a.seconds = it.next().and_then(|v| v.parse().ok()),
                _ => a.files.push(PathBuf::from(x)),
            }
        }
        if a.files.is_empty() {
            return Err(
                "usage: play [--device <name part>] [--start <s>] [--seconds <n>] <file> [<next>]"
                    .into(),
            );
        }
        Ok(a)
    }

    /// Serves `files[i]` at `/i` with `Range` support.
    fn serve(files: Vec<Arc<Vec<u8>>>) -> Result<u16, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let files = files.clone();
                std::thread::spawn(move || -> std::io::Result<()> {
                    let mut reader = BufReader::new(stream.try_clone()?);
                    let mut w = stream;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line)? == 0 {
                            return Ok(());
                        }
                        let idx: usize = line
                            .split_whitespace()
                            .nth(1)
                            .and_then(|p| p.trim_start_matches('/').parse().ok())
                            .unwrap_or(usize::MAX);
                        let mut range = (0usize, usize::MAX);
                        loop {
                            let mut h = String::new();
                            reader.read_line(&mut h)?;
                            let h = h.trim();
                            if h.is_empty() {
                                break;
                            }
                            if let Some(v) = h.to_ascii_lowercase().strip_prefix("range: bytes=") {
                                let (a, b) = v.split_once('-').unwrap_or((v, ""));
                                range = (a.parse().unwrap_or(0), b.parse().unwrap_or(usize::MAX));
                            }
                        }
                        let Some(body) = files.get(idx) else {
                            write!(w, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")?;
                            continue;
                        };
                        let end = range.1.min(body.len().saturating_sub(1));
                        write!(
                            w,
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{end}/{}\r\n\r\n",
                            end + 1 - range.0,
                            range.0,
                            body.len()
                        )?;
                        w.write_all(&body[range.0..=end])?;
                    }
                });
            }
        });
        Ok(port)
    }

    pub fn run() -> Result<(), String> {
        let a = args()?;
        let bodies = a
            .files
            .iter()
            .map(|f| {
                std::fs::read(f)
                    .map(Arc::new)
                    .map_err(|e| format!("{}: {e}", f.display()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let port = serve(bodies)?;
        let devices = boogiebox_audio::list_devices();
        for d in &devices {
            println!(
                "device: {}{}",
                d.name,
                if d.is_default { " (default)" } else { "" }
            );
        }
        let device_id = match &a.device {
            Some(part) => Some(
                devices
                    .iter()
                    .find(|d| d.name.to_lowercase().contains(&part.to_lowercase()))
                    .map(|d| d.id.clone())
                    .ok_or_else(|| format!("no device matching {part:?}"))?,
            ),
            None => None,
        };
        let (engine, events) =
            boogiebox_audio::start(boogiebox_audio::default_factory(), EngineConfig::default());
        let item = |i: usize| PlayItem {
            track_id: a.files[i]
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            url: format!("http://127.0.0.1:{port}/{i}"),
            format_hint: a.files[i]
                .extension()
                .map(|e| e.to_string_lossy().into_owned()),
            duration_sec: None,
            server_decoded: false,
        };
        engine.send(Command::SetOutput {
            device_id,
            mode: OutputMode::Shared,
        });
        engine.send(Command::Load {
            item: item(0),
            start_sec: a.start,
            autoplay: true,
        });
        if a.files.len() > 1 {
            engine.send(Command::SetNext {
                item: Some(item(1)),
            });
        }
        let began = Instant::now();
        loop {
            if a.seconds.is_some_and(|s| began.elapsed().as_secs_f64() > s) {
                engine.send(Command::Stop);
            }
            match events.recv_timeout(Duration::from_millis(200)) {
                Ok(Event::State {
                    status,
                    track_id,
                    position_sec,
                    ..
                }) => {
                    println!(
                        "{:>6.2}s  {status:?} {} @ {position_sec:.2}",
                        began.elapsed().as_secs_f64(),
                        track_id.unwrap_or_default()
                    );
                    if status == Status::Stopped {
                        break;
                    }
                }
                Ok(e) => println!("{:>6.2}s  {e:?}", began.elapsed().as_secs_f64()),
                Err(_) => {}
            }
        }
        engine.send(Command::Shutdown);
        std::thread::sleep(Duration::from_millis(200));
        Ok(())
    }
}
