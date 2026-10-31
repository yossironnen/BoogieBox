//! Lets the Windows tray launcher (`boogiebox.exe`) stop a server it started without a console:
//! it creates a named event, passes the name in [`SHUTDOWN_EVENT_ENV`] and sets it on Exit. The
//! server then shuts down the same way it does on Ctrl+C. Never fires elsewhere or when unset.

use std::ffi::OsString;

/// Environment variable naming the Windows event that asks the server to shut down.
pub const SHUTDOWN_EVENT_ENV: &str = "BOOGIEBOX_SHUTDOWN_EVENT";

/// Resolves once the event named in [`SHUTDOWN_EVENT_ENV`] is set; pends forever without one.
pub async fn requested() {
    wait_named(std::env::var_os(SHUTDOWN_EVENT_ENV)).await
}

async fn wait_named(name: Option<OsString>) {
    #[cfg(windows)]
    if let Some(name) = name.filter(|name| !name.is_empty()) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        // A detached thread rather than spawn_blocking: the runtime waits for blocking tasks
        // when it drops, and this wait never ends when Ctrl+C stops the server instead.
        std::thread::spawn(move || {
            if wait_for_event(&name) {
                let _ = tx.send(());
            }
        });
        if rx.await.is_ok() {
            return;
        }
    }
    #[cfg(not(windows))]
    let _ = name;
    std::future::pending::<()>().await
}

#[cfg(windows)]
fn wait_for_event(name: &std::ffi::OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        OpenEventW, WaitForSingleObject, INFINITE, SYNCHRONIZATION_SYNCHRONIZE,
    };

    let wide: Vec<u16> = name.encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        let handle = OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, 0, wide.as_ptr());
        if handle.is_null() {
            tracing::warn!(
                "Shutdown event {} could not be opened; only Ctrl+C stops this server",
                name.to_string_lossy()
            );
            return false;
        }
        let signalled = WaitForSingleObject(handle, INFINITE) == WAIT_OBJECT_0;
        CloseHandle(handle);
        signalled
    }
}

#[cfg(test)]
mod tests {
    use super::wait_named;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn without_an_event_name_it_never_fires() {
        assert!(timeout(Duration::from_millis(50), wait_named(None))
            .await
            .is_err());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn an_event_that_does_not_exist_never_fires() {
        let name = format!("Local\\BoogieBox-Test-Missing-{}", uuid::Uuid::now_v7());
        assert!(
            timeout(Duration::from_millis(50), wait_named(Some(name.into())))
                .await
                .is_err()
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn setting_the_event_fires() {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent};

        let name = format!("Local\\BoogieBox-Test-Shutdown-{}", uuid::Uuid::now_v7());
        let wide: Vec<u16> = std::ffi::OsStr::new(&name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, wide.as_ptr()) };
        assert!(!event.is_null());
        let waiter = tokio::spawn(wait_named(Some(name.into())));
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!waiter.is_finished());
        unsafe { SetEvent(event) };
        assert!(timeout(Duration::from_secs(5), waiter).await.is_ok());
        unsafe { CloseHandle(event) };
    }
}
