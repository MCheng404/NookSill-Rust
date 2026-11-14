//! Window procedure, Win32 helpers and the summary line.

use super::*;

pub(super) unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    const WM_CHAR: u32 = 0x0102;
    const WM_KEYDOWN: u32 = 0x0100;
    const VK_ESCAPE: usize = 0x1B;
    const VK_RETURN: usize = 0x0D;
    const VK_BACK: usize = 0x08;

    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut ToolWindow;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let win = unsafe { &mut *ptr };
    if let Some(result) = win.on_message(msg, wparam, lparam) {
        return result;
    }
    match msg {
        // Keyboard handling while typing a todo. `WM_KEYDOWN` is converted
        // into characters instead of reading `WM_CHAR` directly, so backspace,
        // Enter and Esc travel the same path as ordinary characters.
        WM_KEYDOWN => {
            // Outside typing mode Esc hides the panel; it closes without the
            // mouse capture, which is the only way out when `PanelDismiss`
            // is off. Esc while typing is handled by `on_char`.
            if !win.typing && wparam.0 == VK_ESCAPE {
                win.hide();
                return LRESULT(0);
            }
            if win.typing {
                let handled = match wparam.0 {
                    VK_ESCAPE => win.on_char(0x1b),
                    VK_RETURN => win.on_char(b'\r' as u32),
                    VK_BACK => win.on_char(0x08),
                    _ => false,
                };
                if handled {
                    win.next_repaint = Instant::now();
                    return LRESULT(0);
                }
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_CHAR => {
            if win.on_char(wparam.0 as u32) {
                win.next_repaint = Instant::now();
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

impl Drop for ToolWindow {
    fn drop(&mut self) {
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
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

pub(super) fn physical_size(ppp: f32, h: f32) -> (i32, i32) {
    ((PANEL_W * ppp).round() as i32, (h * ppp).round() as i32)
}

pub(super) fn client_pos(lparam: LPARAM, ppp: f32) -> (f32, f32) {
    let x = (lparam.0 & 0xFFFF) as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    (x / ppp, y / ppp)
}

/// Milliseconds since start, monotonically increasing, for timing.
#[must_use]
pub fn now_ms() -> i64 {
    use std::sync::OnceLock;
    static START: OnceLock<Instant> = OnceLock::new();
    let start = START.get_or_init(Instant::now);
    i64::try_from(start.elapsed().as_millis()).unwrap_or(i64::MAX)
}

/// Text of the panel's summary line (used by tests).
#[must_use]
pub fn summary(state: &ToolState, now_ms: i64) -> String {
    nooksill_core::i18n::fmt(
        "{0} {1}  To-do {2}/{3}",
        &[
            state.mode.title().to_string().as_str(),
            format_ms(state.display_ms(now_ms), state.mode)
                .to_string()
                .as_str(),
            state
                .todos
                .iter()
                .filter(|t| t.done)
                .count()
                .to_string()
                .as_str(),
            state.todos.len().to_string().as_str(),
        ],
    )
}
