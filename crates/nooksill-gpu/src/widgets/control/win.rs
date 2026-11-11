//! Window procedure, Win32 placement and helpers.

use super::*;

pub(super) unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut CcWindow;
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

impl Drop for CcWindow {
    fn drop(&mut self) {
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
        }
    }
}

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

pub(super) fn physical_size(ppp: f32, tiles: usize) -> (i32, i32) {
    (
        (PANEL_W * ppp).round() as i32,
        (panel_h(tiles) * ppp).round() as i32,
    )
}

/// Places the panel next to the card by the same rules as the calendar
/// popup, then clamps it into the monitor.
pub(super) fn place(
    pw: i32,
    ph: i32,
    anchor: (f32, f32, f32, f32),
    edge: nooksill_core::config::Edge,
    ppp: f32,
) -> (i32, i32) {
    let mon = super::super::anchor::monitor_of(anchor);
    super::super::anchor::place(pw, ph, anchor, edge, ppp, mon)
}

/// Converts client coords (physical px) to logical points.
pub(super) fn client_pos(lparam: LPARAM, ppp: f32) -> (f32, f32) {
    let x = (lparam.0 & 0xffff) as u16 as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xffff) as u16 as i16 as f32;
    (x / ppp, y / ppp)
}

/// Tells whether this window is the control centre panel.
#[must_use]
pub fn is_cc_window(hwnd: HWND) -> bool {
    let mut buf = [0_u16; 64];
    let n = unsafe { windows::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, &mut buf) };
    n > 0 && String::from_utf16_lossy(&buf[..n as usize]) == CLASS_NAME
}
