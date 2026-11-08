//! Modal edit dialog: rename, new group, retarget and launch arguments.
//!
//! It is a separate window because the layered Dock window clips anything
//! painted outside the bar; the context menu escapes it only by owning a
//! window of its own.
//!
//! Modal: it runs its own message loop and blocks until the user accepts or
//! cancels (same approach as the context menu, see `show_menu` in
//! `bin/dock.rs`).
//!
//! Text input goes through `WM_CHAR` + `WM_IME_COMPOSITION`:
//!
//! - `WM_CHAR` -> `Input::text` -> egui's `Event::Text` (egui text fields
//!   only accept that event, `Event::Key` inserts nothing);
//! - the committed string of `WM_IME_COMPOSITION` takes the same path, and
//!   the system draws the candidate window.
//!
//! Backspace, Esc and Enter go through `WM_KEYDOWN` -> `Input::key`.

use crate::widgets::style::PanelStyle;
use egui::{Align2, Color32, CornerRadius, FontId, Id, Rect as EguiRect, Sense, Stroke};
use nooksill_settings::text_field::{text_field, FieldOpts, FIELD_H};
use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetCapture, GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetForegroundWindow, GetWindowLongPtrW,
    MsgWaitForMultipleObjectsEx, PeekMessageW, RegisterClassW, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, GWLP_USERDATA, HWND_TOPMOST,
    MSG, MWMO_INPUTAVAILABLE, PM_REMOVE, QS_ALLINPUT, SWP_NOZORDER, SW_HIDE, SW_SHOW, WM_CHAR,
    WM_IME_COMPOSITION, WM_IME_ENDCOMPOSITION, WM_IME_SETCONTEXT, WM_IME_STARTCOMPOSITION,
    WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_SETCURSOR, WNDCLASSW,
    WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_TOPMOST, WS_POPUP,
};

use crate::frame::FrameRenderer;
use crate::gpu::Gpu;
use crate::ime::Ime;
use crate::input::Input;

/// Window class name.
const CLASS_NAME: &str = "NookSillEditWindow";

/// Dialog width in logical px.
const W: f32 = 380.0;
/// Padding on all sides.
const PAD: f32 = 20.0;
/// Title height.
const TITLE_H: f32 = 30.0;
/// Field label height.
const LABEL_H: f32 = 18.0;
/// Gap between fields.
const FIELD_GAP: f32 = 14.0;
/// Button height.
const BTN_H: f32 = 30.0;
/// Height of a multiline field.
const MULTI_H: f32 = 170.0;

/// One field to edit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditField {
    /// Field name, drawn above the input.
    pub label: String,
    /// Current value.
    pub value: String,
    /// Placeholder shown when the value is empty.
    pub hint: String,
    /// Multiline editing: Enter inserts a newline instead of submitting, and
    /// the window grows taller.
    pub multiline: bool,
}

/// One edit request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditRequest {
    /// Dialog title.
    pub title: String,
    /// Fields, drawn in order.
    pub fields: Vec<EditField>,
    /// Label on the accept button.
    pub ok_label: String,
}

/// Result of an edit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditOutcome {
    /// The user accepted (`false` means cancelled).
    pub accepted: bool,
    /// New value of each field, matching the request order.
    pub values: Vec<String>,
}

impl EditOutcome {
    /// Returns field `i`; out of range yields an empty string.
    #[must_use]
    pub fn value(&self, i: usize) -> &str {
        self.values.get(i).map_or("", String::as_str)
    }
}

mod layout;
mod paint;
mod show;
mod win;

pub use self::layout::{logical_size, place, EditSlot};
pub use self::paint::paint_preview;
pub use self::show::show_edit;

#[cfg(test)]
mod tests {
    use super::layout::{logical_height, logical_size, place};
    use super::*;

    #[test]
    fn height_grows_with_the_field_count() {
        let one = logical_height(&[EditField::default()]);
        let two = logical_height(&[EditField::default(), EditField::default()]);
        assert!(two > one);
        assert!((two - one - (LABEL_H + FIELD_H + FIELD_GAP)).abs() < 0.01);
    }

    /// A multiline field must actually grow the window, else the body is
    /// clipped.
    #[test]
    fn a_multiline_field_makes_the_window_taller() {
        let single = logical_height(&[EditField::default()]);
        let multi = logical_height(&[EditField {
            multiline: true,
            ..EditField::default()
        }]);
        assert!((multi - single - (MULTI_H - FIELD_H)).abs() < 0.01);
        assert!(
            logical_size(&[EditField {
                multiline: true,
                ..EditField::default()
            }])
            .1 > single
        );
    }

    #[test]
    fn place_keeps_the_window_inside_the_work_area() {
        let size = (380.0, 200.0);
        let bounds = (0.0, 0.0, 2560.0, 1600.0);
        let (x, y) = place((2500.0, 1500.0), size, bounds);
        assert!(x + size.0 <= bounds.2 + 0.01, "不该顶出右边");
        assert!(y >= bounds.1 - 0.01, "不该顶出上边");
        assert!(y + size.1 <= bounds.3 + 0.01, "不该顶出下边");
    }

    #[test]
    fn outcome_reads_fields_by_index() {
        let o = EditOutcome {
            accepted: true,
            values: vec!["a".into(), "b".into()],
        };
        assert_eq!(o.value(0), "a");
        assert_eq!(o.value(9), "", "越界返回空串");
    }
}
