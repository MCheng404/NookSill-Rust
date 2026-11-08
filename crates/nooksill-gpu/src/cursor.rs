//! System cursors.
//!
//! A window class with a null `hCursor` shows the busy cursor instead of the
//! arrow, and `SetCapture` suppresses `WM_SETCURSOR` entirely. Both need the
//! explicit handling below.

use std::sync::atomic::{AtomicUsize, Ordering};

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, LoadCursorW, SetCursor, HCURSOR, IDC_APPSTARTING, IDC_ARROW, IDC_HAND, IDC_IBEAM,
    IDC_SIZEALL, IDC_SIZENS, IDC_WAIT,
};

/// Cache of system cursor handles.
///
/// Called from `WM_MOUSEMOVE`, up to hundreds of times per second while a window
/// holds capture. `LoadCursorW` is cheap but not free. Handles are process-wide
/// shared resources that are never freed, so storing them as `usize` is sound;
/// atomics instead of `OnceLock` because `HCURSOR` is not `Sync`.
static ARROW_HANDLE: AtomicUsize = AtomicUsize::new(0);
static HAND_HANDLE: AtomicUsize = AtomicUsize::new(0);
static IBEAM_HANDLE: AtomicUsize = AtomicUsize::new(0);
static SIZEALL_HANDLE: AtomicUsize = AtomicUsize::new(0);
static SIZENS_HANDLE: AtomicUsize = AtomicUsize::new(0);
static APPSTARTING_HANDLE: AtomicUsize = AtomicUsize::new(0);
static WAIT_HANDLE: AtomicUsize = AtomicUsize::new(0);

/// Load and cache one system cursor. Returns a null handle on failure.
fn cached(slot: &AtomicUsize, id: windows::core::PCWSTR) -> HCURSOR {
    let hit = slot.load(Ordering::Relaxed);
    if hit != 0 {
        return HCURSOR(hit as *mut std::ffi::c_void);
    }
    let loaded = unsafe { LoadCursorW(None, id) }.unwrap_or_default();
    if !loaded.is_invalid() {
        slot.store(loaded.0 as usize, Ordering::Relaxed);
    }
    loaded
}

fn load(id: windows::core::PCWSTR) -> HCURSOR {
    match id {
        IDC_ARROW => cached(&ARROW_HANDLE, IDC_ARROW),
        IDC_HAND => cached(&HAND_HANDLE, IDC_HAND),
        IDC_IBEAM => cached(&IBEAM_HANDLE, IDC_IBEAM),
        IDC_SIZEALL => cached(&SIZEALL_HANDLE, IDC_SIZEALL),
        IDC_SIZENS => cached(&SIZENS_HANDLE, IDC_SIZENS),
        IDC_APPSTARTING => cached(&APPSTARTING_HANDLE, IDC_APPSTARTING),
        IDC_WAIT => cached(&WAIT_HANDLE, IDC_WAIT),
        other => unsafe { LoadCursorW(None, other) }.unwrap_or_default(),
    }
}

/// Fill `WNDCLASSW::hCursor` with this when registering a window class.
///
/// Leaving it null makes Windows show the busy cursor rather than the arrow.
pub fn class_cursor() -> HCURSOR {
    load(IDC_ARROW)
}

pub fn arrow() -> HCURSOR {
    load(IDC_ARROW)
}

/// Hand shape, for clickable targets.
pub fn hand() -> HCURSOR {
    load(IDC_HAND)
}

pub fn i_beam() -> HCURSOR {
    load(IDC_IBEAM)
}

/// Vertical resize arrow, for dragging the dock edge.
pub fn size_ns() -> HCURSOR {
    load(IDC_SIZENS)
}

/// Apply a cursor shape. A null handle is ignored so the cursor never goes blank.
pub fn set(cursor: HCURSOR) {
    if !cursor.is_invalid() {
        unsafe {
            SetCursor(Some(cursor));
        }
    }
}

/// Cursor position in `hwnd` client coordinates, in physical pixels.
///
/// `WM_SETCURSOR`'s `lparam` is a hit-test code, not a mouse position, and the
/// system sends it before `WM_MOUSEMOVE`, so a stored position is one event
/// stale. Query it directly.
pub fn client_pos(hwnd: HWND) -> Option<(f32, f32)> {
    let mut p = POINT::default();
    if unsafe { GetCursorPos(&raw mut p) }.is_err() {
        return None;
    }
    // Infallible in practice; only an invalid hwnd fails, and screen
    // coordinates are an acceptable fallback there.
    let _ = unsafe { ScreenToClient(hwnd, &raw mut p) };
    Some((p.x as f32, p.y as f32))
}

/// Set hand for clickable, arrow otherwise.
///
/// For windows that do not follow egui's cursor icon (dock window, context
/// menu). A window holding capture receives no `WM_SETCURSOR`, so these windows
/// must call this from `WM_MOUSEMOVE`.
pub fn set_hand_if(hand_cursor: bool) {
    set(if hand_cursor { hand() } else { arrow() });
}

/// Map an egui cursor icon to a system cursor.
pub fn from_icon(icon: egui::CursorIcon) -> HCURSOR {
    match icon {
        egui::CursorIcon::PointingHand => hand(),
        egui::CursorIcon::Text | egui::CursorIcon::VerticalText => i_beam(),
        egui::CursorIcon::Move | egui::CursorIcon::Grabbing | egui::CursorIcon::AllScroll => {
            load(IDC_SIZEALL)
        }
        // egui's `Progress` / `Wait` mean genuinely busy, matching the system
        // semantics; reuse the stock cursors rather than drawing our own.
        egui::CursorIcon::Progress => load(IDC_APPSTARTING),
        egui::CursorIcon::Wait => load(IDC_WAIT),
        _ => arrow(),
    }
}

/// Apply egui's requested cursor icon.
///
/// `platform_output.cursor_icon` is per-frame and more accurate than guessing
/// from hover: text fields switch to an I-beam and drags to a four-way arrow.
pub fn apply(icon: egui::CursorIcon) {
    set(from_icon(icon));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The common cursors must all return a handle.
    ///
    /// Guards against a wrong `LoadCursorW` id: it returns Err, which becomes a
    /// null handle, and `WM_SETCURSOR` then leaves the busy cursor in place.
    #[test]
    fn system_cursors_are_available() {
        for (name, h) in [
            ("arrow", arrow()),
            ("hand", hand()),
            ("i_beam", i_beam()),
            ("class", class_cursor()),
        ] {
            assert!(!h.is_invalid(), "{name} 光标取不到");
        }
    }

    /// `from_icon` must never return a null handle, which would leave the
    /// cursor stuck in its previous state.
    #[test]
    fn apply_covers_egui_icons() {
        // Exhaustive enough: new egui icons are silently skipped rather than
        // reported, which is acceptable for this guard.
        for icon in [
            egui::CursorIcon::Default,
            egui::CursorIcon::None,
            egui::CursorIcon::ContextMenu,
            egui::CursorIcon::Help,
            egui::CursorIcon::PointingHand,
            egui::CursorIcon::Progress,
            egui::CursorIcon::Wait,
            egui::CursorIcon::Cell,
            egui::CursorIcon::Crosshair,
            egui::CursorIcon::Text,
            egui::CursorIcon::VerticalText,
            egui::CursorIcon::Alias,
            egui::CursorIcon::Copy,
            egui::CursorIcon::Move,
            egui::CursorIcon::NoDrop,
            egui::CursorIcon::NotAllowed,
            egui::CursorIcon::Grab,
            egui::CursorIcon::Grabbing,
            egui::CursorIcon::AllScroll,
            egui::CursorIcon::ResizeHorizontal,
            egui::CursorIcon::ResizeVertical,
            egui::CursorIcon::ResizeNeSw,
            egui::CursorIcon::ResizeNwSe,
            egui::CursorIcon::ResizeEast,
            egui::CursorIcon::ResizeSouth,
            egui::CursorIcon::ResizeWest,
            egui::CursorIcon::ResizeNorth,
        ] {
            let h = from_icon(icon);
            assert!(!h.is_invalid(), "{icon:?} 映射出空句柄");
        }
    }
}
