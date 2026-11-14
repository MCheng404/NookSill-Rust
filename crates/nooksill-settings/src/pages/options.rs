//! Option tables for widgets; the integers are the codes stored in the INI.
//!
//! Shared here rather than per page so a combo box that appears twice (the
//! icon reflection shows up on two pages) cannot drift in meaning.

pub(crate) const THEME_OPTS: &[(i32, &str)] = &[(0, "Dark"), (1, "Light"), (2, "Follow system")];

/// Note: `UiTheme` uses the opposite 0/1 order from `DockTheme`.
pub(crate) const UI_THEME_OPTS: &[(i32, &str)] = &[(0, "Light"), (1, "Dark"), (2, "Follow system")];

pub(crate) const BG_OPTS: &[(i32, &str)] = &[(0, "Solid"), (1, "Gradient"), (2, "Frosted glass")];

pub(crate) const EDGE_OPTS: &[(i32, &str)] = &[
    (0, "Screen bottom"),
    (1, "Screen top"),
    (2, "Screen left"),
    (3, "Screen right"),
];

pub(crate) const MODE_OPTS: &[(i32, &str)] =
    &[(0, "Always visible"), (1, "Auto-hide"), (2, "Smart hide")];

pub(crate) const PLATE_OPTS: &[(i32, &str)] = &[
    (0, "No plate"),
    (1, "Solid plate"),
    (2, "Translucent plate"),
];

pub(crate) const PLATE_THEME_OPTS: &[(i32, &str)] =
    &[(0, "Light"), (1, "Dark"), (2, "Follow Dock theme")];

pub(crate) const DOT_OPTS: &[(i32, &str)] = &[(0, "Bar"), (1, "Dot")];

pub(crate) const MENU_THEME_OPTS: &[(i32, &str)] =
    &[(0, "Light"), (1, "Dark"), (2, "Follow system")];

pub(crate) const CLOCK_SHOW_OPTS: &[(i32, &str)] = &[
    (0, "Time + date (stacked)"),
    (1, "Time only"),
    (2, "Date only"),
];

pub(crate) const DATE_FMT_OPTS: &[(i32, &str)] =
    &[(0, "22 October 2026"), (1, "2026/10/22"), (2, "2026-10-22")];

pub(crate) const BROWSER_OPTS: &[(i32, &str)] = &[
    (0, "Automatic (Edge first)"),
    (1, "Microsoft Edge"),
    (2, "Google Chrome"),
    (3, "Mozilla Firefox"),
    (4, "System default browser"),
];

/// Indices must match `core::smoke::PALETTE`, whose order is the upstream
/// `kRauchRgb` order and not the order of this list's last two entries.
pub(crate) const SMOKE_COLOR_OPTS: &[(i32, &str)] = &[
    (0, "Purple"),
    (1, "Orange"),
    (2, "Red"),
    (3, "Pink"),
    (4, "Light blue"),
    (5, "Ice blue"),
    (6, "Green"),
    (7, "Gold"),
    (8, "Follow system accent"),
    (9, "Rainbow (90-second cycle)"),
];

/// Second colour, blended left to right along the bar; `-1` disables it.
pub(crate) const SMOKE_COLOR2_OPTS: &[(i32, &str)] = &[
    (-1, "None"),
    (0, "Purple"),
    (1, "Orange"),
    (2, "Red"),
    (3, "Pink"),
    (4, "Light blue"),
    (5, "Ice blue"),
    (6, "Green"),
    (7, "Gold"),
];

/// The two steps above 100% are alpha gain, not a different colour ramp.
pub(crate) const SMOKE_OPACITY_OPTS: &[(i32, &str)] = &[
    (0, "17%"),
    (1, "30%"),
    (2, "50%"),
    (3, "70%"),
    (4, "100%"),
    (5, "125%"),
    (6, "150%"),
];

pub(crate) const SMOKE_DENSITY_OPTS: &[(i32, &str)] = &[(0, "Sparse"), (1, "Normal"), (2, "Dense")];

pub(crate) const SMOKE_SPEED_OPTS: &[(i32, &str)] = &[(0, "Calm"), (1, "Normal"), (2, "Lively")];

/// Glow thickness, which also sets the simulated outer margin.
pub(crate) const SMOKE_THICK_OPTS: &[(i32, &str)] =
    &[(0, "Thin"), (1, "Normal"), (2, "Thick"), (3, "Extra thick")];

pub(crate) const SMOKE_REACH_OPTS: &[(i32, &str)] = &[(0, "Small"), (1, "Normal"), (2, "Large")];

/// 0 picks the level from window area, using 2x for large windows to save VRAM.
pub(crate) const MSAA_OPTS: &[(i32, &str)] = &[
    (0, "Automatic (2x on large windows)"),
    (1, "Off"),
    (2, "2x"),
    (4, "4x (sharpest)"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_option_label_is_registered() {
        let tables: &[&[(i32, &str)]] = &[
            THEME_OPTS,
            UI_THEME_OPTS,
            BG_OPTS,
            EDGE_OPTS,
            MODE_OPTS,
            PLATE_OPTS,
            PLATE_THEME_OPTS,
            DOT_OPTS,
            MENU_THEME_OPTS,
            CLOCK_SHOW_OPTS,
            DATE_FMT_OPTS,
            BROWSER_OPTS,
            SMOKE_COLOR_OPTS,
            SMOKE_COLOR2_OPTS,
            SMOKE_OPACITY_OPTS,
            SMOKE_DENSITY_OPTS,
            SMOKE_SPEED_OPTS,
            SMOKE_THICK_OPTS,
            SMOKE_REACH_OPTS,
            MSAA_OPTS,
        ];
        for table in tables {
            for pair in table.iter() {
                assert!(
                    nooksill_core::i18n::has_key(pair.1),
                    "unregistered option: {}",
                    pair.1
                );
            }
        }
    }
}
