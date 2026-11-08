//! Click-outside dismissal for popup panels.
//!
//! The dock is `WS_EX_NOACTIVATE` and never receives capture (Windows only gives
//! it to the foreground window), so the main loop polls the global button state.
//! The click still reaches its target and the panel closes alongside it.

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetWindowRect};

/// Poll interval. 8 ms bounds the dismissal latency while a panel is open.
pub const POLL_MS: u32 = 8;

/// Button watcher that separates a press edge from a held button.
#[derive(Debug, Default)]
pub struct Watcher {
    left_down: bool,
    right_down: bool,
}

impl Watcher {
    /// Drop the edges accumulated so far, recording only the held state.
    ///
    /// Must run when a panel opens: the click that opened it still sits in
    /// `GetAsyncKeyState`'s low bit, and the next poll would read it as a fresh
    /// press and close the panel immediately.
    pub fn reset(&mut self) {
        // SAFETY: Reads global state only.
        let left = unsafe { GetAsyncKeyState(i32::from(VK_LBUTTON.0)) };
        let right = unsafe { GetAsyncKeyState(i32::from(VK_RBUTTON.0)) };
        self.left_down = left < 0;
        self.right_down = right < 0;
    }

    /// Whether a button was pressed since the last call, plus the cursor position.
    ///
    /// Both signals are needed: the low bit means "pressed since the last call"
    /// but another thread calling the API can consume it, while the held state
    /// alone misses fast clicks between polls.
    pub fn poll(&mut self) -> Option<(i32, i32)> {
        // SAFETY: `GetAsyncKeyState` reads global state and has no preconditions.
        let left = unsafe { GetAsyncKeyState(i32::from(VK_LBUTTON.0)) };
        let right = unsafe { GetAsyncKeyState(i32::from(VK_RBUTTON.0)) };
        let left_hit = (left & 1) != 0 || (left < 0 && !self.left_down);
        let right_hit = (right & 1) != 0 || (right < 0 && !self.right_down);
        self.left_down = left < 0;
        self.right_down = right < 0;
        if left_hit || right_hit {
            Some(cursor())
        } else {
            None
        }
    }
}

/// Cursor position in screen coordinates, physical pixels.
fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    // SAFETY: The output struct lives on this frame.
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

/// Window screen rect `(left, top, right, bottom)` in physical pixels.
#[must_use]
pub fn screen_rect(hwnd: HWND) -> Option<(i32, i32, i32, i32)> {
    let mut rc = RECT::default();
    // SAFETY: `rc` is on this frame; the caller guarantees a valid handle.
    if unsafe { GetWindowRect(hwnd, &mut rc) }.is_err() {
        return None;
    }
    Some((rc.left, rc.top, rc.right, rc.bottom))
}

#[must_use]
pub fn hits(hwnd: HWND, x: i32, y: i32) -> bool {
    screen_rect(hwnd).is_some_and(|(l, t, r, b)| x >= l && y >= t && x < r && y < b)
}
