//! End-to-end engine tests: test HTTP server → engine → null output (recorded, ~20x real time).

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use crate::convert::{DeviceFormat, SampleKind};
use crate::output::null::{NullConfig, NullFactory};
use crate::test_media::{ramp, wav_bytes};
use crate::test_server::{Behaviour, TestServer};
use crate::{
    start, Command, EngineConfig, EngineHandle, ErrorCode, Event, OutputMode, PathKind, PlayItem,
    Status,
};

const WAIT: Duration = Duration::from_secs(15);

fn factory(kind: SampleKind, rate: u32, channels: u16) -> NullFactory {
    paced(kind, rate, channels, Duration::from_micros(500))
}

/// A null device running at `10 ms / sleep` times real time (debug-build resampling is slow).
fn paced(kind: SampleKind, rate: u32, channels: u16, sleep: Duration) -> NullFactory {
    NullFactory::new(NullConfig {
        format: DeviceFormat {
            sample_rate: rate,
            channels,
            kind,
        },
        period_frames: (rate / 100) as usize,
        sleep,
        record: true,
    })
}

struct Run {
    engine: EngineHandle,
    events: Receiver<Event>,
    factory: NullFactory,
    seen: Vec<Event>,
}

impl Run {
    fn new(factory: NullFactory) -> Self {
        let (engine, events) = start(
            Box::new(factory.clone()),
            EngineConfig {
                backoff: Duration::from_millis(5),
            },
        );
        Self {
            engine,
            events,
            factory,
            seen: Vec::new(),
        }
    }

    fn send(&self, c: Command) {
        assert!(self.engine.send(c));
    }

    /// Waits for an event matching `pred` (keeping everything seen on the way).
    fn until(&mut self, what: &str, pred: impl Fn(&Event) -> bool) -> Event {
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            if let Ok(e) = self.events.recv_timeout(Duration::from_millis(50)) {
                self.seen.push(e.clone());
                if pred(&e) {
                    return e;
                }
            }
        }
        panic!("timed out waiting for {what}; saw {:#?}", self.seen);
    }

    fn until_stopped(&mut self) {
        self.until("stopped", |e| {
            matches!(
                e,
                Event::State {
                    status: Status::Stopped,
                    ..
                }
            )
        });
    }

    /// Track starts and ends, in order.
    fn track_events(&self) -> Vec<String> {
        self.seen
            .iter()
            .filter_map(|e| match e {
                Event::TrackStarted { track_id, .. } => Some(format!("start {track_id}")),
                Event::TrackEnded { track_id } => Some(format!("end {track_id}")),
                _ => None,
            })
            .collect()
    }

    fn recording(&self) -> Vec<u8> {
        let rec = self.factory.last_recording.lock().unwrap().clone().unwrap();
        let bytes = rec.lock().unwrap().clone();
        bytes
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = self.engine.send(Command::Shutdown);
    }
}

fn item(server: &TestServer, path: &str, id: &str) -> PlayItem {
    PlayItem {
        track_id: id.into(),
        url: server.url(path),
        format_hint: Some("wav".into()),
        duration_sec: None,
        server_decoded: false,
    }
}

/// 16-bit stereo frames from recorded device bytes, without the silence before and after.
fn trimmed_i16_frames(bytes: &[u8]) -> Vec<[i16; 2]> {
    let frames: Vec<[i16; 2]> = bytes
        .chunks_exact(4)
        .map(|c| {
            [
                i16::from_le_bytes([c[0], c[1]]),
                i16::from_le_bytes([c[2], c[3]]),
            ]
        })
        .collect();
    let first = frames.iter().position(|f| *f != [0, 0]).unwrap_or(0);
    let last = frames
        .iter()
        .rposition(|f| *f != [0, 0])
        .map_or(0, |i| i + 1);
    frames[first..last].to_vec()
}

fn expected_i16(samples: &[i32]) -> Vec<[i16; 2]> {
    samples
        .chunks_exact(2)
        .map(|c| [(c[0] >> 16) as i16, (c[1] >> 16) as i16])
        .collect()
}

#[test]
fn gapless_items_join_sample_exact_and_events_follow_the_sound() {
    let server = TestServer::start();
    let a = ramp(2, 22_050, 1);
    let b = ramp(2, 11_025, 22_051);
    server.put("/a", wav_bytes(44_100, 2, 16, &a), Behaviour::Normal);
    server.put("/b", wav_bytes(44_100, 2, 16, &b), Behaviour::Normal);
    let mut run = Run::new(factory(SampleKind::I16, 44_100, 2));
    run.send(Command::Load {
        item: item(&server, "/a", "A"),
        start_sec: 0.0,
        autoplay: true,
    });
    run.send(Command::SetNext {
        item: Some(item(&server, "/b", "B")),
    });
    let started = run.until("A started", |e| matches!(e, Event::TrackStarted { .. }));
    let Event::TrackStarted {
        source,
        output,
        path,
        ..
    } = started
    else {
        unreachable!()
    };
    assert_eq!(
        (source.sample_rate, source.bits, source.codec.as_str()),
        (44_100, Some(16), "PCM")
    );
    assert_eq!(output.codec, "s16");
    assert!(
        path.is_empty(),
        "same rate and channels on an integer device: nothing changes"
    );
    run.until_stopped();
    assert_eq!(run.track_events(), ["start A", "end A", "start B", "end B"]);
    let mut expected = expected_i16(&a);
    expected.extend(expected_i16(&b));
    assert_eq!(
        trimmed_i16_frames(&run.recording()),
        expected,
        "one unbroken ramp: no gap, no overlap"
    );
}

#[test]
fn twenty_four_bit_audio_reaches_a_24_in_32_device_bit_for_bit() {
    let server = TestServer::start();
    let samples: Vec<i32> = (1..=4_800)
        .map(|i: i32| (i * 1_543 - 3_000_000) << 8)
        .collect();
    server.put("/hi", wav_bytes(48_000, 2, 24, &samples), Behaviour::Normal);
    let mut run = Run::new(factory(SampleKind::I24In32, 48_000, 2));
    run.send(Command::Load {
        item: item(&server, "/hi", "hi"),
        start_sec: 0.0,
        autoplay: true,
    });
    run.until_stopped();
    let rec = run.recording();
    let words: Vec<i32> = rec
        .chunks_exact(4)
        .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let first = words.iter().position(|w| *w != 0).unwrap();
    assert_eq!(&words[first..first + samples.len()], &samples[..]);
}

#[test]
fn a_rate_change_is_resampled_for_a_float_mixer() {
    let server = TestServer::start();
    server.put(
        "/a",
        wav_bytes(44_100, 2, 16, &ramp(2, 22_050, 1)),
        Behaviour::Normal,
    );
    let mut run = Run::new(paced(SampleKind::F32, 48_000, 2, Duration::from_millis(5)));
    run.send(Command::Load {
        item: item(&server, "/a", "A"),
        start_sec: 0.0,
        autoplay: true,
    });
    let started = run.until("started", |e| matches!(e, Event::TrackStarted { .. }));
    let Event::TrackStarted { path, output, .. } = started else {
        unreachable!()
    };
    assert_eq!(path, [PathKind::Resampled { to_hz: 48_000 }]);
    assert_eq!((output.sample_rate, output.codec.as_str()), (48_000, "f32"));
    run.until_stopped();
    let rec = run.recording();
    let frames: Vec<f32> = rec
        .chunks_exact(8)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let first = frames.iter().position(|v| v.abs() > 1e-9).unwrap();
    let last = frames.iter().rposition(|v| v.abs() > 1e-9).unwrap();
    let length = last + 1 - first;
    assert!(
        (length as i64 - 24_000).abs() < 200,
        "half a second at 48 kHz: {length}"
    );
    let gaps = frames[first..=last]
        .iter()
        .filter(|v| v.abs() <= 1e-9)
        .count();
    assert!(gaps < 10, "no silence inside the item: {gaps}");
}

#[test]
fn load_at_a_position_seek_and_pause() {
    let server = TestServer::start();
    let long = ramp(2, 44_100 * 6, 1);
    server.put("/long", wav_bytes(44_100, 2, 16, &long), Behaviour::Normal);
    let mut run = Run::new(factory(SampleKind::I16, 44_100, 2));
    run.send(Command::Load {
        item: item(&server, "/long", "L"),
        start_sec: 0.25,
        autoplay: false,
    });
    run.until("paused", |e| {
        matches!(
            e,
            Event::State {
                status: Status::Paused,
                ..
            }
        )
    });
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        trimmed_i16_frames(&run.recording()).is_empty(),
        "paused: nothing played"
    );

    run.send(Command::Play);
    run.until("playing", |e| {
        matches!(
            e,
            Event::State {
                status: Status::Playing,
                ..
            }
        )
    });
    std::thread::sleep(Duration::from_millis(30));
    run.send(Command::Pause);
    run.until("paused again", |e| {
        matches!(
            e,
            Event::State {
                status: Status::Paused,
                ..
            }
        )
    });
    let first = trimmed_i16_frames(&run.recording())[0][0];
    assert!(
        (i32::from(first) - (11_025 + 1)).abs() <= 1,
        "started at 0.25 s: {first}"
    );
    let len_paused = trimmed_i16_frames(&run.recording()).len();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        trimmed_i16_frames(&run.recording()).len(),
        len_paused,
        "nothing consumed while paused"
    );

    run.send(Command::Seek { sec: 5.0 });
    run.send(Command::Play);
    run.until("position after the seek", |e| {
        matches!(e, Event::State { position_sec, status: Status::Playing, .. } if *position_sec >= 5.0)
    });
    run.until_stopped();
    assert_eq!(run.track_events().last().map(String::as_str), Some("end L"));
}

#[test]
fn volume_scales_and_short_items_ask_for_a_next_one() {
    let server = TestServer::start();
    let samples = vec![16_384 << 16; 2 * 44_100];
    server.put("/a", wav_bytes(44_100, 2, 16, &samples), Behaviour::Normal);
    let mut run = Run::new(paced(SampleKind::I16, 44_100, 2, Duration::from_millis(5)));
    run.send(Command::SetVolume { linear: 0.5 });
    run.send(Command::Load {
        item: item(&server, "/a", "A"),
        start_sec: 0.0,
        autoplay: true,
    });
    run.until(
        "need next",
        |e| matches!(e, Event::NeedNext { track_id } if track_id == "A"),
    );
    run.until_stopped();
    let frames = trimmed_i16_frames(&run.recording());
    let mid = frames[frames.len() / 2][0];
    assert!((i32::from(mid) - 8_192).abs() <= 1, "half volume: {mid}");
}

#[test]
fn a_changed_next_item_after_the_decoder_moved_on_still_wins() {
    let server = TestServer::start();
    for (path, frames) in [("/a", 44_100), ("/b", 4_410), ("/c", 4_410)] {
        server.put(
            path,
            wav_bytes(44_100, 2, 16, &ramp(2, frames, 1)),
            Behaviour::Normal,
        );
    }
    let mut run = Run::new(factory(SampleKind::I16, 44_100, 2));
    run.send(Command::Load {
        item: item(&server, "/a", "A"),
        start_sec: 0.0,
        autoplay: true,
    });
    run.send(Command::SetNext {
        item: Some(item(&server, "/b", "B")),
    });
    run.until(
        "A started",
        |e| matches!(e, Event::TrackStarted { track_id, .. } if track_id == "A"),
    );
    // A (1 s) is already decoded and B queued behind it in the ring.
    std::thread::sleep(Duration::from_millis(20));
    run.send(Command::SetNext {
        item: Some(item(&server, "/c", "C")),
    });
    run.until_stopped();
    let tracks = run.track_events();
    assert!(tracks.contains(&"start C".to_owned()), "{tracks:?}");
    assert!(!tracks.contains(&"start B".to_owned()), "{tracks:?}");
}

#[test]
fn an_expired_link_is_refreshed_through_the_host() {
    let server = TestServer::start();
    let body = wav_bytes(44_100, 2, 16, &ramp(2, 44_100 * 6, 1));
    server.put("/old", body.clone(), Behaviour::ExpireAfter(1));
    server.put("/new", body, Behaviour::Normal);
    let mut run = Run::new(factory(SampleKind::I16, 44_100, 2));
    run.send(Command::Load {
        item: item(&server, "/old", "A"),
        start_sec: 0.0,
        autoplay: true,
    });
    run.until(
        "token expired",
        |e| matches!(e, Event::TokenExpired { track_id } if track_id == "A"),
    );
    run.send(Command::RefreshUrl {
        track_id: "A".into(),
        url: server.url("/new"),
    });
    run.until_stopped();
    assert!(
        run.seen.iter().all(|e| !matches!(e, Event::Error { .. })),
        "{:#?}",
        run.seen
    );
    assert_eq!(trimmed_i16_frames(&run.recording()).len(), 44_100 * 6);
}

#[test]
fn errors_become_events_and_stop_playback() {
    let server = TestServer::start();
    let mut run = Run::new(factory(SampleKind::I16, 44_100, 2));
    run.send(Command::Load {
        item: item(&server, "/missing", "X"),
        start_sec: 0.0,
        autoplay: true,
    });
    let err = run.until("error", |e| matches!(e, Event::Error { .. }));
    assert!(
        matches!(err, Event::Error { code: ErrorCode::Network, track_id: Some(ref t), .. } if t == "X")
    );
    run.until_stopped();

    server.put("/junk", vec![7u8; 5_000], Behaviour::Normal);
    run.send(Command::Load {
        item: item(&server, "/junk", "J"),
        start_sec: 0.0,
        autoplay: true,
    });
    let err = run.until(
        "unsupported",
        |e| matches!(e, Event::Error { track_id: Some(t), .. } if t == "J"),
    );
    assert!(matches!(
        err,
        Event::Error {
            code: ErrorCode::FormatUnsupported,
            ..
        }
    ));

    // The device fails mid-song.
    server.put(
        "/long",
        wav_bytes(44_100, 2, 16, &ramp(2, 44_100 * 6, 1)),
        Behaviour::Normal,
    );
    run.send(Command::Load {
        item: item(&server, "/long", "L"),
        start_sec: 0.0,
        autoplay: true,
    });
    run.until("playing", |e| {
        matches!(
            e,
            Event::State {
                status: Status::Playing,
                ..
            }
        )
    });
    let shared = run.factory.last_shared.lock().unwrap().clone().unwrap();
    shared.fail(crate::OutputError::new(ErrorCode::DeviceLost, "unplugged"));
    let err = run.until("device lost", |e| {
        matches!(
            e,
            Event::Error {
                code: ErrorCode::DeviceLost,
                ..
            }
        )
    });
    assert!(matches!(err, Event::Error { track_id: Some(ref t), .. } if t == "L"));
    run.until_stopped();

    // Exclusive mode isn't built yet: a clear error, not a crash.
    run.send(Command::SetOutput {
        device_id: None,
        mode: OutputMode::Exclusive,
    });
    run.until("exclusive refused", |e| {
        matches!(
            e,
            Event::Error {
                code: ErrorCode::FormatUnsupported,
                ..
            }
        )
    });
}

#[test]
fn stop_silences_and_mono_fills_both_channels() {
    let server = TestServer::start();
    let mono: Vec<i32> = vec![1_000 << 16; 44_100 * 4];
    server.put("/mono", wav_bytes(44_100, 1, 16, &mono), Behaviour::Normal);
    let mut run = Run::new(factory(SampleKind::I16, 44_100, 2));
    run.send(Command::Load {
        item: item(&server, "/mono", "M"),
        start_sec: 0.0,
        autoplay: true,
    });
    run.until("playing", |e| {
        matches!(
            e,
            Event::State {
                status: Status::Playing,
                ..
            }
        )
    });
    std::thread::sleep(Duration::from_millis(30));
    run.send(Command::Stop);
    run.until_stopped();
    let len = run.recording().len();
    std::thread::sleep(Duration::from_millis(60));
    let frames = trimmed_i16_frames(&run.recording());
    assert!(!frames.is_empty());
    assert!(frames.iter().all(|f| f[0] == f[1]), "mono on both sides");
    assert!(frames.len() < 44_100 * 4, "stopped before the end");
    let after: Vec<u8> = run.recording()[len..].to_vec();
    assert!(after.iter().all(|b| *b == 0), "silence after stop");
}
