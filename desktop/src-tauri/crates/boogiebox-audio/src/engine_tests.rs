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
        period_div: 100,
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
                resume_within: Duration::from_millis(400),
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
        let rec = self.factory.last_recording().unwrap();
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
    assert_eq!(
        path,
        [PathKind::SharedMixer],
        "shared mode: Windows' mixer takes it from here"
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
    assert_eq!(
        path,
        [PathKind::SharedMixer, PathKind::Resampled { to_hz: 48_000 }]
    );
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

    // Exclusive control turned off in Windows: said at once, not at the next play.
    run.send(Command::SetOutput {
        device_id: None,
        mode: OutputMode::Exclusive,
        buffer_ms: 50,
    });
    run.until("exclusive refused", |e| {
        matches!(
            e,
            Event::Error {
                code: ErrorCode::ExclusiveNotAllowed,
                ..
            }
        )
    });
}

// ---- Phase 3: exclusive mode -----------------------------------------------------------------------

use crate::negotiate::{DeviceCaps, RATES};
use crate::OutputFactory as _;

/// Like the user's FiiO: every rate, 16-bit, 24-in-32 and 32-bit.
fn fiio_caps(rates: &[u32]) -> DeviceCaps {
    DeviceCaps {
        formats: rates
            .iter()
            .flat_map(|r| {
                [SampleKind::I16, SampleKind::I24In32, SampleKind::I32].map(|kind| DeviceFormat {
                    sample_rate: *r,
                    channels: 2,
                    kind,
                })
            })
            .collect(),
    }
}

/// A null device at ~2x real time with exclusive formats.
fn exclusive_run(caps: Option<DeviceCaps>) -> Run {
    let f = paced(SampleKind::F32, 48_000, 2, Duration::from_millis(5));
    *f.exclusive.lock().unwrap() = caps;
    let run = Run::new(f);
    run.send(Command::SetOutput {
        device_id: None,
        mode: OutputMode::Exclusive,
        buffer_ms: 50,
    });
    run
}

/// Distinct, recognisable 24-bit samples (left-justified).
fn pattern24(frames: usize, seed: i32) -> Vec<i32> {
    (0..frames as i32 * 2)
        .map(|i| ((i * 1_543 + seed) % 8_000_000 - 4_000_000) << 8)
        .collect()
}

fn words(bytes: &[u8]) -> Vec<i32> {
    bytes
        .chunks_exact(4)
        .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// The non-silent stretch of a recording (24-in-32 words).
fn sound(words: &[i32]) -> &[i32] {
    let first = words.iter().position(|w| *w != 0).unwrap_or(0);
    let last = words.iter().rposition(|w| *w != 0).map_or(0, |i| i + 1);
    &words[first..last]
}

#[test]
fn exclusive_mode_follows_each_items_rate_bit_for_bit() {
    let server = TestServer::start();
    let a = ramp(2, 44_100, 1);
    let b = pattern24(96_000, 7);
    let c = pattern24(192_000 / 2, 11);
    server.put("/a", wav_bytes(44_100, 2, 16, &a), Behaviour::Normal);
    server.put("/b", wav_bytes(96_000, 2, 24, &b), Behaviour::Normal);
    server.put("/c", wav_bytes(192_000, 2, 24, &c), Behaviour::Normal);
    let mut run = exclusive_run(Some(fiio_caps(&RATES)));
    run.send(Command::Load {
        item: item(&server, "/a", "A"),
        start_sec: 0.0,
        autoplay: true,
    });
    run.send(Command::SetNext {
        item: Some(item(&server, "/b", "B")),
    });
    let started = run.until("A", |e| matches!(e, Event::TrackStarted { .. }));
    let Event::TrackStarted { path, output, .. } = started else {
        unreachable!()
    };
    assert_eq!(path, [PathKind::BitPerfect]);
    assert_eq!((output.sample_rate, output.codec.as_str()), (44_100, "s16"));
    run.until(
        "B",
        |e| matches!(e, Event::TrackStarted { track_id, .. } if track_id == "B"),
    );
    run.send(Command::SetNext {
        item: Some(item(&server, "/c", "C")),
    });
    run.until_stopped();
    assert_eq!(
        run.track_events(),
        ["start A", "end A", "start B", "end B", "start C", "end C"]
    );
    // Nothing about an item before it is heard.
    for (i, e) in run.seen.iter().enumerate() {
        if let Event::NeedNext { track_id } = e {
            let started = run.seen[..i]
                .iter()
                .any(|p| matches!(p, Event::TrackStarted { track_id: s, .. } if s == track_id));
            assert!(started, "NeedNext for {track_id} before it started");
        }
    }
    let opened = run.factory.opened.lock().unwrap().clone();
    let formats: Vec<(u32, SampleKind, bool)> = opened
        .iter()
        .map(|(f, excl, _)| (f.sample_rate, f.kind, *excl))
        .collect();
    assert_eq!(
        formats,
        [
            (44_100, SampleKind::I16, true),
            (96_000, SampleKind::I24In32, true),
            (192_000, SampleKind::I24In32, true)
        ],
        "the device follows every item's own rate (D8)"
    );
    let rec = |i: usize| opened[i].2.lock().unwrap().clone();
    assert_eq!(
        trimmed_i16_frames(&rec(0)),
        expected_i16(&a),
        "16/44.1 bit for bit"
    );
    assert_eq!(
        sound(&words(&rec(1))),
        &b[..],
        "24/96 bit for bit in 24-in-32"
    );
    assert_eq!(sound(&words(&rec(2))), &c[..], "24/192 bit for bit");
}

#[test]
fn same_format_items_stay_gapless_on_one_exclusive_stream() {
    let server = TestServer::start();
    let a = pattern24(48_000, 3);
    let b = pattern24(24_000, 5);
    server.put("/a", wav_bytes(96_000, 2, 24, &a), Behaviour::Normal);
    server.put("/b", wav_bytes(96_000, 2, 24, &b), Behaviour::Normal);
    let mut run = exclusive_run(Some(fiio_caps(&RATES)));
    run.send(Command::Load {
        item: item(&server, "/a", "A"),
        start_sec: 0.0,
        autoplay: true,
    });
    run.send(Command::SetNext {
        item: Some(item(&server, "/b", "B")),
    });
    run.until_stopped();
    assert_eq!(run.factory.opened.lock().unwrap().len(), 1, "no reopen");
    let mut both = a.clone();
    both.extend_from_slice(&b);
    assert_eq!(sound(&words(&run.recording())), &both[..]);
}

#[test]
fn a_rate_the_device_lacks_is_resampled_to_the_nearest_one_it_takes() {
    let server = TestServer::start();
    server.put(
        "/hi",
        wav_bytes(176_400, 2, 24, &pattern24(17_640, 1)),
        Behaviour::Normal,
    );
    // The Realtek S/PDIF output from the phase 0 spike: no 176.4 kHz.
    let mut run = exclusive_run(Some(fiio_caps(&[44_100, 48_000, 88_200, 96_000, 192_000])));
    run.send(Command::Load {
        item: item(&server, "/hi", "H"),
        start_sec: 0.0,
        autoplay: true,
    });
    let started = run.until("started", |e| matches!(e, Event::TrackStarted { .. }));
    let Event::TrackStarted { path, output, .. } = started else {
        unreachable!()
    };
    assert_eq!(path, [PathKind::Resampled { to_hz: 88_200 }]);
    assert_eq!(output.sample_rate, 88_200);
    run.until_stopped();
}

#[test]
fn the_badge_follows_the_volume() {
    let server = TestServer::start();
    server.put(
        "/a",
        wav_bytes(96_000, 2, 24, &pattern24(96_000 * 4, 1)),
        Behaviour::Normal,
    );
    let mut run = exclusive_run(Some(fiio_caps(&RATES)));
    run.send(Command::Load {
        item: item(&server, "/a", "A"),
        start_sec: 0.0,
        autoplay: true,
    });
    run.until(
        "bit-perfect",
        |e| matches!(e, Event::TrackStarted { path, .. } if path == &[PathKind::BitPerfect]),
    );
    run.send(Command::SetVolume { linear: 0.5 });
    run.until(
        "volume applied",
        |e| matches!(e, Event::PathChanged { path, .. } if path == &[PathKind::VolumeApplied]),
    );
    run.send(Command::SetVolume { linear: 0.7 });
    run.send(Command::SetVolume { linear: 1.0 });
    run.until(
        "bit-perfect again",
        |e| matches!(e, Event::PathChanged { path, .. } if path == &[PathKind::BitPerfect]),
    );
    let changes = run
        .seen
        .iter()
        .filter(|e| matches!(e, Event::PathChanged { .. }))
        .count();
    assert_eq!(changes, 2, "only when it crosses 100%");
    run.send(Command::Stop);
    run.until_stopped();
}

#[test]
fn refused_exclusive_mode_plays_shared_and_says_so() {
    let server = TestServer::start();
    server.put(
        "/a",
        wav_bytes(48_000, 2, 16, &ramp(2, 4_800, 1)),
        Behaviour::Normal,
    );

    // Windows doesn't allow exclusive control.
    let mut run = exclusive_run(None);
    run.until("not allowed", |e| {
        matches!(
            e,
            Event::Error {
                code: ErrorCode::ExclusiveNotAllowed,
                ..
            }
        )
    });
    run.send(Command::Load {
        item: item(&server, "/a", "A"),
        start_sec: 0.0,
        autoplay: true,
    });
    let started = run.until("started", |e| matches!(e, Event::TrackStarted { .. }));
    assert!(
        matches!(started, Event::TrackStarted { ref path, .. } if path == &[PathKind::SharedMixer])
    );
    run.until_stopped();

    // Another app holds the device.
    let mut run = exclusive_run(Some(fiio_caps(&RATES)));
    run.factory
        .busy
        .store(true, std::sync::atomic::Ordering::Relaxed);
    run.send(Command::Load {
        item: item(&server, "/a", "A"),
        start_sec: 0.0,
        autoplay: true,
    });
    let err = run.until("busy", |e| matches!(e, Event::Error { .. }));
    assert!(
        matches!(err, Event::Error { code: ErrorCode::DeviceBusy, ref message, .. } if message.contains("shared mode instead"))
    );
    run.until_stopped();
    let opened = run.factory.opened.lock().unwrap().clone();
    assert_eq!(opened.len(), 1);
    assert!(!opened[0].1, "played in shared mode");
}

#[test]
fn a_lost_device_pauses_and_resumes_when_it_comes_back() {
    let server = TestServer::start();
    server.put(
        "/long",
        wav_bytes(44_100, 2, 16, &ramp(2, 44_100 * 8, 1)),
        Behaviour::Normal,
    );
    let mut run = Run::new(paced(SampleKind::I16, 44_100, 2, Duration::from_millis(5)));
    let devices = run.factory.devices();
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
    std::thread::sleep(Duration::from_millis(100));
    // Unplugged: the device disappears and its stream fails.
    run.factory.set_devices(Vec::new());
    let shared = run.factory.last_shared.lock().unwrap().clone().unwrap();
    shared.fail(crate::OutputError::new(ErrorCode::DeviceLost, "unplugged"));
    run.until("device lost", |e| {
        matches!(
            e,
            Event::Error {
                code: ErrorCode::DeviceLost,
                ..
            }
        )
    });
    let paused = run.until("paused", |e| {
        matches!(
            e,
            Event::State {
                status: Status::Paused,
                ..
            }
        )
    });
    let Event::State {
        position_sec: at, ..
    } = paused
    else {
        unreachable!()
    };
    assert!(at > 0.0);
    // Plugged back in within the window: picks up where it stopped.
    run.factory.set_devices(devices);
    run.until(
        "device list",
        |e| matches!(e, Event::DeviceChanged { devices, .. } if !devices.is_empty()),
    );
    let resumed = run.until("resumed", |e| {
        matches!(
            e,
            Event::State {
                status: Status::Playing,
                ..
            }
        )
    });
    let Event::State { position_sec, .. } = resumed else {
        unreachable!()
    };
    assert!(
        (position_sec - at).abs() < 0.5,
        "resumed at {position_sec}, lost at {at}"
    );
    run.send(Command::Stop);
    run.until_stopped();

    // Gone for good: stops after the window.
    run.send(Command::Load {
        item: item(&server, "/long", "L"),
        start_sec: 0.0,
        autoplay: true,
    });
    run.until("playing again", |e| {
        matches!(
            e,
            Event::State {
                status: Status::Playing,
                ..
            }
        )
    });
    run.factory.set_devices(Vec::new());
    let shared = run.factory.last_shared.lock().unwrap().clone().unwrap();
    shared.fail(crate::OutputError::new(ErrorCode::DeviceLost, "unplugged"));
    run.until("paused", |e| {
        matches!(
            e,
            Event::State {
                status: Status::Paused,
                ..
            }
        )
    });
    run.until_stopped();
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
    // Let the output finish the buffer it was writing when the stop came.
    std::thread::sleep(Duration::from_millis(20));
    let len = run.recording().len();
    std::thread::sleep(Duration::from_millis(60));
    let frames = trimmed_i16_frames(&run.recording());
    assert!(!frames.is_empty());
    assert!(frames.iter().all(|f| f[0] == f[1]), "mono on both sides");
    assert!(frames.len() < 44_100 * 4, "stopped before the end");
    let after: Vec<u8> = run.recording()[len..].to_vec();
    assert!(after.iter().all(|b| *b == 0), "silence after stop");
}
