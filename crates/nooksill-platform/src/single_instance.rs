//! Single instance lock and lookup of the already running window.
//! A named mutex plus a message makes a second launch raise the existing dock.
//! The `Local\` prefix is load-bearing: `Global\` would block other users.

use std::fmt;

use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND};
use windows::Win32::System::Threading::CreateMutexW;

use nooksill_core::SINGLETON_MUTEX;

use crate::win::to_wide;

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
        write!(f, "CreateMutexW 失败: 0x{:08X}", self.code)
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
