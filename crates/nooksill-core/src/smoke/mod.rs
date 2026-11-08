//! Ambient smoke that rises outside the bar, curls and parts around the cursor.
//!
//! Time is passed in by the caller so frames stay reproducible, and the effect
//! only runs on the bottom edge: buoyancy and the upper falloff assume it.

use crate::config::{Config, Key};

/// Noise table edge length; 192 is divisible by the periods of the 4/3/6 base
/// frequencies, so tiles are seamless.
pub const NOISE_N: usize = 192;

/// Simulation cell edge in physical pixels; 5 px balances speed and precision.
pub const CELL: f32 = 5.0;

/// Output pixels per cell: the bitmap is `CELL / RES` times the grid, smoothed
/// by bilinear upscaling.
pub const RES: usize = 2;

/// Maximum time step in seconds; longer gaps (window drag, breakpoint) are cut,
/// otherwise explicit integration blows up.
pub const MAX_DT: f32 = 0.05;

/// Color with 0..=255 sRGB components.
pub type Rgb = [f32; 3];

/// Eight presets indexed by `SmokeColor` 0..=7.
///
/// The array order is config data: reordering changes existing ini files.
pub const PALETTE: [Rgb; 8] = [
    [88.0, 52.0, 160.0],
    [168.0, 84.0, 22.0],
    [160.0, 30.0, 36.0],
    [200.0, 60.0, 120.0],
    [34.0, 96.0, 150.0],
    [70.0, 140.0, 185.0],
    [34.0, 128.0, 78.0],
    [150.0, 108.0, 10.0],
];

/// Accent color slot (`SmokeColor = 8`).
pub const SLOT_ACCENT: i32 = 8;
/// Rainbow slot (`SmokeColor = 9`).
pub const SLOT_RAINBOW: i32 = 9;

/// Opacity steps: 17%, 30%, 50%, 70%, 100%, 125%, 150%.
///
/// The last two need the alpha gain of [`Fluid::post`]: premultiplied components
/// cannot exceed alpha, so scaling color alone only greys the result.
const OPACITY: [f32; 7] = [0.17, 0.30, 0.50, 0.70, 1.0, 1.25, 1.5];
const DENSITY: [f32; 3] = [0.6, 1.0, 1.5];
const SPEED: [f32; 3] = [0.6, 1.0, 1.6];
const THICKNESS: [f32; 4] = [0.7, 1.0, 1.4, 1.9];
const MOUSE: [f32; 3] = [0.0, 1.0, 2.0];
const REACH: [f32; 3] = [0.7, 1.0, 1.4];

/// Simulation parameters resolved from config; no ini indices are left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    pub enabled: bool,
    /// Global opacity; above 1.0 brightens, see [`Fluid::post`].
    pub opacity: f32,
    /// Smoke density.
    pub density: f32,
    /// Time scale.
    pub speed: f32,
    /// Halo thickness; also sets the outer margins.
    pub thickness: f32,
    /// Cursor push strength; 0 disables it.
    pub mouse: f32,
    /// Cursor influence radius factor.
    pub reach: f32,
    /// Light highlight.
    pub glow: bool,
    /// Primary color slot: 0..=7 preset, 8 accent, 9 rainbow.
    pub color: i32,
    /// Secondary color slot, blended left to right along the bar; `None` = off.
    pub color2: Option<i32>,
    /// Concrete accent color, queried from DWM by the caller.
    pub accent: Rgb,
    /// Whether to emit rising puffs.
    pub clouds: bool,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            enabled: false,
            opacity: OPACITY[3],
            density: DENSITY[1],
            speed: SPEED[1],
            thickness: THICKNESS[1],
            mouse: MOUSE[1],
            reach: REACH[1],
            glow: false,
            color: 0,
            color2: None,
            accent: PALETTE[0],
            clouds: true,
        }
    }
}

impl Tuning {
    /// Resolves the parameters from config; `accent` comes from the platform layer.
    ///
    /// `enabled` is false unless the dock sits on the bottom edge: buoyancy and
    /// the slow upper falloff assume it.
    #[must_use]
    pub fn from_config(cfg: &Config, accent: Rgb) -> Self {
        let idx = |key: Key, table_len: usize| -> usize {
            (cfg.get_i32(key).max(0) as usize).min(table_len - 1)
        };
        let on = cfg.get_i32(Key::SMOKE) != 0 && cfg.edge == crate::config::Edge::Bottom;
        let c2 = cfg.get_i32(Key::SMOKE_COLOR2);
        Self {
            enabled: on,
            opacity: OPACITY[idx(Key::SMOKE_OPACITY, OPACITY.len())],
            density: DENSITY[idx(Key::SMOKE_DENSITY, DENSITY.len())],
            speed: SPEED[idx(Key::SMOKE_SPEED, SPEED.len())],
            thickness: THICKNESS[idx(Key::SMOKE_THICK, THICKNESS.len())],
            mouse: MOUSE[idx(Key::SMOKE_MOUSE, MOUSE.len())],
            reach: REACH[idx(Key::SMOKE_REACH, REACH.len())],
            glow: cfg.get_i32(Key::SMOKE_TONE) != 0,
            color: cfg.get_i32(Key::SMOKE_COLOR),
            color2: (c2 >= 0).then_some(c2),
            accent,
            clouds: cfg.get_i32(Key::SMOKE_CLOUDS) != 0,
        }
    }

    /// Horizontal outer margin in pixels.
    #[must_use]
    pub fn margin_x(&self) -> f32 {
        30.0 * self.thickness
    }

    /// Vertical outer margin in pixels.
    #[must_use]
    pub fn margin_y(&self) -> f32 {
        34.0 * self.thickness
    }

    /// Per-step decay at the upper edge: thicker smoke and lower speed travel further.
    #[must_use]
    pub fn decay_top(&self) -> f32 {
        0.982_f32.powf(self.speed / self.thickness)
    }

    /// Per-step decay at the sides and below.
    #[must_use]
    pub fn decay_side(&self) -> f32 {
        0.94_f32.powf(self.speed / self.thickness)
    }

    /// Color of a slot at wall-clock seconds `wall`, with tone applied.
    ///
    /// The rainbow uses unscaled time for its 90 s turn, so `speed` does not also
    /// change the hue rate.
    #[must_use]
    pub fn color_at(&self, slot: i32, wall: f32) -> Rgb {
        let mut c = match slot {
            0..=7 => PALETTE[slot as usize],
            SLOT_ACCENT => self.accent,
            _ => rainbow(wall),
        };
        if self.glow {
            // Brightens the same hue instead of shifting toward white.
            for v in &mut c {
                *v = (*v * 1.35 + 55.0).min(255.0);
            }
        }
        c
    }

    /// Primary and secondary color.
    #[must_use]
    pub fn colors(&self, wall: f32) -> (Rgb, Option<Rgb>) {
        (
            self.color_at(self.color, wall),
            self.color2.map(|s| self.color_at(s, wall)),
        )
    }
}

/// Slow rainbow: one turn per 90 s, saturation 0.7, value 170.
#[must_use]
pub fn rainbow(t: f32) -> Rgb {
    const V: f32 = 170.0;
    const SAT: f32 = 0.7;
    let h = (t / 90.0).rem_euclid(1.0) * 6.0;
    let k = h as usize;
    let f = h - k as f32;
    let pq = V * (1.0 - SAT);
    let qq = V * (1.0 - SAT * f);
    let tt = V * (1.0 - SAT * (1.0 - f));
    match k % 6 {
        0 => [V, tt, pq],
        1 => [qq, V, pq],
        2 => [pq, V, tt],
        3 => [pq, qq, V],
        4 => [tt, pq, V],
        _ => [V, pq, qq],
    }
}

mod fluid;
mod noise;
mod raster;

pub use fluid::{Fluid, Frame};
pub use noise::Noise;

#[cfg(test)]
mod tests;
