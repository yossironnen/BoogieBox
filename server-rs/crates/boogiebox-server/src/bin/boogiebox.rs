//! Windows tray launcher (`boogiebox.exe`): starts `boogiebox-server.exe` from its own folder
//! without a console window and shows a notification-area icon whose menu opens BoogieBox in the
//! default browser or exits. `boogiebox.exe --exit` stops a running launcher (used by the installer).

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    std::process::exit(tray::run());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("boogiebox is the Windows tray launcher; run boogiebox-server instead.");
    std::process::exit(1);
}

#[cfg(windows)]
mod tray {
    use boogiebox_server::{shutdown_event::SHUTDOWN_EVENT_ENV, DEFAULT_PORT};
    use std::{
        cell::RefCell,
        collections::VecDeque,
        ffi::c_void,
        io::{BufRead, BufReader},
        mem::{size_of, zeroed},
        net::{SocketAddr, TcpStream},
        os::windows::{io::AsRawHandle, process::CommandExt},
        process::{Child, Command, Stdio},
        ptr::{null, null_mut},
        sync::{
            atomic::{AtomicI32, AtomicU32, Ordering},
            Arc, Mutex,
        },
        thread,
        time::Duration,
    };
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
        },
        System::{
            Com::{CoInitializeEx, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE},
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            LibraryLoader::GetModuleHandleW,
            Threading::{
                CreateEventW, CreateMutexW, OpenProcess, SetEvent, WaitForSingleObject,
                CREATE_NO_WINDOW, INFINITE, PROCESS_SYNCHRONIZE,
            },
        },
        UI::{
            HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
            Shell::{
                ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP,
                NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_SELECT, NOTIFYICONDATAW,
                NOTIFYICON_VERSION_4,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
                DestroyWindow, DispatchMessageW, FindWindowW, GetMessageW, GetSystemMetrics,
                GetWindowThreadProcessId, KillTimer, LoadIconW, LoadImageW, MessageBoxW,
                PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
                SetForegroundWindow, SetMenuDefaultItem, SetTimer, TrackPopupMenu,
                TranslateMessage, HICON, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTCOLOR,
                MB_ICONERROR, MB_OK, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, SM_CXSMICON,
                SM_CYSMICON, SW_SHOWNORMAL, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP,
                WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_ENDSESSION, WM_NULL, WM_TIMER, WNDCLASSW,
                WS_OVERLAPPED,
            },
        },
    };

    const SERVER_EXE: &str = "boogiebox-server.exe";
    /// Window class of the hidden window that owns the tray icon; `--exit` finds it by this name.
    const WINDOW_CLASS: &str = "BoogieBoxTrayWindow";
    const MUTEX_NAME: &str = "Local\\BoogieBoxTray";
    const TOOLTIP: &str = "BoogieBox Server";
    const CRASH_MESSAGE: &str = "BoogieBox Server stopped unexpectedly.";

    const WM_TRAY: u32 = WM_APP + 1;
    const WM_SERVER_READY: u32 = WM_APP + 2;
    const WM_SERVER_EXITED: u32 = WM_APP + 3;
    /// `NIN_SELECT | NINF_KEY`: the icon was chosen with the keyboard (missing from windows-sys).
    const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
    const MENU_OPEN: usize = 1;
    const MENU_EXIT: usize = 2;
    const STOP_TIMER: usize = 1;
    const ICON_RETRY_TIMER: usize = 2;

    /// How long Exit waits for a graceful stop before ending the server process.
    const STOP_GRACE: Duration = Duration::from_secs(20);
    /// How long a Windows sign-out or shutdown waits for the server.
    const SESSION_END_GRACE: Duration = Duration::from_secs(5);
    const ICON_RETRY: Duration = Duration::from_secs(2);
    const READY_POLL: Duration = Duration::from_millis(500);
    const STDERR_TAIL_LINES: usize = 8;

    static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
    static EXIT_CODE: AtomicI32 = AtomicI32::new(0);

    thread_local! {
        static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ServerState {
        Starting,
        Running,
        Stopping,
    }

    struct Tray {
        hwnd: HWND,
        icon: HICON,
        server: Child,
        shutdown_event: HANDLE,
        stderr_tail: Arc<Mutex<VecDeque<String>>>,
        url: String,
        state: ServerState,
    }

    pub fn run() -> i32 {
        if std::env::args().skip(1).any(|arg| arg == "--exit") {
            stop_running_launcher();
            return 0;
        }
        unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            CoInitializeEx(
                null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            );
        }
        let port = server_port();
        // Already running here, or the port is served by the Windows service or a console run.
        if !acquire_single_instance() || server_listening(port) {
            return 0;
        }
        let hwnd = create_window();
        if hwnd.is_null() {
            return 1;
        }
        let event_name = format!("Local\\BoogieBox-Shutdown-{}", std::process::id());
        let shutdown_event = unsafe { CreateEventW(null(), 1, 0, wide(&event_name).as_ptr()) };
        let mut server = match spawn_server(&event_name) {
            Ok(server) => server,
            Err(error) => {
                show_crash(&error);
                unsafe { DestroyWindow(hwnd) };
                return 1;
            }
        };
        keep_with_launcher(&server);
        let stderr_tail = collect_stderr(&mut server);
        watch_exit(hwnd, &server);
        watch_ready(hwnd, port);

        let tray = Tray {
            hwnd,
            icon: load_icon(),
            server,
            shutdown_event,
            stderr_tail,
            url: format!("http://localhost:{port}/"),
            state: ServerState::Starting,
        };
        tray.add_icon();
        TRAY.with(|cell| *cell.borrow_mut() = Some(tray));

        unsafe {
            let mut message: MSG = zeroed();
            while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        EXIT_CODE.load(Ordering::Relaxed)
    }

    impl Tray {
        fn notify_data(&self) -> NOTIFYICONDATAW {
            let mut data: NOTIFYICONDATAW = unsafe { zeroed() };
            data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            data.hWnd = self.hwnd;
            data.uID = 1;
            data
        }

        /// Adds the icon; retries while Explorer isn't ready yet (early in a sign-in).
        fn add_icon(&self) {
            let mut data = self.notify_data();
            data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
            data.uCallbackMessage = WM_TRAY;
            data.hIcon = self.icon;
            copy_into(&mut data.szTip, TOOLTIP);
            unsafe {
                if Shell_NotifyIconW(NIM_ADD, &data) == 0
                    && Shell_NotifyIconW(NIM_MODIFY, &data) == 0
                {
                    SetTimer(
                        self.hwnd,
                        ICON_RETRY_TIMER,
                        ICON_RETRY.as_millis() as u32,
                        None,
                    );
                    return;
                }
                data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                Shell_NotifyIconW(NIM_SETVERSION, &data);
            }
        }

        fn remove_icon(&self) {
            unsafe {
                KillTimer(self.hwnd, ICON_RETRY_TIMER);
                Shell_NotifyIconW(NIM_DELETE, &self.notify_data());
            }
        }

        /// Asks the server to shut down; it is ended after [`STOP_GRACE`] if it hasn't.
        fn begin_stop(&mut self) {
            if self.state == ServerState::Stopping {
                return;
            }
            self.state = ServerState::Stopping;
            unsafe {
                if self.shutdown_event.is_null() || SetEvent(self.shutdown_event) == 0 {
                    let _ = self.server.kill();
                    return;
                }
                SetTimer(self.hwnd, STOP_TIMER, STOP_GRACE.as_millis() as u32, None);
            }
        }

        fn stop_for_session_end(&mut self) {
            self.begin_stop();
            let process = self.server.as_raw_handle() as HANDLE;
            unsafe { WaitForSingleObject(process, SESSION_END_GRACE.as_millis() as u32) };
            let _ = self.server.kill();
            self.remove_icon();
        }
    }

    fn with_tray<R>(f: impl FnOnce(&mut Tray) -> R) -> Option<R> {
        TRAY.with(|cell| {
            let mut tray = cell.try_borrow_mut().ok()?;
            tray.as_mut().map(f)
        })
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_TRAY => {
                // NOTIFYICON_VERSION_4: the event is in the low word of lParam, the anchor point in wParam.
                let event = (lparam & 0xFFFF) as u32;
                if matches!(event, NIN_SELECT | NIN_KEYSELECT | WM_CONTEXTMENU) {
                    let x = (wparam & 0xFFFF) as u16 as i16 as i32;
                    let y = ((wparam >> 16) & 0xFFFF) as u16 as i16 as i32;
                    show_menu(hwnd, x, y);
                }
                0
            }
            WM_SERVER_READY => {
                with_tray(|tray| {
                    if tray.state == ServerState::Starting {
                        tray.state = ServerState::Running;
                    }
                });
                0
            }
            WM_SERVER_EXITED => {
                server_exited(hwnd);
                0
            }
            WM_TIMER if wparam == STOP_TIMER => {
                KillTimer(hwnd, STOP_TIMER);
                with_tray(|tray| tray.server.kill());
                0
            }
            WM_TIMER if wparam == ICON_RETRY_TIMER => {
                KillTimer(hwnd, ICON_RETRY_TIMER);
                with_tray(|tray| tray.add_icon());
                0
            }
            WM_CLOSE => {
                with_tray(Tray::begin_stop);
                0
            }
            WM_ENDSESSION => {
                if wparam != 0 {
                    with_tray(Tray::stop_for_session_end);
                }
                0
            }
            WM_DESTROY => {
                PostQuitMessage(EXIT_CODE.load(Ordering::Relaxed));
                0
            }
            // Explorer restarted: its notification area starts empty.
            _ if message != 0 && message == TASKBAR_CREATED.load(Ordering::Relaxed) => {
                with_tray(|tray| tray.add_icon());
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    /// Shows the menu. No `TRAY` borrow is held while it is open: its modal loop keeps
    /// dispatching messages to `window_proc`.
    fn show_menu(hwnd: HWND, x: i32, y: i32) {
        let Some(state) = with_tray(|tray| tray.state) else {
            return;
        };
        let status = wide(match state {
            ServerState::Starting => "BoogieBox is starting",
            ServerState::Running => "BoogieBox is running",
            ServerState::Stopping => "BoogieBox is stopping",
        });
        let open = wide("Open BoogieBox");
        let exit = wide("Exit");
        let enabled = if state == ServerState::Stopping {
            MF_GRAYED
        } else {
            0
        };
        let command = unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return;
            }
            AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, status.as_ptr());
            AppendMenuW(menu, MF_STRING | enabled, MENU_OPEN, open.as_ptr());
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            AppendMenuW(menu, MF_STRING | enabled, MENU_EXIT, exit.as_ptr());
            SetMenuDefaultItem(menu, MENU_OPEN as u32, 0);
            // Without this the menu stays open when the user clicks elsewhere.
            SetForegroundWindow(hwnd);
            let command = TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
                x,
                y,
                0,
                hwnd,
                null(),
            );
            PostMessageW(hwnd, WM_NULL, 0, 0);
            DestroyMenu(menu);
            command as usize
        };
        match command {
            MENU_OPEN => {
                with_tray(|tray| open_browser(&tray.url));
            }
            MENU_EXIT => {
                with_tray(Tray::begin_stop);
            }
            _ => {}
        }
    }

    fn server_exited(hwnd: HWND) {
        let Some(mut tray) =
            TRAY.with(|cell| cell.try_borrow_mut().ok().and_then(|mut tray| tray.take()))
        else {
            return;
        };
        let _ = tray.server.wait();
        tray.remove_icon();
        unsafe {
            KillTimer(hwnd, STOP_TIMER);
            if !tray.shutdown_event.is_null() {
                CloseHandle(tray.shutdown_event);
            }
        }
        if tray.state != ServerState::Stopping {
            EXIT_CODE.store(1, Ordering::Relaxed);
            let tail = tray.stderr_tail.lock().unwrap_or_else(|p| p.into_inner());
            show_crash(&Vec::from(tail.clone()).join("\n"));
        }
        unsafe { DestroyWindow(hwnd) };
    }

    fn spawn_server(event_name: &str) -> Result<Child, String> {
        let launcher = std::env::current_exe().map_err(|error| error.to_string())?;
        let folder = launcher
            .parent()
            .ok_or_else(|| format!("{SERVER_EXE}: no install folder"))?;
        // The launcher's own folder plus a constant file name; no user input reaches the path.
        // nosemgrep: boogiebox-rust-dynamic-command
        Command::new(folder.join(SERVER_EXE))
            .current_dir(folder)
            .env(SHUTDOWN_EVENT_ENV, event_name)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| format!("{SERVER_EXE}: {error}"))
    }

    /// Puts the server in a job that ends it when the launcher ends, so a killed launcher
    /// never leaves a windowless server running. The job handle stays open until exit.
    fn keep_with_launcher(server: &Child) {
        unsafe {
            let job = CreateJobObjectW(null(), null());
            if job.is_null() {
                return;
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let limited = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const c_void,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) != 0;
            if !limited || AssignProcessToJobObject(job, server.as_raw_handle() as HANDLE) == 0 {
                CloseHandle(job);
            }
        }
    }

    /// Keeps the last lines the server wrote to stderr (its fatal startup error) for the crash
    /// message. Reads until the pipe closes so the server never blocks on a full pipe.
    fn collect_stderr(server: &mut Child) -> Arc<Mutex<VecDeque<String>>> {
        let tail = Arc::new(Mutex::new(VecDeque::new()));
        if let Some(stderr) = server.stderr.take() {
            let tail = Arc::clone(&tail);
            thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut line = Vec::new();
                while matches!(reader.read_until(b'\n', &mut line), Ok(read) if read > 0) {
                    let text = String::from_utf8_lossy(&line).trim_end().to_string();
                    line.clear();
                    if text.is_empty() {
                        continue;
                    }
                    let mut tail = tail.lock().unwrap_or_else(|p| p.into_inner());
                    if tail.len() == STDERR_TAIL_LINES {
                        tail.pop_front();
                    }
                    tail.push_back(text);
                }
            });
        }
        tail
    }

    /// Posts `WM_SERVER_EXITED` when the server process ends. `Tray` keeps the `Child`, and so
    /// this handle, open until that message is handled.
    fn watch_exit(hwnd: HWND, server: &Child) {
        let (hwnd, process) = (hwnd as usize, server.as_raw_handle() as usize);
        thread::spawn(move || unsafe {
            WaitForSingleObject(process as HANDLE, INFINITE);
            PostMessageW(hwnd as HWND, WM_SERVER_EXITED, 0, 0);
        });
    }

    /// Posts `WM_SERVER_READY` once the server accepts connections.
    fn watch_ready(hwnd: HWND, port: u16) {
        let hwnd = hwnd as usize;
        thread::spawn(move || {
            while !server_listening(port) {
                thread::sleep(READY_POLL);
            }
            unsafe { PostMessageW(hwnd as HWND, WM_SERVER_READY, 0, 0) };
        });
    }

    fn server_port() -> u16 {
        std::env::var("PORT")
            .ok()
            .and_then(|raw| raw.trim().parse().ok())
            .unwrap_or(DEFAULT_PORT)
    }

    fn server_listening(port: u16) -> bool {
        TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), READY_POLL).is_ok()
    }

    fn acquire_single_instance() -> bool {
        // The mutex stays open for the life of the process.
        let mutex = unsafe { CreateMutexW(null(), 0, wide(MUTEX_NAME).as_ptr()) };
        !mutex.is_null() && unsafe { GetLastError() } != ERROR_ALREADY_EXISTS
    }

    fn create_window() -> HWND {
        let class_name = wide(WINDOW_CLASS);
        unsafe {
            let instance = GetModuleHandleW(null());
            let mut class: WNDCLASSW = zeroed();
            class.lpfnWndProc = Some(window_proc);
            class.hInstance = instance;
            class.lpszClassName = class_name.as_ptr();
            RegisterClassW(&class);
            TASKBAR_CREATED.store(
                RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
                Ordering::Relaxed,
            );
            // A hidden top-level window rather than a message-only one: only top-level windows
            // receive the TaskbarCreated and WM_ENDSESSION broadcasts.
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                wide("BoogieBox").as_ptr(),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                instance,
                null(),
            )
        }
    }

    /// The icon embedded by build.rs (resource id 1), at the small-icon size for this DPI.
    fn load_icon() -> HICON {
        unsafe {
            let icon = LoadImageW(
                GetModuleHandleW(null()),
                // MAKEINTRESOURCEW(1)
                std::ptr::without_provenance(1),
                IMAGE_ICON,
                GetSystemMetrics(SM_CXSMICON),
                GetSystemMetrics(SM_CYSMICON),
                LR_DEFAULTCOLOR,
            );
            if icon.is_null() {
                LoadIconW(null_mut(), IDI_APPLICATION)
            } else {
                icon
            }
        }
    }

    fn open_browser(url: &str) {
        unsafe {
            ShellExecuteW(
                null_mut(),
                wide("open").as_ptr(),
                wide(url).as_ptr(),
                null(),
                null(),
                SW_SHOWNORMAL,
            );
        }
    }

    fn show_crash(detail: &str) {
        let text = if detail.is_empty() {
            CRASH_MESSAGE.to_string()
        } else {
            format!("{CRASH_MESSAGE}\n\n{detail}")
        };
        unsafe {
            MessageBoxW(
                null_mut(),
                wide(&text).as_ptr(),
                wide("BoogieBox").as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }

    /// `boogiebox.exe --exit`: closes the running launcher in this session and waits for it.
    fn stop_running_launcher() {
        unsafe {
            let hwnd = FindWindowW(wide(WINDOW_CLASS).as_ptr(), null());
            if hwnd.is_null() {
                return;
            }
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
            if !process.is_null() {
                let wait = STOP_GRACE + Duration::from_secs(10);
                WaitForSingleObject(process, wait.as_millis() as u32);
                CloseHandle(process);
            }
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn copy_into(buffer: &mut [u16], text: &str) {
        let limit = buffer.len().saturating_sub(1);
        for (slot, unit) in buffer.iter_mut().zip(text.encode_utf16().take(limit)) {
            *slot = unit;
        }
    }
}
