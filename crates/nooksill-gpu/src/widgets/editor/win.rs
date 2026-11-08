//! Edit dialog window: state pointer, procedure and class registration.

use super::layout::current_modifiers;
use super::*;

/// Internal dialog state, stored on `GWLP_USERDATA` and read back by the
/// window procedure.
pub(super) struct EditState {
    pub(super) req: EditRequest,
    pub(super) values: Vec<String>,
    /// Exit flag of the modal loop.
    pub(super) done: bool,
    /// The user accepted.
    pub(super) accepted: bool,
    /// DPI scale: physical sizes use it, hit tests convert back with it.
    pub(super) ppp: f32,
    pub(super) dark: bool,
    /// Whether a frosted backdrop exists, which picks a translucent tint over
    /// a solid fill.
    pub(super) frosted: bool,
    pub(super) input: Input,
    pub(super) ime: Ime,
    /// Whether any field is multiline, which decides if Enter submits or
    /// inserts a newline.
    pub(super) multiline: bool,
    /// Whether focus was already given to the first input.
    pub(super) focused: bool,
    /// Logical coords of the text start in the focused field, for
    /// `ImmSetCompositionWindow`.
    pub(super) caret: (f32, f32),
}

pub(super) fn state_ptr(hwnd: HWND) -> *mut EditState {
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditState }
}

/// Converts client coords (physical px) to logical coords.
pub(super) fn client_pos(lparam: LPARAM, ppp: f32) -> (f32, f32) {
    let x = (lparam.0 & 0xFFFF) as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    (x / ppp, y / ppp)
}

pub(super) unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    const VK_RETURN: usize = 0x0D;
    const VK_ESCAPE: usize = 0x1B;

    let ptr = state_ptr(hwnd);
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    // SAFETY: the pointer is stored by `show_edit` on a stack `Box` that
    // lives for the whole modal loop.
    let win = unsafe { &mut *ptr };
    match msg {
        WM_MOUSEMOVE => {
            let (x, y) = client_pos(lparam, win.ppp);
            win.input.pointer_moved(x, y);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = client_pos(lparam, win.ppp);
            win.input.pointer_moved(x, y);
            win.input.pointer_button(true, egui::PointerButton::Primary);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = client_pos(lparam, win.ppp);
            win.input.pointer_moved(x, y);
            win.input
                .pointer_button(false, egui::PointerButton::Primary);
            LRESULT(0)
        }
        WM_CHAR => {
            // Newlines are not handled here: egui 0.37's `Event::Text`
            // explicitly ignores "\n" and "\r" (newlines come from
            // `Key::Enter`). This path only takes printable characters, and
            // control characters (including `\r` and backspace) are handled
            // through `Event::Key`.
            if let Some(c) = char::from_u32(wparam.0 as u32) {
                if !c.is_control() {
                    win.input.text(&c.to_string());
                }
            }
            LRESULT(0)
        }
        WM_IME_COMPOSITION
        | WM_IME_STARTCOMPOSITION
        | WM_IME_ENDCOMPOSITION
        | WM_IME_SETCONTEXT => {
            let mut committed = String::new();
            let before = win.ime.composing().to_owned();
            let handled = win.ime.handle(hwnd, msg, wparam, lparam, &mut committed);
            // Commit first, preedit second. A commit clears the previous
            // frame's preedit before inserting the result, and one
            // `WM_IME_COMPOSITION` may carry both RESULTSTR and COMPSTR
            // (commit and next round at once), so the reverse order would
            // delete the new composition as if it were the old preedit.
            if !committed.is_empty() {
                win.input.ime_commit(&committed);
            }
            let after = win.ime.composing().to_owned();
            if after != before {
                // Let egui draw the underlined preedit text. Without it only
                // the system candidate box moves and the field shows nothing
                // of what is being typed.
                win.input.ime_preedit(&after);
            }
            if handled {
                LRESULT(0)
            } else {
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
        }
        WM_KEYDOWN | WM_KEYUP => {
            let down = msg == WM_KEYDOWN;
            // egui's `TextEdit` reads backspace, arrows and Ctrl+A from
            // `Event::Key` only; `WM_CHAR` carries printable characters and
            // control characters were filtered out above. Without this block
            // the field accepts typing but cannot delete.
            let mods = current_modifiers();
            win.input.set_modifiers(mods);
            let is_return = wparam.0 == VK_RETURN;
            if down {
                match wparam.0 {
                    // Enter during composition selects a word: it must not
                    // submit and must not reach the input field. In a
                    // multiline body Enter is a newline, so only a single
                    // line field, or Ctrl+Enter, submits the dialog.
                    VK_RETURN if !win.ime.is_composing() && (!win.multiline || mods.ctrl) => {
                        win.accepted = true;
                        win.done = true;
                    }
                    VK_ESCAPE if !win.ime.is_composing() => {
                        win.accepted = false;
                        win.done = true;
                    }
                    _ => {}
                }
            }
            // Enter and Esc already closed the dialog, nothing left to feed
            // egui.
            if !win.done {
                // Enter during composition picks a candidate; feeding
                // `Event::Key{Enter}` would insert a newline in the middle
                // of the composition and destroy it.
                let composing_return = is_return && win.ime.is_composing();
                if !composing_return {
                    if let Some(key) = crate::input::map_vk(wparam.0 as u32) {
                        win.input.key(key, down);
                    }
                }
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            // The I-beam inside input fields, an arrow elsewhere. An empty
            // class cursor falls back to the busy cursor, so it is set here.
            crate::cursor::set(crate::cursor::arrow());
            LRESULT(1)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Registers the window class. Repeated calls are safe (a failing
/// `RegisterClassW` is ignored).
pub(super) fn register_class() -> Result<(), Box<dyn std::error::Error>> {
    let hinst = unsafe { GetModuleHandleW(None)? };
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wndproc),
        hInstance: HINSTANCE(hinst.0),
        lpszClassName: class_pcwstr(),
        hCursor: crate::cursor::class_cursor(),
        hIcon: crate::app_icon().unwrap_or_default(),
        ..Default::default()
    };
    unsafe {
        RegisterClassW(&raw const wc);
    }
    Ok(())
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
