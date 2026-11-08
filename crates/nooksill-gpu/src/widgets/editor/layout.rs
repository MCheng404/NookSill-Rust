//! Edit dialog geometry and placement.

use super::*;

/// Reusable slot for the dialog window: handle plus its own renderer.
///
/// The renderer must be separate: the Dock's `FrameRenderer` is fixed to the
/// bar size while the dialog needs a different one. `resize` rebuilds only
/// the offscreen target and DIB, keeping the egui context and the wgpu
/// pipeline, the two expensive steps.
pub type EditSlot = Option<(HWND, FrameRenderer)>;

/// Currently held modifier keys, as `egui::Modifiers`. Ctrl+A and Ctrl+V
/// need them.
pub(super) fn current_modifiers() -> egui::Modifiers {
    // SAFETY: `GetKeyState` only reads global state.
    let down = |vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| unsafe {
        GetKeyState(i32::from(vk.0)) < 0
    };
    let ctrl = down(VK_CONTROL);
    egui::Modifiers {
        alt: down(VK_MENU),
        ctrl,
        shift: down(VK_SHIFT),
        mac_cmd: false,
        command: ctrl,
    }
}

/// Height occupied by one field.
pub(super) fn field_height(field: &EditField) -> f32 {
    LABEL_H + if field.multiline { MULTI_H } else { FIELD_H } + FIELD_GAP
}

/// Logical height of the dialog.
pub(super) fn logical_height(fields: &[EditField]) -> f32 {
    PAD * 2.0 + TITLE_H + fields.iter().map(field_height).sum::<f32>() + BTN_H
}

/// Logical size of the dialog.
#[must_use]
pub fn logical_size(fields: &[EditField]) -> (f32, f32) {
    (W, logical_height(fields))
}

/// Places the dialog at the cursor, fully inside `bounds` (the screen work
/// area).
#[must_use]
pub fn place(cursor: (f32, f32), size: (f32, f32), bounds: (f32, f32, f32, f32)) -> (f32, f32) {
    let (cx, cy) = cursor;
    let (w, h) = size;
    let (left, top, right, bottom) = bounds;
    let mut x = cx;
    let mut y = cy;
    if x + w > right {
        x = right - w;
    }
    if x < left {
        x = left;
    }
    if y + h > bottom {
        y = cy - h;
    }
    if y < top {
        y = top;
    }
    (x, y)
}
