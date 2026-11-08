//! Magnet pull: elements near the pointer drift toward it while their hit
//! rect stays in layout position.

use super::*;

/// Offset for an element pulled toward the pointer, at most `max` pixels.
///
/// `base` is the unshifted layout rect. The pull falls off with the distance
/// from the pointer to the rect edge: full inside, zero beyond `radius`.
///
/// Only the drawing moves; hit testing stays at the layout rect. The few pixel
/// gap is imperceptible and saves a visual/hit separation layer.
pub(super) fn magnet_offset(ui: &Ui, base: egui::Rect, max: f32, radius: f32) -> egui::Vec2 {
    let pointer = ui.ctx().input(|i| i.pointer.hover_pos());
    magnet_pull(base, pointer, max, radius)
}

/// Pure geometry, split out so it can be unit tested.
pub(super) fn magnet_pull(
    base: egui::Rect,
    pointer: Option<egui::Pos2>,
    max: f32,
    radius: f32,
) -> egui::Vec2 {
    let Some(p) = pointer else {
        return egui::Vec2::ZERO;
    };
    // Distance to the nearest rect edge, zero inside the rect.
    let nx = p.x.clamp(base.left(), base.right());
    let ny = p.y.clamp(base.top(), base.bottom());
    let dist = ((p.x - nx).powi(2) + (p.y - ny).powi(2)).sqrt();
    if dist > radius {
        return egui::Vec2::ZERO;
    }
    let pull = 1.0 - dist / radius;
    let c = base.center();
    let dx = p.x - c.x;
    let dy = p.y - c.y;
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    egui::vec2(dx / len * max * pull, dy / len * max * pull)
}
