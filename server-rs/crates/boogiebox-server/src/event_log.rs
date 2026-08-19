//! Writes the server startup banner to the Windows "Application" Event Log,
//! independent of the file/console logging pipeline (`logging.rs`) — so the
//! one-time startup line is visible in Event Viewer even if file logging is
//! misconfigured, the log directory isn't writable, or `BOOGIEBOX_LOG_LEVEL`
//! filters it out. No-op on non-Windows targets.
//!
//! No message-file DLL is registered for the "BoogieBox" event source, so
//! Event Viewer shows the raw text via its generic "description for Event ID
//! X from source BoogieBox cannot be found" fallback rather than formatted
//! text — the message itself is still fully visible in that fallback view.

#[cfg(windows)]
pub fn write_startup_event(message: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::EventLog::{
        DeregisterEventSource, RegisterEventSourceW, ReportEventW, EVENTLOG_INFORMATION_TYPE,
    };

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    unsafe {
        let source_name = wide("BoogieBox");
        let handle = RegisterEventSourceW(std::ptr::null(), source_name.as_ptr());
        if handle.is_null() {
            return;
        }
        let text = wide(message);
        let strings: [*const u16; 1] = [text.as_ptr()];
        ReportEventW(
            handle,
            EVENTLOG_INFORMATION_TYPE,
            0,    // category
            1000, // event id (no registered message-file DLL — see module docs)
            std::ptr::null_mut(),
            1,
            0,
            strings.as_ptr(),
            std::ptr::null(),
        );
        DeregisterEventSource(handle);
    }
}

#[cfg(not(windows))]
pub fn write_startup_event(_message: &str) {}
