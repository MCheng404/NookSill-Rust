//! Layout math: metrics, four-edge coordinate mapping, magnification, hit testing.
//!
//! Pure functions over plain values: the same input always yields the same
//! output, so coordinates are asserted in tests without creating a window.

use crate::config::{Config, Edge, Key};
use crate::units;

mod fan;
mod hit;
mod metrics;
mod slots;
mod window;

pub use fan::{panel as fan_panel, FanBox, FanPanel};
pub use hit::{hit_index, hit_test, magnify_weight, map_actual_to_base, near_dock};
pub use metrics::{calc_metrics, clampf, clock_width, Metrics, WidgetKind, WidgetStrip};
pub use slots::{
    bars, base_centers, base_width, lerp_to, magnify_targets, place_slots, place_slots_insert,
    sep_scale, Bars, SlotKind, Slots, View, MAG_RADIUS, REF_FRAME_MS, SIZE_LERP, SNAP,
};
pub use window::{
    bounce_active, bounce_offset, centered_in, map_rect, physical_size, reserve_thickness,
    to_logical, window_origin, BOUNCE_MS,
};

/// Integer rectangle with Win32 `RECT` convention: right/bottom are exclusive.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    #[must_use]
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    #[must_use]
    pub const fn width(self) -> i32 {
        self.right - self.left
    }

    #[must_use]
    pub const fn height(self) -> i32 {
        self.bottom - self.top
    }
}

#[cfg(test)]
mod tests;
