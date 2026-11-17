//! Ownership guard for a window created before its Rust object exists.
//!
//! Every panel creates the HWND first, then a `FrameRenderer`; a failure in the
//! second step used to leak a hidden window because nothing owned the handle
//! yet. The lease destroys it on that path and is defused once the pinned
//! object takes over.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;

/// Temporary owner of a fresh HWND.
#[derive(Debug)]
pub struct WindowLease(Option<HWND>);

impl WindowLease {
    #[must_use]
    pub fn new(hwnd: HWND) -> Self {
        Self(Some(hwnd))
    }

    /// Transfer ownership; the guard no longer destroys the window.
    #[must_use]
    pub fn defuse(mut self) -> HWND {
        self.0.take().expect("window lease defused twice")
    }
}

impl Drop for WindowLease {
    fn drop(&mut self) {
        if let Some(hwnd) = self.0.take() {
            // SAFETY: the lease owns this handle until `defuse`; the window
            // has no other owner on this path.
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defuse_returns_the_handle_and_stops_owning_it() {
        let lease = WindowLease::new(HWND(std::ptr::null_mut()));
        assert!(lease.defuse().0.is_null());
    }

    #[test]
    fn dropping_without_defuse_is_safe_on_a_null_handle() {
        drop(WindowLease::new(HWND(std::ptr::null_mut())));
    }
}
