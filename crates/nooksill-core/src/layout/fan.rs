//! Fan layout for the group panel: children sit on a shallow arc with the
//! middle one highest, or in a plain vertical column when `FanArc` is 0.
//!
//! Pure math: the panel size and every slot come from the same call, so hit
//! testing and painting cannot disagree about where a child is.

/// Cell metrics shared by the grid and the fan, in logical pixels.
#[derive(Clone, Copy, Debug)]
pub struct FanBox {
    pub cell_w: f32,
    pub cell_h: f32,
    pub icon: f32,
    pub pad: f32,
    pub header: f32,
}

/// Panel size and icon rects, origin at the panel top-left.
#[derive(Clone, Debug, PartialEq)]
pub struct FanPanel {
    pub size: (f32, f32),
    /// `(x, y, w, h)` per child, in order.
    pub slots: Vec<(f32, f32, f32, f32)>,
}

/// Narrowest panel either layout may produce.
const MIN_W: f32 = 200.0;
/// Keeps one or two children from collapsing onto a point.
const MIN_RADIUS_FACTOR: f32 = 1.2;

/// Lay `count` children out for the group panel.
///
/// `arc_deg == 0` is the upstream meaning of "vertical": a single column with
/// `gap_px` between icons. Above zero the children follow an arc whose span is
/// capped at `arc_deg`; `gap_px` is the spacing measured along it.
#[must_use]
pub fn panel(count: usize, arc_deg: f32, gap_px: f32, b: &FanBox) -> FanPanel {
    let base_h = b.header + b.pad * 2.0;
    if count == 0 {
        return FanPanel {
            size: (MIN_W, base_h + b.cell_h),
            slots: Vec::new(),
        };
    }

    let gap = gap_px.max(0.0);
    let pitch = b.icon + gap;
    let inset = ((b.cell_h - b.icon) * 0.5).max(0.0);
    let apex = b.header + b.pad;

    // Vertical column: equal x, even y steps.
    if arc_deg <= 0.0 {
        let width = (b.icon + b.pad * 2.0).max(MIN_W);
        let height = base_h + count as f32 * pitch;
        let x = width * 0.5 - b.icon * 0.5;
        let slots = (0..count)
            .map(|i| (x, apex + i as f32 * pitch + inset, b.icon, b.icon))
            .collect();
        return FanPanel {
            size: (width, height),
            slots,
        };
    }

    // Radius chosen so the arc length between the ends holds every pitch; the
    // floor keeps two children apart even with a huge arc.
    let span_max = arc_deg.min(180.0).to_radians();
    // Radius from the chord, not the arc length: arc/radius overshoots by a
    // few percent at wide angles and two children would overlap by a pixel.
    let radius = if count > 1 {
        let step_max = span_max / (count - 1) as f32;
        (pitch / (2.0 * (step_max * 0.5).sin())).max(pitch * MIN_RADIUS_FACTOR)
    } else {
        pitch * MIN_RADIUS_FACTOR
    };
    let step = if count > 1 {
        2.0 * (pitch / (2.0 * radius)).clamp(0.0, 1.0).asin()
    } else {
        0.0
    };
    let span = step * (count - 1) as f32;
    let half = span * 0.5;

    let dx_max = radius * half.sin();
    let sag = radius * (1.0 - half.cos());
    let width = (dx_max * 2.0 + b.icon + b.pad * 2.0).max(MIN_W);
    let height = base_h + b.cell_h + sag;

    let center_x = width * 0.5;
    let mut slots = Vec::with_capacity(count);
    for i in 0..count {
        let angle = -half + step * i as f32;
        let dx = radius * angle.sin();
        // Both ends drop below the apex, mirroring the arc.
        let drop = radius * (1.0 - angle.cos());
        slots.push((
            center_x + dx - b.icon * 0.5,
            apex + drop + inset,
            b.icon,
            b.icon,
        ));
    }
    FanPanel {
        size: (width, height),
        slots,
    }
}

#[cfg(test)]
mod tests {
    use super::{panel, FanBox};

    fn b() -> FanBox {
        FanBox {
            cell_w: 104.0,
            cell_h: 104.0,
            icon: 60.0,
            pad: 16.0,
            header: 40.0,
        }
    }

    #[test]
    fn empty_panel_has_no_slots_but_a_usable_size() {
        let p = panel(0, 40.0, 9.0, &b());
        assert!(p.slots.is_empty());
        assert!(p.size.0 >= 200.0 && p.size.1 > 0.0);
    }

    /// `FanArc = 0` means a vertical column: one x, even steps, no sag.
    #[test]
    fn zero_arc_lays_out_a_vertical_column() {
        let p = panel(4, 0.0, 12.0, &b());
        assert_eq!(p.slots.len(), 4);
        let x0 = p.slots[0].0;
        for (i, (x, y, _, _)) in p.slots.iter().enumerate() {
            assert!((x - x0).abs() < 0.01, "第 {i} 个槽位 x 应相同");
            if i > 0 {
                assert!(*y > p.slots[i - 1].1, "第 {i} 个槽位应更靠下");
            }
        }
        let pitch = 60.0 + 12.0;
        assert!((p.slots[1].1 - p.slots[0].1 - pitch).abs() < 0.01);
        assert!(p.size.1 >= 4.0 * pitch, "高度要能容纳整列");
    }

    /// Every slot must stay inside the panel it produced, and the arc must be
    /// symmetric about its centre: the window is sized from `size`, so a slot
    /// outside it would be clipped and unclickable.
    #[test]
    fn slots_stay_inside_a_symmetric_panel() {
        for count in 1..=9usize {
            let p = panel(count, 40.0, 9.0, &b());
            assert_eq!(p.slots.len(), count);
            let (w, h) = p.size;
            for i in 0..count {
                let (x, y, iw, ih) = p.slots[i];
                assert!(x >= 0.0 && x + iw <= w + 0.01, "count {count} slot {i} x");
                assert!(y >= 0.0 && y + ih <= h + 0.01, "count {count} slot {i} y");
                let mirror = p.slots[count - 1 - i];
                let left = x + iw * 0.5;
                let right = mirror.0 + iw * 0.5;
                assert!(
                    (left + right - w).abs() < 0.01,
                    "count {count} slot {i} not mirrored"
                );
            }
        }
    }

    #[test]
    fn wider_arc_sags_the_ends_more() {
        let flat = panel(5, 20.0, 9.0, &b());
        let wide = panel(5, 100.0, 9.0, &b());
        let flat_drop = flat.slots[0].1 - flat.slots[2].1;
        let wide_drop = wide.slots[0].1 - wide.slots[2].1;
        assert!(wide_drop > flat_drop, "弧角越大，两端下沉越明显");
        assert!(flat_drop < 20.0, "20 度应当是浅弧");
    }

    #[test]
    fn stacking_never_overlaps_neighbours() {
        for count in 2..=9usize {
            for gap in [0.0_f32, 4.0, 9.0, 20.0] {
                let p = panel(count, 40.0, gap, &b());
                for i in 1..count {
                    let (px, py, _, _) = p.slots[i - 1];
                    let (cx, cy, _, _) = p.slots[i];
                    let dist = ((cx - px).powi(2) + (cy - py).powi(2)).sqrt();
                    assert!(dist >= 60.0 - 0.01, "count {count} gap {gap} slot {i} 重叠");
                }
            }
        }
    }

    #[test]
    fn more_children_widen_the_panel() {
        let two = panel(2, 40.0, 9.0, &b()).size.0;
        let six = panel(6, 40.0, 9.0, &b()).size.0;
        assert!(six > two);
    }
}
