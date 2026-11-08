//! Window geometry: bounce curve, origin and size, AppBar thickness, coordinate maps.

use super::Rect;
use super::*;

/// Bounce duration after launch, in milliseconds.
pub const BOUNCE_MS: u64 = 1100;

/// Bounce offset in logical pixels, positive away from the bar.
///
/// Two full bounces (`|sin|`), amplitude decaying as `1 - 0.55x`. Scaling with
/// `s` keeps the bounce proportional across DPI. 0 means no animation.
#[must_use]
pub fn bounce_offset(elapsed_ms: u64, s: f32) -> f32 {
    if elapsed_ms >= BOUNCE_MS {
        return 0.0;
    }
    let x = elapsed_ms as f32 / BOUNCE_MS as f32;
    let phase = x * std::f32::consts::PI * 2.0;
    phase.sin().abs() * (1.0 - x * 0.55) * s * 0.48
}

/// Returns true while the bounce is still running.
#[must_use]
pub fn bounce_active(elapsed_ms: u64) -> bool {
    elapsed_ms < BOUNCE_MS
}

/// Screen position of the window's top-left corner, physical pixels.
///
/// `reserved` is the rect the AppBar actually got; the window hugs it so the bar
/// stays flush with the released desktop area. Sizes are not guessed here.
#[must_use]
pub fn window_origin(
    edge: Edge,
    work: Rect,
    reserved: Option<Rect>,
    win_w: i32,
    win_h: i32,
) -> (i32, i32) {
    let center_x = work.left + (work.width() - win_w) / 2;
    let center_y = work.top + (work.height() - win_h) / 2;
    match edge {
        Edge::Top => (center_x, reserved.map_or(work.top, |r| r.top)),
        Edge::Bottom => (center_x, reserved.map_or(work.bottom, |r| r.bottom) - win_h),
        Edge::Left => (reserved.map_or(work.left, |r| r.left), center_y),
        Edge::Right => (reserved.map_or(work.right, |r| r.right) - win_w, center_y),
    }
}

/// Window size in physical pixels: `(lw, lh)` horizontal, transposed when vertical.
///
/// Shared by window creation and AppBar thickness; getting it wrong once leaves a
/// vertical dock one thin sliver wide.
#[must_use]
pub fn physical_size(edge: Edge, lw: i32, lh: i32) -> (i32, i32) {
    if edge.is_horizontal() {
        (lw, lh)
    } else {
        (lh, lw)
    }
}

/// Thickness the AppBar reserves, physical pixels.
///
/// Bar plus margin plus 4 px, so a maximized window edge does not touch the bar.
#[must_use]
pub fn reserve_thickness(bar_h: f32, margin: f32) -> i32 {
    (bar_h + margin + 4.0) as i32
}

/// Logical coordinates to window coordinates.
///
/// `lh` is the current logical height; only the top and left edges flip.
#[must_use]
pub fn map_rect(edge: Edge, u: f32, v: f32, w: f32, h: f32, lh: f32) -> (f32, f32, f32, f32) {
    match edge {
        Edge::Top => (u, lh - v - h, w, h),
        Edge::Left => (lh - v - h, u, h, w),
        Edge::Right => (v, u, h, w),
        Edge::Bottom => (u, v, w, h),
    }
}

/// Window coordinates to logical coordinates, for a single point.
///
/// Inverse of [`map_rect`] on a corner: pass the flipped edge, not the origin.
#[must_use]
pub fn to_logical(edge: Edge, px: f32, py: f32, lh: f32) -> (f32, f32) {
    match edge {
        Edge::Top => (px, lh - py),
        Edge::Left => (py, lh - px),
        Edge::Right => (py, px),
        Edge::Bottom => (px, py),
    }
}
