//! Win32 side of the panel framework: placement lookup and backdrop binding.

use nooksill_core::config::panel::PanelKind;
use nooksill_core::config::Edge;
use windows::Win32::Foundation::HWND;

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
