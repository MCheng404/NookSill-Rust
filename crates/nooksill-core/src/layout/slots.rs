//! Slot layout: base centers, magnification targets, actual slots, insert gap.

use super::metrics::span;
use super::*;

/// Per-frame convergence factor of the zoom animation, calibrated at 60 Hz.
pub const SIZE_LERP: f32 = 0.38;
/// Below this distance the animation snaps to the target.
pub const SNAP: f32 = 0.3;
/// Magnification radius, in multiples of `s + gap`.
pub const MAG_RADIUS: f32 = 2.6;

/// Slot kind. Separators take their width from `SepGap`, not from the icon.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SlotKind {
    Icon,
    Separator,
}

impl SlotKind {
    #[must_use]
    pub const fn is_separator(self) -> bool {
        matches!(self, Self::Separator)
    }

    /// Width of this slot when not magnified.
    #[must_use]
    pub const fn base_width(self, m: &Metrics) -> f32 {
        if self.is_separator() {
            m.sep_w
        } else {
            m.s
        }
    }
}

/// Vertical baseline shared by all four edges. Horizontal flips are `map_rect`'s job.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Bars {
    /// Bar top edge.
    pub top: f32,
    /// Bar bottom edge.
    pub bottom: f32,
    /// Icon bottom edge; icons grow upward from here.
    pub icon_bottom: f32,
}

/// Derives the vertical baseline from metrics.
#[must_use]
pub fn bars(m: &Metrics) -> Bars {
    let bottom = m.lh as f32 - m.margin - 2.0;
    Bars {
        top: bottom - m.bar_h,
        bottom,
        icon_bottom: bottom - m.pad - 2.0,
    }
}

/// Unmagnified bar span.
#[must_use]
pub fn base_width(m: &Metrics, kinds: &[SlotKind]) -> f32 {
    let n_sep = kinds.iter().filter(|k| k.is_separator()).count();
    span(m.pad_x, m.s, m.gap, m.sep_w, kinds.len() - n_sep, n_sep)
}

/// Unmagnified slot centers, used as magnification weights.
///
/// `widget_extra` shifts the whole bar left by half its width.
#[must_use]
pub fn base_centers(m: &Metrics, kinds: &[SlotKind], widget_extra: f32) -> Vec<f32> {
    let left = (m.lw as f32 - base_width(m, kinds) - widget_extra) / 2.0;
    let mut x = left + m.pad_x;
    let mut out = Vec::with_capacity(kinds.len());
    for &kind in kinds {
        let w = kind.base_width(m);
        out.push(x + w / 2.0);
        x += w + m.gap;
    }
    out
}

/// Interaction state for one frame.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct View {
    /// Cursor position in the same physical-pixel space as `Metrics`; negative x means outside.
    pub mouse: (f32, f32),
    /// Whether the cursor is inside the window.
    pub hover: bool,
    /// Hide progress: 0 = fully visible, 1 = fully hidden.
    pub hide_cur: f32,
}

/// Target edge length of every slot for this frame.
///
/// Weights use base centers: actual centers move as icons grow, so using them
/// would make the hovered icon drift with the zoom level.
#[must_use]
pub fn magnify_targets(
    m: &Metrics,
    kinds: &[SlotKind],
    base_c: &[f32],
    prev_act_c: &[f32],
    view: View,
    zoom: bool,
) -> Vec<f32> {
    let mag = zoom && view.hover && !kinds.is_empty() && view.mouse.0 >= 0.0 && view.hide_cur < 0.5;
    if !mag {
        return kinds.iter().map(|k| k.base_width(m)).collect();
    }
    let u = map_actual_to_base(view.mouse.0, base_c, prev_act_c);
    let r = (m.s + m.gap) * MAG_RADIUS;
    kinds
        .iter()
        .enumerate()
        .map(|(i, k)| {
            if k.is_separator() {
                m.sep_w
            } else {
                let w = magnify_weight(base_c.get(i).copied().unwrap_or(0.0) - u, r);
                m.s + (m.z - m.s) * w
            }
        })
        .collect()
}

/// Reference frame interval at 60 Hz; the lerp factors are calibrated for it.
pub const REF_FRAME_MS: f32 = 1000.0 / 60.0;

/// Steps `cur` toward `target`; returns true while it is still moving.
///
/// `dt_ms` is the real frame interval, so speed does not follow the refresh
/// rate. It is capped at 4 frames so a stall cannot finish the animation at once.
pub fn lerp_to(cur: &mut f32, target: f32, dt_ms: f32) -> bool {
    let d = target - *cur;
    if d.abs() > SNAP {
        let frames = (dt_ms / REF_FRAME_MS).clamp(0.0, 4.0);
        let k = 1.0 - (1.0 - SIZE_LERP).powf(frames);
        *cur += d * k;
        true
    } else {
        *cur = target;
        false
    }
}

/// Actual slots after magnification. Physical pixels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slots {
    /// Bar left edge.
    pub bar_l: f32,
    /// Bar width.
    pub bar_w: f32,
    /// Left edge of every slot.
    pub slot_l: Vec<f32>,
    /// Right edge of every slot.
    pub slot_r: Vec<f32>,
    /// Actual center of every slot.
    pub act_c: Vec<f32>,
    /// Left edge of the gap reserved for an external drag; `None` when none.
    pub insert_l: Option<f32>,
}

/// Computes actual slots from the current edge lengths, in item order.
#[must_use]
pub fn place_slots(m: &Metrics, cur: &[f32], widget_extra: f32) -> Slots {
    place_slots_insert(m, cur, widget_extra, None)
}

/// Same as [`place_slots`], with an optional gap of `width` before item `index`.
///
/// The gap adds its width plus one `gap` to the bar, shifting everything to its
/// right; `index == cur.len()` places it at the end.
#[must_use]
pub fn place_slots_insert(
    m: &Metrics,
    cur: &[f32],
    widget_extra: f32,
    insert: Option<(usize, f32)>,
) -> Slots {
    let n = cur.len();
    let extra = insert.map_or(0.0, |(_, w)| w + m.gap);
    let total = if n == 0 {
        2.0 * m.pad_x + m.s * 4.0 + extra
    } else {
        2.0 * m.pad_x + cur.iter().sum::<f32>() + m.gap * (n - 1) as f32 + extra
    };
    // Available width must stay >= 0: a strip wider than the window makes
    // `lw - 4 - widget_extra` negative, and a negative `bar_w` panics in the
    // gradient radius clamp.
    let avail = (m.lw as f32 - 4.0 - widget_extra).max(0.0);
    let bar_w = total.min(avail);
    let bar_l = (m.lw as f32 - bar_w - widget_extra) / 2.0;

    let mut x = bar_l + m.pad_x;
    let mut slot_l = Vec::with_capacity(n);
    let mut slot_r = Vec::with_capacity(n);
    let mut act_c = Vec::with_capacity(n);
    for (i, &c) in cur.iter().enumerate() {
        if insert.is_some_and(|(at, _)| at == i) {
            x += extra;
        }
        slot_l.push(x);
        slot_r.push(x + c);
        act_c.push(x + c / 2.0);
        x += c + m.gap;
    }
    // A gap at the end is not visited by the loop; only its position is needed
    // for the drop marker.
    let insert_l = insert.map(|(at, _)| {
        let at = at.min(n);
        if at < n {
            // `slot_l[at]` already shifted right; subtract the whole gap.
            slot_l[at] - extra
        } else {
            slot_r.last().copied().unwrap_or(x - m.gap) + m.gap
        }
    });
    Slots {
        bar_l,
        bar_w,
        slot_l,
        slot_r,
        act_c,
        insert_l,
    }
}

/// New sizes while dragging a separator (`SepScale`).
///
/// `dy` is the cursor shift since press in physical pixels, negative upward.
/// Returns `(icon, zoom)`: icon clamped to 24..=128, zoom scaled by the initial
/// ratio and clamped to 32..=220; config clamps both again on write.
///
/// The ratio uses the drag-start values, so dragging back returns to the
/// original size without rounding drift.
#[must_use]
pub fn sep_scale(dy: f32, icon0: i32, zoom0: i32) -> (i32, i32) {
    // Truncate, not round: matches the upstream cast and avoids +-1 jitter on
    // slow drags.
    let delta = (dy * 0.6) as i32;
    let icon = (icon0 - delta).clamp(24, 128);
    let base = icon0.max(1) as f32;
    let zoom = (zoom0 as f32 * icon as f32 / base).round() as i32;
    (icon, zoom.clamp(32, 220))
}
