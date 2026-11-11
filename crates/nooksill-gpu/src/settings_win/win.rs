//! Window procedure and Win32 class/geometry helpers.

use super::*;

pub(super) unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut SettingsWindow;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    // SAFETY: the pointer was stored by `attach` and points at the
    // `Box<SettingsWindow>` owned by the caller; the window procedure only
    // runs on the UI thread and never overlaps the main loop's render phase.
    let win = unsafe { &mut *ptr };
    if let Some(result) = win.on_message(msg, wparam, lparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Whether to print diagnostics: the window message flow and the input
/// events handed to egui.
///
/// Off by default via the `NOOKSILL_SETTINGS_TRACE=1` environment variable:
/// this output arrives every frame and would drown out real error messages.
pub(super) fn trace() -> bool {
    std::env::var_os("NOOKSILL_SETTINGS_TRACE").is_some()
}

/// The window class name as a UTF-16 wide string.
///
/// The buffer must stay valid for the whole process lifetime. Encoding a
/// temporary `Vec<u16>` in `new` and passing it to `CreateWindowExW` is
/// wrong: the memory is freed once the call returns, and every later use of
/// the class name then dereferences freed heap.
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
        (LOGICAL_SIZE.0 * ppp).round() as i32,
        (LOGICAL_SIZE.1 * ppp).round() as i32,
    )
}

pub(super) fn client_pos(lparam: LPARAM, ppp: f32) -> (f32, f32) {
    let x = (lparam.0 & 0xFFFF) as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    (x / ppp, y / ppp)
}

pub(super) fn current_modifiers() -> egui::Modifiers {
    let down = |vk: u16| unsafe { (GetKeyState(i32::from(vk)) as u16 & 0x8000) != 0 };
    egui::Modifiers {
        alt: down(VK_MENU.0),
        ctrl: down(VK_CONTROL.0),
        shift: down(VK_SHIFT.0),
        mac_cmd: false,
        command: down(VK_CONTROL.0),
    }
}
