//! Window procedure, Win32 placement and date helpers.

use super::*;

pub(super) unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut CalendarWindow;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    // SAFETY: the pointer is stored by `attach` on the final address of the
    // pinned `Box<Self>`.
    let win = unsafe { &mut *ptr };
    if let Some(result) = win.on_message(msg, wparam, lparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

impl Drop for CalendarWindow {
    fn drop(&mut self) {
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
        }
    }
}

/// The class name must be a UTF-16 wide string that outlives the process.
pub(super) fn class_pcwstr() -> windows::core::PCWSTR {
    static BUF: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();
    let buf = BUF.get_or_init(|| {
        CLASS_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect()
    });
    windows::core::PCWSTR(buf.as_ptr())
}

pub(super) fn physical_size(ppp: f32) -> (i32, i32) {
    (
        (PANEL_W * ppp).round() as i32,
        (PANEL_H * ppp).round() as i32,
    )
}

/// Places the popup next to the clock card, then clamps it to the monitor.
///
/// `anchor` and the result are physical pixels.
pub(super) fn place(
    pw: i32,
    ph: i32,
    anchor: (f32, f32, f32, f32),
    edge: Edge,
    ppp: f32,
) -> (i32, i32) {
    place_in(
        pw,
        ph,
        anchor,
        edge,
        ppp,
        nooksill_platform::display::monitor_rect(0),
    )
}

/// Same as [`place`], but the monitor rect comes from the caller.
///
/// Split out so tests are reproducible: the CI virtual screen is only
/// 1024x768, and the real `monitor_rect(0)` would clamp the popup elsewhere.
pub(super) fn place_in(
    pw: i32,
    ph: i32,
    anchor: (f32, f32, f32, f32),
    edge: Edge,
    ppp: f32,
    mon: nooksill_core::layout::Rect,
) -> (i32, i32) {
    let (al, at, ar, ab) = anchor;
    let ax = (al + ar) * 0.5;
    let ay = (at + ab) * 0.5;
    let pm = (PM * ppp).round() as i32;
    let gap = (4.0 * ppp).round() as i32;
    let (mut x, mut y) = match edge {
        Edge::Top => (ax - pw as f32 * 0.5, ab - pm as f32 + gap as f32),
        Edge::Left => (ar - pm as f32 + gap as f32, ay - ph as f32 * 0.5),
        Edge::Right => (
            al - pw as f32 + pm as f32 - gap as f32,
            ay - ph as f32 * 0.5,
        ),
        Edge::Bottom => (
            ax - pw as f32 * 0.5,
            at - ph as f32 + pm as f32 - gap as f32,
        ),
    };
    // Clamp into the monitor using `rcMonitor`, not the work area: covering
    // the taskbar is fine, leaving the screen is not.
    let (mut xi, mut yi) = (x.round() as i32, y.round() as i32);
    if xi + pw > mon.right {
        xi = mon.right - pw;
    }
    if yi + ph > mon.bottom {
        yi = mon.bottom - ph;
    }
    if xi < mon.left {
        xi = mon.left;
    }
    if yi < mon.top {
        yi = mon.top;
    }
    x = xi as f32;
    y = yi as f32;
    (x as i32, y as i32)
}

/// Converts client coords (physical px) to logical points.
pub(super) fn client_pos(lparam: LPARAM, ppp: f32) -> (f32, f32) {
    let x = (lparam.0 & 0xffff) as u16 as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xffff) as u16 as i16 as f32;
    (x / ppp, y / ppp)
}

/// Tells whether this window is the calendar popup.
#[must_use]
pub fn is_calendar_window(hwnd: HWND) -> bool {
    let mut buf = [0_u16; 64];
    let n = unsafe { windows::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, &mut buf) };
    n > 0 && String::from_utf16_lossy(&buf[..n as usize]) == CLASS_NAME
}

/// Reads today's local calendar date.
#[must_use]
pub fn today() -> (i32, u32, u32) {
    let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    (
        i32::from(st.wYear),
        u32::from(st.wMonth),
        u32::from(st.wDay),
    )
}
