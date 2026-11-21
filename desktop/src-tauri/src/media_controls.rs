//! Windows media controls while "This PC" (the native engine) is the output (`wip/native-hires-engine-plan.md`
//! §11): the System Media Transport Controls (volume flyout, lock screen, keyboard media keys), the taskbar
//! thumbnail buttons and the song's progress on the taskbar icon.
//!
//! The page says what plays (`media_now_playing` / `media_clear`); the engine's events say how far along it is.
//! Button presses go back to the page as `media-control` events, because the page owns the queue (D1).
//!
//! [`Tracker`] works out what to show and is plain logic; the Windows calls live in the `platform` module.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use boogiebox_audio::{Event, Status};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

/// Tauri event the page gets when a media button is pressed.
pub const EVENT: &str = "media-control";
/// The timeline in Windows' controls is refreshed at most this often while playing.
const TIMELINE_EVERY: Duration = Duration::from_secs(5);
/// The taskbar progress is refreshed at most this often while playing.
const PROGRESS_EVERY: Duration = Duration::from_secs(1);
const MAX_TEXT: usize = 300;

/// What the page says is playing on "This PC".
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    pub title: String,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub album_artist: Option<String>,
    /// Album art (an absolute `http(s)` URL that needs no sign-in).
    #[serde(default)]
    pub art_url: Option<String>,
    #[serde(default)]
    pub duration_sec: Option<f64>,
    /// Internet radio: no timeline, no seeking, no progress.
    #[serde(default)]
    pub live: bool,
}

fn clip(s: String) -> String {
    s.trim().chars().take(MAX_TEXT).collect()
}

impl NowPlaying {
    /// Trims the texts and drops an art URL that isn't plain http(s).
    fn cleaned(mut self) -> Self {
        self.title = clip(self.title);
        self.artist = self.artist.map(clip).filter(|s| !s.is_empty());
        self.album = self.album.map(clip).filter(|s| !s.is_empty());
        self.album_artist = self.album_artist.map(clip).filter(|s| !s.is_empty());
        self.art_url = self
            .art_url
            .filter(|u| url::Url::parse(u).is_ok_and(|u| matches!(u.scheme(), "http" | "https")));
        self.duration_sec = self.duration_sec.filter(|d| d.is_finite() && *d > 0.0);
        self
    }
}

/// A button pressed in Windows, for the page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MediaAction {
    /// `play`, `pause`, `toggle`, `next`, `previous`, `stop` or `seek`.
    pub action: &'static str,
    /// Seek target (s).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sec: Option<f64>,
}

impl MediaAction {
    fn new(action: &'static str) -> Self {
        Self { action, sec: None }
    }
}

/// Playback status as Windows shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Playing,
    Paused,
    Stopped,
}

/// The taskbar icon's progress bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Off,
    /// Green: playing, `done` of `total` (ms).
    Playing {
        done: u64,
        total: u64,
    },
    /// Yellow: paused.
    Paused {
        done: u64,
        total: u64,
    },
}

/// One change for Windows' controls.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// Show this song (and turn the controls on).
    Show(NowPlaying),
    /// Nothing on "This PC" any more: controls off.
    Clear,
    Status(PlayState),
    /// Position and length (s); radio has none.
    Timeline {
        position: f64,
        duration: f64,
    },
    Progress(Progress),
}

/// Works out what Windows' controls should show from the page's song and the engine's events.
#[derive(Debug, Default)]
pub struct Tracker {
    now: Option<NowPlaying>,
    state: Option<PlayState>,
    position: f64,
    duration: Option<f64>,
    last_timeline: Option<Instant>,
    last_progress: Option<Instant>,
}

fn play_state(status: Status) -> PlayState {
    match status {
        Status::Playing | Status::Loading => PlayState::Playing,
        Status::Paused => PlayState::Paused,
        Status::Idle | Status::Stopped => PlayState::Stopped,
    }
}

impl Tracker {
    /// The page's song changed (`None`: "This PC" isn't the output or nothing plays).
    pub fn set_now(&mut self, now: Option<NowPlaying>) -> Vec<Update> {
        match now.map(NowPlaying::cleaned) {
            Some(np) => {
                if np.duration_sec.is_some() {
                    self.duration = np.duration_sec;
                }
                self.position = 0.0;
                self.now = Some(np.clone());
                let mut out = vec![Update::Show(np)];
                out.extend(self.state.map(Update::Status));
                self.last_timeline = None;
                self.last_progress = None;
                out.extend(self.timeline_and_progress(Instant::now(), true));
                out
            }
            None => {
                let was = self.now.take().is_some();
                self.state = None;
                if was {
                    vec![Update::Clear, Update::Progress(Progress::Off)]
                } else {
                    Vec::new()
                }
            }
        }
    }

    /// An engine event at time `at`.
    pub fn on_event(&mut self, event: &Event, at: Instant) -> Vec<Update> {
        let Event::State {
            status,
            position_sec,
            duration_sec,
            ..
        } = event
        else {
            return Vec::new();
        };
        self.position = *position_sec;
        if duration_sec.is_some() {
            self.duration = *duration_sec;
        }
        if self.now.is_none() {
            return Vec::new();
        }
        let state = play_state(*status);
        let changed = self.state != Some(state);
        self.state = Some(state);
        let mut out = Vec::new();
        if changed {
            out.push(Update::Status(state));
        }
        out.extend(self.timeline_and_progress(at, changed));
        out
    }

    fn timeline_and_progress(&mut self, at: Instant, force: bool) -> Vec<Update> {
        let live = self.now.as_ref().is_some_and(|n| n.live);
        let (Some(duration), false) = (self.duration, live) else {
            return if force {
                vec![Update::Progress(Progress::Off)]
            } else {
                Vec::new()
            };
        };
        let mut out = Vec::new();
        let position = self.position.clamp(0.0, duration);
        if force || self.last_timeline.is_none_or(|t| at - t >= TIMELINE_EVERY) {
            self.last_timeline = Some(at);
            out.push(Update::Timeline { position, duration });
        }
        if force || self.last_progress.is_none_or(|t| at - t >= PROGRESS_EVERY) {
            self.last_progress = Some(at);
            let (done, total) = ((position * 1000.0) as u64, (duration * 1000.0) as u64);
            out.push(Update::Progress(match self.state {
                Some(PlayState::Playing) => Progress::Playing { done, total },
                Some(PlayState::Paused) => Progress::Paused { done, total },
                _ => Progress::Off,
            }));
        }
        out
    }
}

/// Media controls state (managed by Tauri).
#[derive(Default)]
pub struct MediaControls {
    tracker: Mutex<Tracker>,
}

fn apply(app: &AppHandle, updates: Vec<Update>) {
    if updates.is_empty() {
        return;
    }
    #[cfg(windows)]
    platform::apply(app, updates);
    #[cfg(not(windows))]
    let _ = (app, updates);
}

/// Sets up Windows' controls for the main window (call once the window exists).
pub fn start(app: &AppHandle) {
    #[cfg(windows)]
    platform::start(app);
    #[cfg(not(windows))]
    let _ = app;
}

/// The engine sent an event (called from the shell's event thread).
pub fn on_engine_event(app: &AppHandle, controls: &MediaControls, event: &Event) {
    let updates = controls
        .tracker
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .on_event(event, Instant::now());
    apply(app, updates);
}

fn emit_action(app: &AppHandle, action: MediaAction) {
    if let Err(e) = app.emit(EVENT, &action) {
        tracing::warn!("media control not delivered: {e}");
    }
}

/// The page: this song plays on "This PC".
#[tauri::command]
pub fn media_now_playing(app: AppHandle, state: State<MediaControls>, now: NowPlaying) {
    let updates = state
        .tracker
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .set_now(Some(now));
    apply(&app, updates);
}

/// The page: nothing plays on "This PC" any more.
#[tauri::command]
pub fn media_clear(app: AppHandle, state: State<MediaControls>) {
    let updates = state
        .tracker
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .set_now(None);
    apply(&app, updates);
}

/// Anti-aliased 32-bit BGRA glyphs for the thumbnail buttons, drawn in code so they stay sharp at any scale.
pub mod glyphs {
    /// A button's glyph.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Glyph {
        Previous,
        Play,
        Pause,
        Next,
    }

    type Pt = (f32, f32);

    fn in_triangle(p: Pt, a: Pt, b: Pt, c: Pt) -> bool {
        let s =
            |p1: Pt, p2: Pt, p3: Pt| (p1.0 - p3.0) * (p2.1 - p3.1) - (p2.0 - p3.0) * (p1.1 - p3.1);
        let (d1, d2, d3) = (s(p, a, b), s(p, b, c), s(p, c, a));
        let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        !(neg && pos)
    }

    fn in_rect(p: Pt, x0: f32, y0: f32, x1: f32, y1: f32) -> bool {
        p.0 >= x0 && p.0 <= x1 && p.1 >= y0 && p.1 <= y1
    }

    /// Whether a point of the unit square (0..1) is inside the glyph.
    fn inside(glyph: Glyph, p: Pt) -> bool {
        match glyph {
            Glyph::Play => in_triangle(p, (0.28, 0.18), (0.28, 0.82), (0.8, 0.5)),
            Glyph::Pause => in_rect(p, 0.24, 0.2, 0.42, 0.8) || in_rect(p, 0.58, 0.2, 0.76, 0.8),
            Glyph::Next => {
                in_triangle(p, (0.2, 0.2), (0.2, 0.8), (0.64, 0.5))
                    || in_rect(p, 0.66, 0.2, 0.8, 0.8)
            }
            Glyph::Previous => {
                in_triangle(p, (0.8, 0.2), (0.8, 0.8), (0.36, 0.5))
                    || in_rect(p, 0.2, 0.2, 0.34, 0.8)
            }
        }
    }

    /// `size`×`size` premultiplied BGRA pixels, top row first, in colour `rgb`.
    pub fn render(glyph: Glyph, size: usize, rgb: (u8, u8, u8)) -> Vec<u8> {
        const SS: usize = 4;
        let mut out = Vec::with_capacity(size * size * 4);
        for y in 0..size {
            for x in 0..size {
                let mut hits = 0;
                for sy in 0..SS {
                    for sx in 0..SS {
                        let p = (
                            (x as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32,
                            (y as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32,
                        );
                        if inside(glyph, p) {
                            hits += 1;
                        }
                    }
                }
                let a = (hits * 255 / (SS * SS)) as u32;
                let pre = |c: u8| ((u32::from(c) * a) / 255) as u8;
                out.extend_from_slice(&[pre(rgb.2), pre(rgb.1), pre(rgb.0), a as u8]);
            }
        }
        out
    }
}

#[cfg(windows)]
mod platform {
    //! The Windows side: SMTC from the main window, the taskbar's thumbnail toolbar and progress.

    use std::cell::RefCell;
    use std::sync::OnceLock;

    use tauri::{AppHandle, Manager};
    use windows::core::{factory, HSTRING};
    use windows::Foundation::{TimeSpan, TypedEventHandler, Uri};
    use windows::Media::{
        MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
        SystemMediaTransportControls, SystemMediaTransportControlsButton,
        SystemMediaTransportControlsButtonPressedEventArgs,
        SystemMediaTransportControlsTimelineProperties,
    };
    use windows::Storage::Streams::RandomAccessStreamReference;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS, HBITMAP,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
    use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
    use windows::Win32::UI::Shell::{
        DefSubclassProc, ITaskbarList3, SetWindowSubclass, TaskbarList, TBPF_NOPROGRESS,
        TBPF_NORMAL, TBPF_PAUSED, THBF_ENABLED, THBF_HIDDEN, THB_FLAGS, THB_ICON, THB_TOOLTIP,
        THUMBBUTTON,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIconIndirect, RegisterWindowMessageW, HICON, ICONINFO, SM_CXSMICON, WM_COMMAND,
    };

    use super::glyphs::{render, Glyph};
    use super::{emit_action, MediaAction, PlayState, Progress, Update};

    static APP: OnceLock<AppHandle> = OnceLock::new();
    static SMTC: OnceLock<SystemMediaTransportControls> = OnceLock::new();
    static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

    const THBN_CLICKED: u32 = 0x1800;
    const ID_PREVIOUS: u32 = 1;
    const ID_TOGGLE: u32 = 2;
    const ID_NEXT: u32 = 3;

    /// The taskbar side, owned by the window's thread (COM objects there aren't shared).
    struct Taskbar {
        hwnd: HWND,
        list: Option<ITaskbarList3>,
        icons: [HICON; 4],
        added: bool,
        shown: bool,
        playing: bool,
    }

    thread_local! {
        static TASKBAR: RefCell<Option<Taskbar>> = const { RefCell::new(None) };
    }

    fn ticks(sec: f64) -> TimeSpan {
        TimeSpan {
            Duration: (sec.max(0.0) * 10_000_000.0) as i64,
        }
    }

    /// The taskbar uses a light theme (dark glyphs then).
    fn light_taskbar() -> bool {
        use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
        let mut value: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: reads one DWORD into a local of the right size.
        let ok = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                windows::core::w!(
                    "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"
                ),
                windows::core::w!("SystemUsesLightTheme"),
                RRF_RT_REG_DWORD,
                None,
                Some((&mut value as *mut u32).cast()),
                Some(&mut size),
            )
        };
        ok.is_ok() && value == 1
    }

    fn make_icon(glyph: Glyph, size: i32, rgb: (u8, u8, u8)) -> HICON {
        let px = render(glyph, size as usize, rgb);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: a top-down 32-bit DIB of `size`×`size`; `bits` is filled with exactly that many pixels.
        unsafe {
            let Ok(color) = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)
            else {
                return HICON::default();
            };
            if !bits.is_null() {
                std::ptr::copy_nonoverlapping(px.as_ptr(), bits.cast::<u8>(), px.len());
            }
            let mask: HBITMAP = CreateBitmap(size, size, 1, 1, None);
            let icon = CreateIconIndirect(&ICONINFO {
                fIcon: true.into(),
                hbmMask: mask,
                hbmColor: color,
                ..Default::default()
            })
            .unwrap_or_default();
            let _ = DeleteObject(color.into());
            let _ = DeleteObject(mask.into());
            icon
        }
    }

    fn tip(text: &str) -> [u16; 260] {
        let mut out = [0u16; 260];
        for (i, c) in text.encode_utf16().take(259).enumerate() {
            out[i] = c;
        }
        out
    }

    impl Taskbar {
        fn buttons(&self) -> [THUMBBUTTON; 3] {
            let flags = if self.shown {
                THBF_ENABLED
            } else {
                THBF_HIDDEN
            };
            let button = |id: u32, icon: HICON, text: &str| THUMBBUTTON {
                dwMask: THB_ICON | THB_TOOLTIP | THB_FLAGS,
                iId: id,
                hIcon: icon,
                szTip: tip(text),
                dwFlags: flags,
                ..Default::default()
            };
            let (toggle_icon, toggle_tip) = if self.playing {
                (self.icons[2], "Pause")
            } else {
                (self.icons[1], "Play")
            };
            [
                button(ID_PREVIOUS, self.icons[0], "Previous"),
                button(ID_TOGGLE, toggle_icon, toggle_tip),
                button(ID_NEXT, self.icons[3], "Next"),
            ]
        }

        /// Adds the buttons once the taskbar knows the window, else updates them.
        fn refresh(&mut self) {
            let Some(list) = self.list.as_ref() else {
                return;
            };
            let buttons = self.buttons();
            // SAFETY: COM calls on the window's thread with this window's handle.
            let done = unsafe {
                if self.added {
                    list.ThumbBarUpdateButtons(self.hwnd, &buttons)
                } else {
                    list.ThumbBarAddButtons(self.hwnd, &buttons)
                }
            };
            if done.is_ok() {
                self.added = true;
            }
        }

        fn progress(&self, progress: Progress) {
            let Some(list) = self.list.as_ref() else {
                return;
            };
            // SAFETY: COM calls on the window's thread with this window's handle.
            unsafe {
                let _ = match progress {
                    Progress::Off => list.SetProgressState(self.hwnd, TBPF_NOPROGRESS),
                    Progress::Playing { done, total } => list
                        .SetProgressState(self.hwnd, TBPF_NORMAL)
                        .and_then(|_| list.SetProgressValue(self.hwnd, done, total.max(1))),
                    Progress::Paused { done, total } => list
                        .SetProgressState(self.hwnd, TBPF_PAUSED)
                        .and_then(|_| list.SetProgressValue(self.hwnd, done, total.max(1))),
                };
            }
        }
    }

    unsafe extern "system" fn subclass(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        if Some(&msg) == TASKBAR_CREATED.get() {
            // Explorer (re)created the taskbar button: the buttons must be added again.
            TASKBAR.with(|t| {
                if let Some(t) = t.borrow_mut().as_mut() {
                    t.added = false;
                    t.refresh();
                }
            });
        } else if msg == WM_COMMAND && ((wparam.0 >> 16) & 0xFFFF) as u32 == THBN_CLICKED {
            let action = match (wparam.0 & 0xFFFF) as u32 {
                ID_PREVIOUS => Some("previous"),
                ID_TOGGLE => Some("toggle"),
                ID_NEXT => Some("next"),
                _ => None,
            };
            if let (Some(action), Some(app)) = (action, APP.get()) {
                emit_action(app, MediaAction::new(action));
                return LRESULT(0);
            }
        }
        // SAFETY: hands every other message to the window's own procedure.
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }

    fn smtc_for(hwnd: HWND) -> windows::core::Result<SystemMediaTransportControls> {
        let interop =
            factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
        // SAFETY: the main window's handle, on its thread.
        let smtc: SystemMediaTransportControls = unsafe { interop.GetForWindow(hwnd)? };
        smtc.SetIsEnabled(false)?;
        smtc.SetIsPlayEnabled(true)?;
        smtc.SetIsPauseEnabled(true)?;
        smtc.SetIsStopEnabled(true)?;
        smtc.SetIsNextEnabled(true)?;
        smtc.SetIsPreviousEnabled(true)?;
        smtc.ButtonPressed(&TypedEventHandler::<
            SystemMediaTransportControls,
            SystemMediaTransportControlsButtonPressedEventArgs,
        >::new(|_, args| {
            let button = args.ok()?.Button()?;
            let action = match button {
                SystemMediaTransportControlsButton::Play => "play",
                SystemMediaTransportControlsButton::Pause => "pause",
                SystemMediaTransportControlsButton::Stop => "stop",
                SystemMediaTransportControlsButton::Next => "next",
                SystemMediaTransportControlsButton::Previous => "previous",
                _ => return Ok(()),
            };
            if let Some(app) = APP.get() {
                emit_action(app, MediaAction::new(action));
            }
            Ok(())
        }))?;
        smtc.PlaybackPositionChangeRequested(&TypedEventHandler::<
            SystemMediaTransportControls,
            PlaybackPositionChangeRequestedEventArgs,
        >::new(|_, args| {
            let at = args.ok()?.RequestedPlaybackPosition()?;
            if let Some(app) = APP.get() {
                emit_action(
                    app,
                    MediaAction {
                        action: "seek",
                        sec: Some(at.Duration as f64 / 10_000_000.0),
                    },
                );
            }
            Ok(())
        }))?;
        Ok(smtc)
    }

    /// Called on the main thread once the window exists.
    pub fn start(app: &AppHandle) {
        let _ = APP.set(app.clone());
        let Some(window) = app.get_webview_window("main") else {
            return;
        };
        let Ok(raw) = window.hwnd() else {
            return;
        };
        let hwnd = HWND(raw.0);
        match smtc_for(hwnd) {
            Ok(smtc) => {
                let _ = SMTC.set(smtc);
            }
            Err(e) => tracing::warn!("Windows media controls not available: {e}"),
        }
        // SAFETY: plain Win32/COM calls on the window's thread.
        unsafe {
            let _ = TASKBAR_CREATED.set(RegisterWindowMessageW(windows::core::w!(
                "TaskbarButtonCreated"
            )));
            let list: Option<ITaskbarList3> =
                CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER).ok();
            if let Some(l) = list.as_ref() {
                let _ = l.HrInit();
            }
            let dpi = GetDpiForWindow(hwnd).max(96);
            let size = GetSystemMetricsForDpi(SM_CXSMICON, dpi).max(16);
            let rgb = if light_taskbar() {
                (32, 32, 32)
            } else {
                (240, 240, 240)
            };
            let icons = [Glyph::Previous, Glyph::Play, Glyph::Pause, Glyph::Next]
                .map(|g| make_icon(g, size, rgb));
            TASKBAR.with(|t| {
                *t.borrow_mut() = Some(Taskbar {
                    hwnd,
                    list,
                    icons,
                    added: false,
                    shown: false,
                    playing: false,
                });
            });
            if !SetWindowSubclass(hwnd, Some(subclass), 0x00B0_0B1E, 0).as_bool() {
                tracing::warn!("taskbar buttons: window subclass failed");
            }
        }
    }

    fn show(
        smtc: &SystemMediaTransportControls,
        np: &super::NowPlaying,
    ) -> windows::core::Result<()> {
        smtc.SetIsEnabled(true)?;
        smtc.SetIsNextEnabled(true)?;
        smtc.SetIsPreviousEnabled(true)?;
        let updater = smtc.DisplayUpdater()?;
        updater.ClearAll()?;
        updater.SetType(MediaPlaybackType::Music)?;
        let music = updater.MusicProperties()?;
        music.SetTitle(&HSTRING::from(np.title.as_str()))?;
        if let Some(a) = np.artist.as_deref() {
            music.SetArtist(&HSTRING::from(a))?;
        }
        if let Some(a) = np.album.as_deref() {
            music.SetAlbumTitle(&HSTRING::from(a))?;
        }
        if let Some(a) = np.album_artist.as_deref() {
            music.SetAlbumArtist(&HSTRING::from(a))?;
        }
        if let Some(url) = np.art_url.as_deref() {
            let uri = Uri::CreateUri(&HSTRING::from(url))?;
            updater.SetThumbnail(&RandomAccessStreamReference::CreateFromUri(&uri)?)?;
        }
        updater.Update()
    }

    fn timeline(
        smtc: &SystemMediaTransportControls,
        position: f64,
        duration: f64,
    ) -> windows::core::Result<()> {
        let t = SystemMediaTransportControlsTimelineProperties::new()?;
        t.SetStartTime(ticks(0.0))?;
        t.SetMinSeekTime(ticks(0.0))?;
        t.SetEndTime(ticks(duration))?;
        t.SetMaxSeekTime(ticks(duration))?;
        t.SetPosition(ticks(position))?;
        smtc.UpdateTimelineProperties(&t)
    }

    /// Applies updates on the window's thread, without waiting: the controls were made there, and a call from
    /// another thread would be marshalled to it and could hold up the engine's event thread.
    pub fn apply(app: &AppHandle, updates: Vec<Update>) {
        let _ = app.run_on_main_thread(move || apply_here(updates));
    }

    fn apply_here(updates: Vec<Update>) {
        if let Some(smtc) = SMTC.get() {
            for u in &updates {
                let done = match u {
                    Update::Show(np) => show(smtc, np),
                    Update::Clear => smtc
                        .DisplayUpdater()
                        .and_then(|d| d.ClearAll())
                        .and_then(|_| smtc.SetIsEnabled(false)),
                    Update::Status(s) => smtc.SetPlaybackStatus(match s {
                        PlayState::Playing => MediaPlaybackStatus::Playing,
                        PlayState::Paused => MediaPlaybackStatus::Paused,
                        PlayState::Stopped => MediaPlaybackStatus::Stopped,
                    }),
                    Update::Timeline { position, duration } => timeline(smtc, *position, *duration),
                    Update::Progress(_) => Ok(()),
                };
                if let Err(e) = done {
                    tracing::debug!("media controls update failed: {e}");
                }
            }
        }
        let taskbar: Vec<Update> = updates
            .into_iter()
            .filter(|u| {
                matches!(
                    u,
                    Update::Show(_) | Update::Clear | Update::Status(_) | Update::Progress(_)
                )
            })
            .collect();
        if taskbar.is_empty() {
            return;
        }
        {
            TASKBAR.with(|t| {
                let mut t = t.borrow_mut();
                let Some(t) = t.as_mut() else {
                    return;
                };
                let mut refresh = false;
                for u in taskbar {
                    match u {
                        Update::Show(_) => {
                            refresh |= !t.shown;
                            t.shown = true;
                        }
                        Update::Clear => {
                            refresh |= t.shown;
                            t.shown = false;
                            t.playing = false;
                        }
                        Update::Status(s) => {
                            let playing = s == PlayState::Playing;
                            refresh |= playing != t.playing;
                            t.playing = playing;
                        }
                        Update::Progress(p) => t.progress(p),
                        Update::Timeline { .. } => {}
                    }
                }
                if refresh {
                    t.refresh();
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(live: bool) -> NowPlaying {
        NowPlaying {
            title: "  Callas Went Away ".into(),
            artist: Some("Enigma".into()),
            album: Some(String::new()),
            album_artist: None,
            art_url: Some("http://srv:3001/api/albums/1/art".into()),
            duration_sec: Some(269.0),
            live,
        }
    }

    fn state(status: Status, pos: f64) -> Event {
        Event::State {
            status,
            track_id: Some("t#1".into()),
            position_sec: pos,
            duration_sec: Some(269.0),
        }
    }

    #[test]
    fn a_song_shows_with_its_timeline_and_progress() {
        let mut t = Tracker::default();
        assert!(
            t.on_event(&state(Status::Playing, 1.0), Instant::now())
                .is_empty(),
            "nothing shown yet"
        );
        let out = t.set_now(Some(song(false)));
        let Update::Show(np) = &out[0] else {
            panic!("{out:?}")
        };
        assert_eq!(np.title, "Callas Went Away");
        assert_eq!(np.album, None, "empty texts are dropped");
        assert!(out.contains(&Update::Timeline {
            position: 0.0,
            duration: 269.0
        }));

        let at = Instant::now();
        let out = t.on_event(&state(Status::Playing, 10.0), at);
        assert_eq!(out[0], Update::Status(PlayState::Playing));
        assert!(out.contains(&Update::Progress(Progress::Playing {
            done: 10_000,
            total: 269_000
        })));
        // Within a second: nothing new; after 1 s the progress, after 5 s the timeline too.
        assert!(t
            .on_event(
                &state(Status::Playing, 10.25),
                at + Duration::from_millis(250)
            )
            .is_empty());
        let out = t.on_event(
            &state(Status::Playing, 11.5),
            at + Duration::from_millis(1_500),
        );
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], Update::Progress(Progress::Playing { .. })));
        let out = t.on_event(&state(Status::Playing, 16.0), at + Duration::from_secs(6));
        assert!(out.iter().any(|u| matches!(u, Update::Timeline { .. })));

        let out = t.on_event(&state(Status::Paused, 16.0), at + Duration::from_secs(7));
        assert_eq!(out[0], Update::Status(PlayState::Paused));
        assert!(out.contains(&Update::Progress(Progress::Paused {
            done: 16_000,
            total: 269_000
        })));

        assert_eq!(
            t.set_now(None),
            [Update::Clear, Update::Progress(Progress::Off)]
        );
        assert!(t.set_now(None).is_empty());
    }

    #[test]
    fn radio_has_no_timeline_or_progress() {
        let mut t = Tracker::default();
        let mut np = song(true);
        np.duration_sec = None;
        let out = t.set_now(Some(np));
        assert!(out.iter().all(|u| !matches!(u, Update::Timeline { .. })));
        let out = t.on_event(&state(Status::Playing, 30.0), Instant::now());
        assert_eq!(
            out,
            [
                Update::Status(PlayState::Playing),
                Update::Progress(Progress::Off)
            ]
        );
    }

    #[test]
    fn only_plain_web_art_urls_and_short_texts_pass() {
        let mut np = song(false);
        np.art_url = Some("file:///c:/x.jpg".into());
        np.title = "x".repeat(1_000);
        let np = np.cleaned();
        assert_eq!(np.art_url, None);
        assert_eq!(np.title.chars().count(), MAX_TEXT);
        let parsed: NowPlaying = serde_json::from_str(
            r#"{"title":"T","artist":"A","artUrl":"https://h/a.jpg","durationSec":12.5}"#,
        )
        .unwrap();
        assert_eq!(parsed.duration_sec, Some(12.5));
        assert!(!parsed.live);
        assert_eq!(
            serde_json::to_string(&MediaAction {
                action: "seek",
                sec: Some(3.0)
            })
            .unwrap(),
            r#"{"action":"seek","sec":3.0}"#
        );
        assert_eq!(
            serde_json::to_string(&MediaAction::new("next")).unwrap(),
            r#"{"action":"next"}"#
        );
    }

    #[test]
    fn glyphs_are_drawn_where_expected() {
        use glyphs::{render, Glyph};
        let px = render(Glyph::Play, 16, (255, 255, 255));
        assert_eq!(px.len(), 16 * 16 * 4);
        let alpha = |x: usize, y: usize| px[(y * 16 + x) * 4 + 3];
        assert_eq!(alpha(8, 8), 255, "the middle of the triangle");
        assert_eq!(alpha(1, 1), 0, "the corner is clear");
        let pause = render(Glyph::Pause, 16, (0, 0, 0));
        assert_eq!(pause[(8 * 16 + 8) * 4 + 3], 0, "the gap between the bars");
        assert_eq!(pause[(8 * 16 + 5) * 4 + 3], 255, "a bar");
        for g in [Glyph::Previous, Glyph::Next] {
            assert!(render(g, 24, (1, 2, 3)).chunks(4).any(|p| p[3] == 255));
        }
    }
}
