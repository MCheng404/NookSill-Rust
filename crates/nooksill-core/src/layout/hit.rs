//! Hit testing: near-dock tests, hit index, actual-to-base mapping.

use super::*;

/// Returns true when the cursor counts as near the dock.
///
/// `hot` is the whole band (icons plus widgets) in screen physical pixels and
/// `inner` how far it reaches inward; `keep` only widens the tolerance.
#[must_use]
pub fn near_dock(
    edge: Edge,
    cursor: (i32, i32),
    hot: (f32, f32, f32, f32),
    work: Rect,
    inner: f32,
    keep: bool,
) -> bool {
    /// Cursor tolerance around the band, physical pixels.
    const SLACK: f32 = 8.0;
    /// Extra tolerance while expanded, so collapsing needs a real move away.
    const HYSTERESIS: f32 = 32.0;
    /// Tolerance for the screen-edge probe.
    const EDGE_REVEAL: f32 = 2.0;

    let slack = if keep { SLACK + HYSTERESIS } else { SLACK };
    let (x, y, w, h) = hot;
    // Extend toward the screen interior: up for bottom, down for top, and so on.
    let (x, y, w, h) = match edge {
        Edge::Bottom => (x, y - inner, w, h + inner),
        Edge::Top => (x, y, w, h + inner),
        Edge::Left => (x, y, w + inner, h),
        Edge::Right => (x - inner, y, w + inner, h),
    };
    let (cx, cy) = (cursor.0 as f32, cursor.1 as f32);

    // Inside the band plus slack.
    if cx >= x - slack && cx <= x + w + slack && cy >= y - slack && cy <= y + h + slack {
        return true;
    }

    // Touching the work-area edge within the band span: the collapsed probe.
    let (along, at_edge) = match edge {
        Edge::Bottom => (
            cx >= x && cx <= x + w,
            cy >= work.bottom as f32 - EDGE_REVEAL,
        ),
        Edge::Top => (cx >= x && cx <= x + w, cy <= work.top as f32 + EDGE_REVEAL),
        Edge::Left => (cy >= y && cy <= y + h, cx <= work.left as f32 + EDGE_REVEAL),
        Edge::Right => (
            cy >= y && cy <= y + h,
            cx >= work.right as f32 - EDGE_REVEAL,
        ),
    };
    along && at_edge
}
/// Magnification weight: 1.0 at the center, 0.0 beyond `radius`, cosine between.
///
/// Cosine instead of a linear ramp: a linear falloff has a kink at the edge.
#[must_use]
pub fn magnify_weight(distance: f32, radius: f32) -> f32 {
    if radius <= 0.0 || distance.abs() >= radius {
        return 0.0;
    }
    let t = std::f32::consts::PI * distance.abs() / radius;
    0.5 * (1.0 + t.cos())
}

/// Hit test on the horizontal axis; returns the item index.
///
/// Widens slot ranges by half a gap instead of testing icon rects, which jitter
/// while zooming. The first match from the left wins, since neighbors overlap.
#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "geometry bundle maps 1:1 to upstream"
)]
pub fn hit_index(
    u: f32,
    v: f32,
    slot_l: &[f32],
    slot_r: &[f32],
    visible: &[bool],
    gap: f32,
    bar_top: f32,
    zoom: f32,
    lh: f32,
) -> Option<usize> {
    // Vertical range first: the zoom overflow area and below the window miss.
    if v < bar_top - zoom - 16.0 || v > lh {
        return None;
    }
    let half = gap / 2.0;
    let n = slot_l.len().min(slot_r.len());
    for i in 0..n {
        // Icons with `cur <= 1` are not drawn, so they are not hittable.
        if !visible.get(i).copied().unwrap_or(false) {
            continue;
        }
        if u >= slot_l[i] - half && u < slot_r[i] + half {
            return Some(i);
        }
    }
    None
}

/// Maps a cursor position from the magnified slots back to the base layout.
///
/// Weights must be computed on base centers: actual centers move while icons
/// grow, so using them makes the hovered item drift with the zoom level.
#[must_use]
pub fn map_actual_to_base(mx: f32, base_c: &[f32], act_c: &[f32]) -> f32 {
    // Mismatched lengths mean the item list changed; skip the mapping.
    if base_c.len() != act_c.len() {
        return mx;
    }
    let n = base_c.len();
    if n == 0 {
        return mx;
    }
    if n == 1 {
        return base_c[0] + (mx - act_c[0]);
    }
    // Outside both ends: shift by the nearest endpoint.
    if mx <= act_c[0] {
        return base_c[0] + (mx - act_c[0]);
    }
    if mx >= act_c[n - 1] {
        return base_c[n - 1] + (mx - act_c[n - 1]);
    }
    // Between two actual centers: interpolate onto the base interval.
    for i in 0..n - 1 {
        if mx >= act_c[i] && mx < act_c[i + 1] {
            let d = act_c[i + 1] - act_c[i];
            let t = if d > 0.001 { (mx - act_c[i]) / d } else { 0.0 };
            return base_c[i] + t * (base_c[i + 1] - base_c[i]);
        }
    }
    mx
}

/// Hit test for one frame; `None` while hiding or when not hovered.
///
/// Uses this frame's slots, otherwise bubbles trail the icons by one frame.
#[must_use]
pub fn hit_test(m: &Metrics, bars: &Bars, slots: &Slots, cur: &[f32], view: View) -> Option<usize> {
    if !view.hover || view.hide_cur >= 0.5 {
        return None;
    }
    // Icons that are not drawn cannot be hit.
    let visible: Vec<bool> = cur.iter().map(|c| *c > 1.0).collect();
    hit_index(
        view.mouse.0,
        view.mouse.1,
        &slots.slot_l,
        &slots.slot_r,
        &visible,
        m.gap,
        bars.top,
        m.z,
        m.lh as f32,
    )
}
