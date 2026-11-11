//! Dock window procedure: translates Win32 messages into Dock state.

use super::drag::dock_cursor;
use super::sys::{arm_mouse_leave, dock_ptr, physical_xy};
use super::*;

/// Mouse message position mapped into the dock's logical axes.
fn logical_xy(dock: *mut Dock, lparam: LPARAM) -> (f32, f32) {
    let (px, py) = physical_xy(lparam);
    if dock.is_null() {
        return (px, py);
    }
    unsafe { (*dock).logical_point(px, py) }
}

pub(super) unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        // The tray icon was clicked. The window procedure only holds a `Dock`
        // pointer, so the click is recorded for the main loop: opening a menu
        // must not block inside the window procedure.
        nooksill_platform::tray::WM_APP_TRAY => {
            if let Some(toggle) = nooksill_platform::tray::Tray::parse(lparam.0) {
                let ptr = dock_ptr(hwnd);
                if !ptr.is_null() {
                    unsafe {
                        (*ptr).push_tray_click(toggle);
                    }
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEMOVE => {
            let ptr = dock_ptr(hwnd);
            let (u, v) = logical_xy(ptr, lparam);
            if !ptr.is_null() {
                unsafe {
                    (*ptr).mouse = (u, v);
                    (*ptr).hover = true;
                    // Feed the move to the drag state machine while a button is
                    // held; it returns early when nothing is pressed.
                    (*ptr).drag_to(u, v);
                    (*ptr).mark_dirty();
                }
                // The cursor has to be set here too: while a button is held
                // (`SetCapture`) the system sends no `WM_SETCURSOR`, so the
                // whole drag would show the busy cursor.
                dock_cursor(&*ptr, u, v);
                // Arm `WM_MOUSELEAVE`: without it `hover` stays true and the
                // Dock never leaves the magnified state.
                arm_mouse_leave(hwnd);
            }
            LRESULT(0)
        }
        // The mouse left the window: cancel the hover.
        WM_MOUSELEAVE => {
            let ptr = dock_ptr(hwnd);
            if !ptr.is_null() {
                unsafe {
                    (*ptr).mouse = (-1.0, -1.0);
                    (*ptr).hover = false;
                    (*ptr).mark_dirty();
                }
            }
            LRESULT(0)
        }
        // Drop: the paths are recorded and the main loop picks them up.
        // The window procedure has no `App`, only a `Dock` pointer, so it
        // cannot create items, that would change `raw`.
        WM_DROPFILES => {
            let hdrop = HDROP(wparam.0 as *mut core::ffi::c_void);
            let n = unsafe { DragQueryFileW(hdrop, u32::MAX, None) };
            let mut files = Vec::new();
            for i in 0..n {
                let len = unsafe { DragQueryFileW(hdrop, i, None) };
                let mut buf = vec![0_u16; len as usize + 1];
                let got = unsafe { DragQueryFileW(hdrop, i, Some(buf.as_mut_slice())) };
                if got > 0 {
                    buf.truncate(got as usize);
                    files.push(String::from_utf16_lossy(&buf));
                }
            }
            unsafe {
                DragFinish(hdrop);
            }
            // Drop position. `GetCursorPos + ScreenToClient` has a defined
            // coordinate system, while `DragQueryPoint` only says "relative
            // to the window that received WM_DROPFILES" and does not settle
            // screen or client, so it is not used for the insertion point.
            // Without a cursor the files are appended instead of being
            // inserted at (0,0) in front of everything.
            let mut pt = POINT::default();
            let has_pt = unsafe { GetCursorPos(&raw mut pt) }.is_ok()
                && unsafe { ScreenToClient(hwnd, &raw mut pt) }.as_bool();
            let ptr = dock_ptr(hwnd);
            let (x, y) = if has_pt {
                let p = (pt.x as f32, pt.y as f32);
                if ptr.is_null() {
                    p
                } else {
                    unsafe { (*ptr).logical_point(p.0, p.1) }
                }
            } else {
                (f32::MAX, 0.0)
            };
            if !ptr.is_null() {
                unsafe {
                    (*ptr).push_dropped_paths(files, x, y);
                }
            }
            LRESULT(0)
        }
        // Cursor: the class cursor only covers the moment the pointer enters
        // the window, and the system sends this again on every move, click
        // and Alt+Tab. Without a reply it falls back to the system default,
        // which is the busy cursor.
        WM_SETCURSOR => {
            let ptr = dock_ptr(hwnd);
            if !ptr.is_null() {
                if let Some((px, py)) = nooksill_gpu::cursor::client_pos(hwnd) {
                    let (u, v) = (*ptr).logical_point(px, py);
                    dock_cursor(&*ptr, u, v);
                }
            }
            LRESULT(1)
        }
        WM_LBUTTONDOWN => {
            let ptr = dock_ptr(hwnd);
            let (u, v) = logical_xy(ptr, lparam);
            if !ptr.is_null() {
                // `SetCapture` on a hit: all later mouse messages come here,
                // so the button-up is not lost when the pointer leaves the
                // window. Shift must be read **at press time**: the user may
                // already have released it at button-up, and Shift+click
                // means "open a second instance".
                let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
                if unsafe { (*ptr).press_at(u, v, shift) } {
                    unsafe {
                        let _ = SetCapture(hwnd);
                    }
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let ptr = dock_ptr(hwnd);
            let (u, v) = logical_xy(ptr, lparam);
            if !ptr.is_null() {
                unsafe {
                    // **Settle the click before releasing the capture.** The
                    // other order swallows every click: `ReleaseCapture()`
                    // **synchronously** sends a `WM_CAPTURECHANGED` to this
                    // window, and that branch calls `cancel_drag()`, which
                    // clears `press` before `release_at()` ever runs.
                    // Widget cards sit outside the bar, not in an item slot,
                    // so they are hit tested first.
                    if let Some(kind) = (*ptr).widget_at(u, v) {
                        (*ptr).click_widget(kind);
                    } else {
                        // Only record which item was hit; the launch happens
                        // in the main loop because `ShellExecuteW` can block
                        // for tens of milliseconds.
                        let _ = (*ptr).release_at(u, v);
                    }
                    (*ptr).mark_dirty();
                    // Hand the capture back only after settling, so the
                    // `WM_CAPTURECHANGED` has no state left to clear.
                    if GetCapture() == hwnd {
                        let _ = ReleaseCapture();
                    }
                }
            }
            LRESULT(0)
        }
        // **Never steal the focus.** The Dock is a resident tool window; if
        // clicking an icon took the focus away from the window the user is
        // working in, the app that starts would not get the foreground and
        // the whole desktop would flash.
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        // The mouse capture was taken away (UAC dialog, Alt+Tab, another
        // program calling `SetCapture`): the button-up never arrives, so the
        // drag has to be cancelled explicitly, otherwise the next click
        // carries the previous press state and launches the wrong icon.
        WM_CAPTURECHANGED => {
            let ptr = dock_ptr(hwnd);
            if !ptr.is_null() {
                unsafe {
                    (*ptr).cancel_drag();
                }
            }
            LRESULT(0)
        }
        // Middle button forces a new instance.
        WM_MBUTTONUP => {
            let ptr = dock_ptr(hwnd);
            let (u, v) = logical_xy(ptr, lparam);
            if !ptr.is_null() {
                unsafe {
                    (*ptr).force_click(u, v);
                    (*ptr).mark_dirty();
                }
            }
            LRESULT(0)
        }
        // `Ctrl+Alt+D` forces hide or restore, also in always-visible mode.
        WM_HOTKEY => {
            let ptr = dock_ptr(hwnd);
            if !ptr.is_null() {
                let hidden = unsafe { (*ptr).toggle_force_hide() };
                eprintln!(
                    "[hotkey] {}",
                    if hidden {
                        nooksill_core::i18n::t("Hidden")
                    } else {
                        nooksill_core::i18n::t("Restored")
                    }
                );
            }
            LRESULT(0)
        }
        // A system dark mode switch (`ImmersiveColorSet`) must take effect
        // **immediately**: a theme read only at startup leaves the Dock light
        // until the next restart.
        WM_SETTINGCHANGE => {
            let ptr = dock_ptr(hwnd);
            if !ptr.is_null() {
                unsafe {
                    (*ptr).mark_dirty();
                    (*ptr).anim = true;
                    // The accent colour and the dark mode live in `App`, which
                    // the window procedure cannot reach, so a counter passes
                    // "this has to be re-read" upwards.
                    (*ptr).look_epoch = (*ptr).look_epoch.wrapping_add(1);
                }
            }
            LRESULT(0)
        }
        // Right click: only the position is recorded, the main loop opens the
        // menu because a modal window must not block in the procedure.
        WM_RBUTTONDOWN => LRESULT(0),
        WM_RBUTTONUP => {
            let ptr = dock_ptr(hwnd);
            let (u, v) = logical_xy(ptr, lparam);
            if !ptr.is_null() {
                unsafe {
                    (*ptr).request_menu(u, v);
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 as u32 == 0x1B {
                unsafe { PostQuitMessage(0) };
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
