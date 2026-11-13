//! Per-frame metrics: bar size, icon and spacing scaling, widget strip width.

use super::*;

/// Empirical geometry budget: zoom overflow, label bubbles and side reserve.
const MIN_FIT: f32 = 0.25;
const ZOOM_OVERFLOW_SLOTS: f32 = 6.0;
const LABEL_BUBBLE_SCALE: f32 = 0.55;
const LABEL_BUBBLE_PAD: f32 = 34.0;
const SHADOW_PAD: f32 = 8.0;
const VERTICAL_LABEL_ROOM: f32 = 170.0;
const SIDE_RESERVE: f32 = 260.0;

/// Widget kind.
///
/// The declared order is config data: `WideMask` uses the index as a bit
/// position, so reordering changes what existing ini files mean.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WidgetKind {
    /// Clock card.
    Clock,
    /// Notification list.
    Notify,
    /// Control center tiles.
    CtrlCenter,
    /// Resource monitor.
    Resource,
    /// Timer / to-do list.
    Tool,
    /// Focus mode.
    Focus,
    /// System tray; opens the Windows notification area.
    Tray,
}

impl WidgetKind {
    /// Index used as the `WideMask` bit position.
    #[must_use]
    pub const fn index(self) -> u32 {
        match self {
            Self::Clock => 0,
            Self::Notify => 1,
            Self::CtrlCenter => 2,
            Self::Resource => 3,
            Self::Tool => 4,
            Self::Focus => 5,
            Self::Tray => 6,
        }
    }

    /// Returns true for widgets that have a renderer.
    ///
    /// Unimplemented widgets stay out of the strip even when enabled: the space
    /// would be reserved and stay blank.
    #[must_use]
    pub const fn is_implemented(self) -> bool {
        matches!(
            self,
            Self::Clock | Self::Resource | Self::Tool | Self::CtrlCenter | Self::Tray
        )
    }

    /// Returns true when the widget is enabled in config.
    #[must_use]
    pub fn is_enabled(self, cfg: &Config) -> bool {
        match self {
            Self::Clock => cfg.clock,
            Self::Notify => cfg.get_i32(Key::NOTIFY_W) != 0,
            Self::CtrlCenter => cfg.get_i32(Key::CTRL_CENTER) != 0,
            Self::Resource => cfg.get_i32(Key::RES_MON) != 0,
            Self::Tool => cfg.get_i32(Key::TOOL_W) != 0,
            Self::Focus => cfg.get_i32(Key::MODE_W) != 0,
            Self::Tray => cfg.get_i32(Key::TRAY_W) != 0,
        }
    }

    /// Returns true when the matching `WideMask` bit is set.
    #[must_use]
    pub fn is_wide(self, cfg: &Config) -> bool {
        cfg.get_i32(Key::WIDE_MASK) & (1 << self.index()) != 0
    }

    /// Card width in physical pixels, as a fraction of the bar height.
    ///
    /// The clock is not covered: its width follows the rendered text, see
    /// [`clock_width`].
    #[must_use]
    pub fn width(self, cfg: &Config, bar_h: f32) -> i32 {
        match self {
            Self::Clock => 0, // computed by clock_width
            Self::Notify => (bar_h * 0.92) as i32,
            Self::CtrlCenter => (bar_h * if self.is_wide(cfg) { 1.6 } else { 0.72 }) as i32,
            Self::Resource => (bar_h * if self.is_wide(cfg) { 2.2 } else { 1.16 }) as i32,
            Self::Tool => (bar_h * if self.is_wide(cfg) { 1.6 } else { 0.72 }) as i32,
            Self::Focus => (bar_h * 0.72) as i32,
            // Fixed width, no double-wide variant.
            Self::Tray => (bar_h * 0.72) as i32,
        }
    }
}

/// Row of cards attached to the outside of the bar.
///
/// With the clock as its only member it must match the pre-strip layout pixel
/// for pixel.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct WidgetStrip {
    /// Cards in strip order; `WOrder` only swaps clock and notify.
    pub items: [Option<(WidgetKind, i32)>; 7],
    /// Number of occupied entries in `items`.
    pub len: usize,
    /// Space the strip reserves: member widths plus gaps.
    pub extra: f32,
    /// Side of the bar: right (false) or left (true).
    pub on_left: bool,
    /// Extra gap between cards, added on top of `gap`.
    ///
    /// `build` and `left_of` must use the same value, otherwise the reserved
    /// space and the drawn positions drift apart.
    pub gap_extra: f32,
}

impl WidgetStrip {
    /// Builds the strip, accumulating widths and gaps.
    ///
    /// Order is clock/notify (swapped by `WOrder`), control center, resources,
    /// tool, focus, tray. `clock_w` comes from [`clock_width`], not `width()`.
    #[must_use]
    pub fn build(cfg: &Config, dpi: i32, bar_h: f32, gap: f32, clock_w: i32) -> Self {
        // Only the first two swap; the tray is always last.
        let order = if cfg.get_i32(Key::W_ORDER) != 0 {
            [
                WidgetKind::Notify,
                WidgetKind::Clock,
                WidgetKind::CtrlCenter,
                WidgetKind::Resource,
                WidgetKind::Tool,
                WidgetKind::Focus,
                WidgetKind::Tray,
            ]
        } else {
            [
                WidgetKind::Clock,
                WidgetKind::Notify,
                WidgetKind::CtrlCenter,
                WidgetKind::Resource,
                WidgetKind::Tool,
                WidgetKind::Focus,
                WidgetKind::Tray,
            ]
        };

        // `WidgetGap` is logical pixels in config, physical here.
        let gap_extra = units::scale(cfg.get_i32(Key::WIDGET_GAP).clamp(0, 24), dpi) as f32;
        let mut items = [None; 7];
        let mut len = 0usize;
        let mut extra = 0.0f32;
        for kind in order {
            if !kind.is_enabled(cfg) || !kind.is_implemented() {
                continue;
            }
            let w = if kind == WidgetKind::Clock {
                clock_w
            } else {
                kind.width(cfg, bar_h)
            };
            items[len] = Some((kind, w));
            len += 1;
            extra += w as f32 + gap + gap_extra;
        }
        Self {
            items,
            len,
            extra,
            on_left: cfg.get_i32(Key::W_SIDE) != 0,
            gap_extra,
        }
    }

    /// Width of a widget inside the strip; `None` when absent.
    #[must_use]
    pub fn width_of(&self, kind: WidgetKind) -> Option<i32> {
        self.items[..self.len]
            .iter()
            .flatten()
            .find(|(k, _)| *k == kind)
            .map(|(_, w)| *w)
    }

    /// Iterates the strip members in order.
    pub fn iter(&self) -> impl Iterator<Item = (WidgetKind, i32)> + '_ {
        self.items[..self.len].iter().flatten().copied()
    }

    /// Left edge of strip member `i`, in the same units as `bar_l` / `bar_w`.
    ///
    /// Right side: outward from the bar's right edge. Left side: outward from
    /// the left edge, walking members in reverse.
    #[must_use]
    pub fn left_of(&self, i: usize, bar_l: f32, bar_w: f32, gap: f32) -> Option<f32> {
        self.items.get(i)?.as_ref()?;
        let gap_w = gap + self.gap_extra;
        if self.on_left {
            // Lay out right-to-left from the bar's left edge.
            let mut u = bar_l - gap_w;
            for (j, slot) in self.items.iter().take(self.len).enumerate().rev() {
                // `build` fills the first `len` slots but the fields are public;
                // a hole must resolve to `None`, not panic the UI.
                let (_, wj) = slot.as_ref()?;
                u -= *wj as f32;
                if j == i {
                    return Some(u);
                }
                u -= gap_w;
            }
            None
        } else {
            // Lay out left-to-right from the bar's right edge.
            let mut u = bar_l + bar_w + gap_w;
            for (j, slot) in self.items.iter().take(self.len).enumerate() {
                if j == i {
                    return Some(u);
                }
                let (_, wj) = slot.as_ref()?;
                u += *wj as f32 + gap_w;
            }
            None
        }
    }
}

/// Clock card width in physical pixels; 0 when the clock is disabled.
///
/// Width follows `ClockShow` (0 = both, 1 = date only, 2 = time only),
/// `DateFmt` (0 = month/day, 1 = short) and whether week/seconds are shown.
#[must_use]
pub fn clock_width(cfg: &Config, dpi: i32) -> i32 {
    if !cfg.clock {
        return 0;
    }
    let has_time = cfg.get_i32(Key::CLOCK_SHOW) != 2;
    let has_date = cfg.get_i32(Key::CLOCK_SHOW) != 1;
    if cfg.edge.is_horizontal() {
        let wt = if has_time {
            units::scale(
                if cfg.get_i32(Key::CLOCK_SEC) != 0 {
                    92
                } else {
                    74
                },
                dpi,
            )
        } else {
            0
        };
        let wd = if has_date {
            let base = if cfg.get_i32(Key::DATE_FMT) == 0 {
                if cfg.get_i32(Key::CLOCK_WEEK) != 0 {
                    166
                } else {
                    124
                }
            } else if cfg.get_i32(Key::CLOCK_WEEK) != 0 {
                146
            } else {
                110
            };
            units::scale(base, dpi)
        } else {
            0
        };
        wt.max(wd)
    } else {
        let lines = i32::from(has_time)
            + if has_date {
                if cfg.get_i32(Key::CLOCK_WEEK) != 0 {
                    2
                } else {
                    1
                }
            } else {
                0
            };
        units::scale(lines * 21 + 14, dpi)
    }
}

/// One frame of geometry. Every field is in physical pixels.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Metrics {
    /// Base icon edge length.
    pub s: f32,
    /// Zoomed icon edge length.
    pub z: f32,
    /// Gap between adjacent slots.
    pub gap: f32,
    /// Horizontal padding at both bar ends, from `HPad` (default 12).
    pub pad_x: f32,
    /// Separator slot width.
    pub sep_w: f32,
    /// Vertical bar padding.
    pub pad: f32,
    /// Bar height = `s + 2*pad + 4`.
    pub bar_h: f32,
    /// Space above the bar for zoom overflow and label bubbles.
    pub overhang: f32,
    /// Distance from the screen edge to the bar, including the reflection.
    pub margin: f32,
    /// Widget strip attached to the bar.
    pub widgets: WidgetStrip,
    /// Width the bar yields to the strip.
    pub widget_extra: f32,
    /// Clock card width, duplicated from the strip after scaling.
    pub clock_w: i32,
    /// Reflection height; 0 when disabled or not on the bottom edge.
    pub refl_h: f32,
    /// Logical width; the window width for horizontal edges.
    pub lw: i32,
    /// Logical height; the window height for horizontal edges.
    pub lh: i32,
}

/// Clamps `v` into `lo..=hi`.
#[must_use]
pub fn clampf(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// Base horizontal span: `2*pad_x + n_icon*s + n_sep*sep_w + gap*(n-1)`.
///
/// Single definition of the bar geometry, shared by metrics, base centers and
/// slots. An empty dock keeps 4 slots of width so the drop hint still fits.
pub(super) fn span(pad_x: f32, s: f32, gap: f32, sep_w: f32, n_icon: usize, n_sep: usize) -> f32 {
    let n = n_icon + n_sep;
    if n == 0 {
        return 2.0 * pad_x + s * 4.0;
    }
    let mut w = 2.0 * pad_x + n_icon as f32 * s + n_sep as f32 * sep_w;
    w += gap * (n - 1) as f32;
    w
}

/// Computes one frame of metrics.
///
/// `n_items` counts every entry including separators, `n_sep` only the
/// separators; `work` is the target monitor's work area without the taskbar.
#[must_use]
pub fn calc_metrics(cfg: &Config, dpi: i32, n_items: usize, n_sep: usize, work: Rect) -> Metrics {
    // Config lengths are 96 DPI logical pixels; this is the only conversion
    // point, everything downstream is physical pixels.
    let sc = |v: i32| crate::units::scale(v, dpi) as f32;
    let s0 = sc(cfg.icon_size);
    // Zoom must not go below the base size, or `overhang` turns negative.
    let z0 = if cfg.zoom {
        sc(cfg.zoom_size.max(cfg.icon_size))
    } else {
        s0
    };
    let g0 = sc(cfg.spacing);
    let pad0 = sc(cfg.pad);
    let sep0 = sc(cfg.get_i32(Key::SEP_GAP));
    let pad_x0 = sc(cfg.get_i32(Key::HPAD).max(0));

    let n_icon = n_items.saturating_sub(n_sep);
    let wb = span(pad_x0, s0, g0, sep0, n_icon, n_sep);

    let along = if cfg.edge.is_horizontal() {
        work.width() as f32
    } else {
        work.height() as f32
    };
    let limit = along - 40.0;
    let mut fit = 1.0_f32;
    if wb > limit && (wb - 2.0 * pad_x0).abs() > f32::EPSILON {
        // Too many icons for the screen: shrink everything, floor at 25%.
        fit = clampf((limit - 2.0 * pad_x0) / (wb - 2.0 * pad_x0), MIN_FIT, 1.0);
    }

    let s = s0 * fit;
    let z = z0 * fit;
    let gap = g0 * fit;
    let sep_w = sep0 * fit;
    let pad = pad0;
    let pad_x = pad_x0;

    // `ReflSize` is a percentage of the icon edge, `ReflGap` adds spacing.
    let refl_h = if cfg.reflect && cfg.edge == Edge::Bottom {
        s * (cfg.get_i32(Key::REFL_SIZE) as f32 / 100.0) + sc(cfg.get_i32(Key::REFL_GAP)) + 2.0
    } else {
        0.0
    };
    let margin = cfg.margin as f32 + refl_h;
    let bar_h = s + 2.0 * pad + 4.0;

    let clock_w = clock_width(cfg, dpi);
    let widgets = WidgetStrip::build(cfg, dpi, bar_h, gap, clock_w);
    let widget_extra = widgets.extra;

    // Above the bar: zoom overflow plus half an icon for the label bubble.
    let mut overhang =
        (z - s - pad).max(0.0) + s * LABEL_BUBBLE_SCALE + LABEL_BUBBLE_PAD + SHADOW_PAD;
    if !cfg.edge.is_horizontal() {
        // Vertical edges put labels inward; reserve room for long names.
        overhang += VERTICAL_LABEL_ROOM;
    }

    let mut w = wb * fit + (z - s) * ZOOM_OVERFLOW_SLOTS + s + SIDE_RESERVE + widget_extra;
    if n_items == 0 {
        w = wb + SIDE_RESERVE;
    }
    if w > along {
        w = along;
    }

    Metrics {
        s,
        z,
        gap,
        pad_x,
        sep_w,
        pad,
        bar_h,
        overhang,
        margin,
        widgets,
        widget_extra,
        clock_w,
        refl_h,
        lw: w as i32,
        lh: (bar_h + overhang + margin) as i32 + 2,
    }
}
