//! Win32 side of the panel framework: placement lookup and backdrop binding.

use nooksill_core::config::panel::PanelKind;
use nooksill_core::config::Edge;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWLP_USERDATA, SHOW_WINDOW_CMD,
    SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOW, SW_SHOWNA,
};

use super::super::anchor;
use super::super::style::PanelStyle;
use super::{placement, prefs};
use crate::frame::FrameRenderer;
use crate::frost;

/// Place `kind` from its published policy.
///
/// `anchor` is the card or item rect in physical pixels; panels opened from
/// the tray or a menu have none and fall back to the work-area centre.
#[must_use]
pub fn place_panel(
    kind: PanelKind,
    size: (i32, i32),
    anchor: Option<(f32, f32, f32, f32)>,
    edge: Edge,
    ppp: f32,
) -> (i32, i32) {
    let (monitor, work) = match anchor {
        Some(card) => (anchor::monitor_of(card), anchor::work_area_of(card)),
        None => (
            nooksill_platform::display::target_monitor_rect(),
            nooksill_platform::display::target_work_area(),
        ),
    };
    let req = placement::Request {
        size,
        anchor,
        edge,
        ppp,
        monitor,
        work,
    };
    placement::place(prefs::current().placement(kind), &req)
}

/// Blur geometry shared by the open and resize paths.
fn spec() -> frost::Spec {
    frost::Spec {
        radius: prefs::current().blur as u32,
        corner_pt: f32::from(PanelStyle::R_PANEL),
    }
}

/// Bind a panel's renderer to the published background material.
///
/// Must run after the window is positioned and before it is shown: the first
/// capture must not include the panel itself.
pub fn apply_backdrop(renderer: &mut FrameRenderer, hwnd: HWND, ppp: f32) {
    if !prefs::frost_wanted() {
        release_backdrop(renderer, hwnd);
        renderer.set_frost_whole_window(false);
        return;
    }
    let spec = spec();
    frost::exclude_from_capture(hwnd);
    spec.apply(renderer, hwnd, ppp);
    if !renderer.frost_on() {
        // RDP / VM capture failure: an opaque, screenshot-visible panel beats
        // an invisible one with no blur at all.
        release_backdrop(renderer, hwnd);
        renderer.set_frost_whole_window(false);
        return;
    }
    renderer.start_live_frost(hwnd, spec, ppp);
    renderer.set_frost_whole_window(true);
}

/// Re-capture after a resize; the old blur texture would be stretched.
pub fn refresh_backdrop(renderer: &mut FrameRenderer, hwnd: HWND, ppp: f32) {
    if !prefs::frost_wanted() || !renderer.has_frost() {
        return;
    }
    let spec = spec();
    spec.apply(renderer, hwnd, ppp);
    if renderer.frost_on() {
        renderer.set_frost_spec(spec);
    } else {
        release_backdrop(renderer, hwnd);
        renderer.set_frost_whole_window(false);
    }
}

/// Stop the capture and hand the window back to screenshots.
pub fn release_backdrop(renderer: &mut FrameRenderer, hwnd: HWND) {
    renderer.clear_frost();
    renderer.stop_live_frost();
    frost::allow_capture(hwnd);
}

/// Place, bind the backdrop and show a panel without taking focus.
///
/// Every panel used to repeat this sequence: a missed step left a panel at the
/// old position or with a stale capture. Returns the screen position.
pub fn show_panel(
    renderer: &mut FrameRenderer,
    hwnd: HWND,
    kind: PanelKind,
    size: (i32, i32),
    anchor: Option<(f32, f32, f32, f32)>,
    edge: Edge,
    ppp: f32,
) -> (i32, i32) {
    show_panel_with(renderer, hwnd, kind, size, anchor, edge, ppp, SW_SHOWNA)
}

/// Same as [`show_panel`] but activates the window, for panels with an input
/// field (notes, todo, group rename).
pub fn show_panel_active(
    renderer: &mut FrameRenderer,
    hwnd: HWND,
    kind: PanelKind,
    size: (i32, i32),
    anchor: Option<(f32, f32, f32, f32)>,
    edge: Edge,
    ppp: f32,
) -> (i32, i32) {
    show_panel_with(renderer, hwnd, kind, size, anchor, edge, ppp, SW_SHOW)
}

#[expect(
    clippy::too_many_arguments,
    reason = "win32 call shape; the public wrappers keep the call sites short"
)]
fn show_panel_with(
    renderer: &mut FrameRenderer,
    hwnd: HWND,
    kind: PanelKind,
    size: (i32, i32),
    anchor: Option<(f32, f32, f32, f32)>,
    edge: Edge,
    ppp: f32,
    show: SHOW_WINDOW_CMD,
) -> (i32, i32) {
    let (x, y) = place_panel(kind, size, anchor, edge, ppp);
    // SAFETY: the handle belongs to this process; failure only means the
    // window is gone, which the caller's visibility flag already covers.
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            x,
            y,
            size.0,
            size.1,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    // Must run after positioning: the first capture must not include the panel.
    apply_backdrop(renderer, hwnd, ppp);
    unsafe {
        let _ = ShowWindow(hwnd, show);
    }
    (x, y)
}

/// Hide a panel and release its backdrop in one step.
pub fn hide_panel(renderer: &mut FrameRenderer, hwnd: HWND) {
    release_backdrop(renderer, hwnd);
    // SAFETY: same reasoning as `show_panel`.
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

/// Tear a panel down: stop the capture, clear the user-data pointer, then
/// destroy the window.
///
/// `Drop` calls this so no message can reach a half-dropped object: the
/// pointer is cleared before `DestroyWindow` sends `WM_DESTROY`.
pub fn destroy_panel(renderer: &mut FrameRenderer, hwnd: HWND) {
    release_backdrop(renderer, hwnd);
    // SAFETY: the pointer and the handle belong to this panel; both calls are
    // no-ops when the window is already gone.
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        let _ = DestroyWindow(hwnd);
    }
}
