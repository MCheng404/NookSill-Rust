//! Pure geometry for the three panel placements.
//!
//! All inputs and outputs are physical pixels; the caller owns the Win32 call.

use nooksill_core::config::panel::Placement;
use nooksill_core::config::Edge;
use nooksill_core::layout::{centered_in, Rect};

use super::super::anchor;

/// One placement decision.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Request {
    /// Physical panel size.
    pub size: (i32, i32),
    /// Widget card rect `(l, t, r, b)` in physical pixels, if there is one.
    pub anchor: Option<(f32, f32, f32, f32)>,
    pub edge: Edge,
    pub ppp: f32,
    /// Full monitor rect, used to clamp a followed panel.
    pub monitor: Rect,
    /// Work area, used to centre and to decide whether `Auto` fits.
    pub work: Rect,
}

/// Top-left corner of a panel under `mode`.
#[must_use]
pub fn place(mode: Placement, req: &Request) -> (i32, i32) {
    let (pw, ph) = req.size;
    let centre = || {
        let (x, y) = centered_in(req.work, pw, ph);
        clamp_into(x, y, pw, ph, req.monitor)
    };
    let Some(card) = req.anchor else {
        return centre();
    };
    if mode == Placement::ScreenCenter {
        return centre();
    }
    if mode == Placement::Auto {
        let (x, y) = anchor::ideal(pw, ph, card, req.edge, req.ppp);
        if !fits(x, y, pw, ph, req.work) {
            return centre();
        }
    }
    anchor::place(pw, ph, card, req.edge, req.ppp, req.monitor)
}

/// A panel bigger than the monitor still needs a usable corner.
fn clamp_into(x: i32, y: i32, pw: i32, ph: i32, mon: Rect) -> (i32, i32) {
    let hi_x = mon.right - pw;
    let hi_y = mon.bottom - ph;
    let cx = if hi_x < mon.left {
        mon.left
    } else {
        x.clamp(mon.left, hi_x)
    };
    let cy = if hi_y < mon.top {
        mon.top
    } else {
        y.clamp(mon.top, hi_y)
    };
    (cx, cy)
}

fn fits(x: f32, y: f32, pw: i32, ph: i32, work: Rect) -> bool {
    x >= work.left as f32
        && y >= work.top as f32
        && x + pw as f32 <= work.right as f32
        && y + ph as f32 <= work.bottom as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    const MON: Rect = Rect::new(1920, 0, 3840, 1080);
    const WORK: Rect = Rect::new(1920, 0, 3840, 1040);

    fn req(size: (i32, i32), card: Option<(f32, f32, f32, f32)>) -> Request {
        Request {
            size,
            anchor: card,
            edge: Edge::Bottom,
            ppp: 1.0,
            monitor: MON,
            work: WORK,
        }
    }

    #[test]
    fn screen_center_uses_the_dock_work_area() {
        assert_eq!(
            place(Placement::ScreenCenter, &req((800, 600), None)),
            (2480, 220)
        );
        let card = Some((2400.0, 1000.0, 2460.0, 1060.0));
        assert_eq!(
            place(Placement::ScreenCenter, &req((800, 600), card)),
            (2480, 220),
            "an anchor must not move a centred panel"
        );
    }

    #[test]
    fn follow_widget_places_the_panel_above_the_card() {
        let card = Some((2400.0, 1000.0, 2460.0, 1060.0));
        assert_eq!(
            place(Placement::FollowWidget, &req((400, 300), card)),
            (2230, 704)
        );
    }

    #[test]
    fn auto_follows_when_the_panel_fits() {
        let card = Some((2400.0, 1000.0, 2460.0, 1060.0));
        assert_eq!(place(Placement::Auto, &req((400, 300), card)), (2230, 704));
    }

    #[test]
    fn auto_centres_when_the_ideal_rect_leaves_the_work_area() {
        // 300 - 480 + 8 - 4 = -176: the panel would start above the screen.
        let card = Some((2400.0, 300.0, 2460.0, 360.0));
        assert_eq!(place(Placement::Auto, &req((400, 480), card)), (2680, 280));
        assert_eq!(
            place(Placement::FollowWidget, &req((400, 480), card)),
            (2230, 0)
        );
    }

    #[test]
    fn follow_without_an_anchor_falls_back_to_the_centre() {
        assert_eq!(
            place(Placement::FollowWidget, &req((400, 300), None)),
            (2680, 370)
        );
    }

    #[test]
    fn oversized_panel_does_not_panic_or_leave_the_monitor() {
        let (x, y) = place(
            Placement::Auto,
            &req((5000, 4000), Some((2400.0, 900.0, 2460.0, 960.0))),
        );
        assert_eq!((x, y), (MON.left, MON.top));
        assert_eq!(clamp_into(-50, -50, 100, 100, MON), (MON.left, MON.top));
    }
}
