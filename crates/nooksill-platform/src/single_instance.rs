//! Single instance lock and lookup of the already running window.
//! A named mutex plus a message makes a second launch raise the existing dock.
//! The `Local\` prefix is load-bearing: `Global\` would block other users.

use std::fmt;

use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, WPARAM,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use nooksill_core::SINGLETON_MUTEX;

use crate::win::to_wide;

/// A second launch asks the running instance to surface with this message.
///
/// It must not collide with `worker::WM_APP_WAKE` (`WM_APP+1`) or
/// `tray::WM_APP_TRAY` (`WM_APP+2`); both land on the same window.
pub const WM_APP_SHOW: u32 = WM_APP + 3;

/// Held single-instance lock; the handle is closed on drop.
#[derive(Debug)]
pub struct SingleInstance {
    handle: HANDLE,
}

impl SingleInstance {
    /// Try to take the lock.
    ///
    /// `Ok(Some(_))` means this process is first; `Ok(None)` means another
    /// instance is running and the caller should raise its window and exit;
    /// `Err` means the mutex could not be created at all.
    pub fn acquire() -> Result<Option<Self>, MutexError> {
        // The name comes from `nooksill_core::SINGLETON_MUTEX` so both builds
        // contend on the same mutex and never run two docks at once.
        let name = to_wide(SINGLETON_MUTEX);
        let handle = unsafe { CreateMutexW(None, true, windows::core::PCWSTR(name.as_ptr())) }
            .map_err(|e| MutexError { code: e.code().0 })?;
        // CreateMutexW returns a valid handle even when the object already
        // exists; the real answer is in GetLastError.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
            };
            return Ok(None);
        }
        Ok(Some(Self { handle }))
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        };
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MutexError {
    pub code: i32,
}

impl fmt::Display for MutexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&nooksill_core::i18n::fmt(
            "CreateMutexW failed with code {0}",
            &[format!("0x{:08X}", self.code).as_str()],
        ))
    }
}

impl std::error::Error for MutexError {}

/// Find an existing instance by window class name.
#[must_use]
pub fn find_window(class: &str) -> Option<HWND> {
    let wide = crate::win::to_wide(class);
    let hwnd = unsafe {
        windows::Win32::UI::WindowsAndMessaging::FindWindowW(
            windows::core::PCWSTR(wide.as_ptr()),
            None,
        )
    };
    hwnd.ok().filter(|h| !h.0.is_null())
}

/// Ask the running instance to surface its window.
///
/// Returns false when no window was found or the post was refused (for
/// example when the running dock is elevated and this process is not, which
/// UIPI blocks). The caller still exits: a second Dock is worse than a summon
/// that did not land.
pub fn summon(class: &str) -> bool {
    let Some(hwnd) = find_window(class) else {
        return false;
    };
    // SAFETY: the handle comes from `FindWindowW`; a dead window makes the
    // post fail instead of crashing.
    unsafe { PostMessageW(Some(hwnd), WM_APP_SHOW, WPARAM(0), LPARAM(0)).is_ok() }
}
