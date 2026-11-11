//! Popup anchoring: place a panel next to a widget card and keep it on screen.
//!
//! One implementation for every panel: the calendar, the control centre and the
//! resource monitor all hang off a card, and a second copy of this math drifts
//! the moment a margin changes.

use nooksill_core::config::Edge;
use nooksill_core::layout::Rect;

/// Distance from the card edge into the bar, at 96 DPI. Matches the bar's own
/// padding so the panel starts where the card's outline does.
const CARD_MARGIN: f32 = 8.0;

/// Visible gap between the panel and the card, at 96 DPI.
const PANEL_GAP: f32 = 4.0;

/// Monitor rect (not the work area) of the screen the anchor sits on.
///
/// Clamping must use `rcMonitor`: covering the taskbar is fine, hanging off the
/// screen is not.
#[must_use]
pub fn monitor_of(anchor: (f32, f32, f32, f32)) -> Rect {
    let (al, at, ar, ab) = anchor;
    nooksill_platform::display::monitor_at(
        ((al + ar) * 0.5).round() as i32,
        ((at + ab) * 0.5).round() as i32,
    )
}

/// Top-left corner for a `pw` x `ph` panel hanging off `anchor`.
///
/// `anchor` is the card rect in physical pixels, `mon` the monitor to stay
/// inside. All values and the result are physical pixels.
#[must_use]
pub fn place(
    pw: i32,
    ph: i32,
    anchor: (f32, f32, f32, f32),
    edge: Edge,
    ppp: f32,
    mon: Rect,
) -> (i32, i32) {
    let (al, at, ar, ab) = anchor;
    let ax = (al + ar) * 0.5;
    let ay = (at + ab) * 0.5;
    let margin = (CARD_MARGIN * ppp).round();
    let gap = (PANEL_GAP * ppp).round();
    let half_w = pw as f32 * 0.5;
    let half_h = ph as f32 * 0.5;
    let (x, y) = match edge {
        Edge::Top => (ax - half_w, ab - margin + gap),
        Edge::Left => (ar - margin + gap, ay - half_h),
        Edge::Right => (al - pw as f32 + margin - gap, ay - half_h),
        Edge::Bottom => (ax - half_w, at - ph as f32 + margin - gap),
    };
    let mut xi = x.round() as i32;
    let mut yi = y.round() as i32;
    if xi + pw > mon.right {
        xi = mon.right - pw;
    }
    if yi + ph > mon.bottom {
        yi = mon.bottom - ph;
    }
    if xi < mon.left {
        xi = mon.left;
    }
    if yi < mon.top {
        yi = mon.top;
    }
    (xi, yi)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1920x1080 screen to the right of the primary.
    const MON: Rect = Rect {
        left: 1920,
        top: 0,
        right: 3840,
        bottom: 1080,
    };

    #[test]
    fn bottom_dock_puts_the_panel_above_the_card() {
        let card = (2400.0, 990.0, 2460.0, 1070.0);
        // 990 - 300 + 8 - 4, centred on the card at 2430.
        assert_eq!(place(400, 300, card, Edge::Bottom, 1.0, MON), (2230, 694));
    }

    #[test]
    fn right_dock_pushes_the_panel_inwards() {
        let card = (3800.0, 500.0, 3830.0, 560.0);
        // 3800 - 400 + 8 - 4; vertical centre 530 - 150.
        assert_eq!(place(400, 300, card, Edge::Right, 1.0, MON), (3404, 380));
    }

    /// A wide panel next to a left dock must slide in, not spill onto the
    /// primary monitor: the old code clamped against monitor 0.
    #[test]
    fn left_dock_clamps_inside_the_dock_monitor() {
        let card = (3800.0, 500.0, 3830.0, 560.0);
        let (x, _) = place(600, 300, card, Edge::Left, 1.0, MON);
        assert_eq!(x, MON.right - 600);
        assert!(x >= MON.left);
    }
}
