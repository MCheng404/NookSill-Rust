//! Key name, section and range lookups.
//!
//! Built on the key table and the section prefix rules in `keys`.

use super::keys::section_of_numeric;
use super::*;

impl Key {
    /// Item-by-item the ini row order, which must match exactly: saving
    /// writes back in the same order, so round-tripping between two
    /// implementations produces no row-order diff.
    ///
    /// 118 keys: 35 typed plus 83 table-driven (`Numeric`).
    pub const ALL: &'static [Key] = &[
        Key::IconSize,
        Key::ZoomSize,
        Key::Spacing,
        Key::Margin,
        Key::HPAD,
        Key::Padding,
        Key::Radius,
        Key::Opacity,
        Key::Blur,
        Key::Zoom,
        Key::Theme,
        Key::Dots,
        Key::Labels,
        Key::Bounce,
        Key::WebAppMode,
        Key::Border,
        Key::BgMode,
        Key::Mode,
        Key::Reserve,
        Key::Edge,
        Key::Monitor,
        Key::Preview,
        Key::ShowRunning,
        Key::Lock,
        Key::Hotkey,
        Key::DotStyle,
        Key::PLATE_MODE,
        Key::PLATE_THEME,
        Key::PLATE_OPACITY,
        Key::PlateRadius,
        Key::BgColor1,
        Key::BgColor2,
        Key::BG_COLOR3,
        Key::AccentTone,
        Key::Reflect,
        Key::Clock,
        Key::FolderFan,
        Key::MinAnim,
        Key::Notes,
        Key::UiTheme,
        Key::MENU_THEME,
        Key::BORDER_COLOR,
        Key::BORDER_WIDTH,
        Key::SEP_COLOR,
        Key::DOT_COLOR,
        Key::GRAD_ANGLE,
        Key::GRAD_A1,
        Key::GRAD_A2,
        Key::GRAD_A3,
        Key::CLOCK_SHOW,
        Key::CLOCK_COL_LIGHT,
        Key::CLOCK_COL_DARK,
        Key::REFL_SIZE,
        Key::REFL_GAP,
        Key::REFL_OPACITY,
        Key::FAN_ARC,
        Key::FAN_GAP,
        Key::FAN_CUT,
        Key::GROUP_FROST,
        Key::GROUP_POS,
        Key::GUARDIAN,
        Key::CLICK_TOGGLE,
        Key::SEP_GAP,
        Key::SEP_WIDTH,
        Key::CLOCK_SEC,
        Key::CLOCK_WEEK,
        Key::DATE_FMT,
        Key::CLOCK_ORDER,
        Key::CLOCK_BOLD_T,
        Key::CLOCK_BOLD_D,
        Key::BROWSER,
        Key::GRP_OPACITY,
        Key::GRP_BG,
        Key::MIN_STYLE,
        Key::MN_AUTO,
        Key::MN_NOTES,
        Key::MN_ADD,
        // Second batch, in append order.
        Key::LAUNCHPAD,
        Key::W_SIDE,
        Key::W_ORDER,
        Key::NOTIFY_W,
        Key::FAN_BG_L,
        Key::FAN_BG_D,
        Key::FAN_FG_L,
        Key::FAN_FG_D,
        Key::PIN_MENU,
        Key::MULTI_MODE,
        Key::UPD_CHK,
        Key::UPD_LAST,
        Key::LP_COLS,
        Key::LP_ROWS,
        Key::FAN_DRAG,
        Key::TRAY_HIDE,
        Key::CTRL_CENTER,
        Key::CC_ENERGY,
        Key::CC_HOTSPOT,
        Key::RES_MON,
        Key::TOOL_W,
        Key::WIDE_MASK,
        Key::MODE_W,
        Key::UI_FROST,
        Key::LP_FROST,
        Key::TRAY_W,
        Key::W_MERGE,
        Key::STATUS_BAR,
        Key::SB_FROST,
        Key::SB_OPACITY,
        Key::SB_COLOR,
        Key::SB_RADIUS,
        Key::SB_HEIGHT,
        Key::SB_FOLLOW_SYS,
        Key::SB_TOP_GAP,
        Key::SB_MAX_AVOID,
        Key::SB_FLOAT,
        Key::SB_SIDE_GAP,
        Key::SB_AUTO_HIDE,
        Key::SEP_SCALE,
        Key::MENU_FROST,
    ];

    /// Own keys, not in the row-order table, so they cannot join
    /// [`Key::ALL`].
    ///
    /// `Key::ALL` has a hard constraint: it maps item-by-item onto the ini row
    /// order and has the same length (`ALL.len == UPSTREAM.len` is the
    /// regression test that pins it). That is the basis for round-tripping
    /// between two implementations without a row-order diff, so adding an own
    /// key would break it.
    ///
    /// Read, write and clamp all iterate `ALL` and then `EXTRA`, so a new key
    /// automatically gets "missing means default" and "out of range means
    /// clamped" without any hand-written branch.
    pub const EXTRA: &'static [Key] = &[
        Key::SMOKE,
        Key::SMOKE_COLOR,
        Key::SMOKE_COLOR2,
        Key::SMOKE_OPACITY,
        Key::SMOKE_DENSITY,
        Key::SMOKE_SPEED,
        Key::SMOKE_TONE,
        Key::SMOKE_THICK,
        Key::SMOKE_MOUSE,
        Key::SMOKE_REACH,
        Key::SMOKE_CLOUDS,
        Key::PANEL_DISMISS,
        Key::UI_SCALE,
        Key::UI_ACCENT,
        Key::UI_ACCENT_DOCK,
        Key::FRAME_CAP,
        Key::VSYNC,
        Key::MSAA,
        Key::UI_ACCENT_DOCK_ICONS,
        Key::UI_ACCENT_CC_ICONS,
        Key::NET_SECURE_DNS,
        Key::NET_MIRROR_ALL,
        // Auto-hide sensitivity delays and widget/panel appearance.
        Key::HIDE_DELAY,
        Key::SHOW_DELAY,
        Key::HIDE_DELAY_MAX,
        Key::SHOW_DELAY_MAX,
        Key::WIDGET_OPACITY,
        Key::WIDGET_RADIUS,
        Key::WIDGET_GAP,
        Key::WIDGET_BLUR,
        Key::PANEL_OPACITY,
        Key::PANEL_OVERLAY,
        Key::FROST_ILLUSION,
        // Updater switches.
        Key::UPD_CHANNEL,
        Key::UPD_PROMPT,
        Key::UPD_AUTO,
    ];

    /// Every key that needs reading and writing: the row-order table first,
    /// then the own keys.
    ///
    /// An iterator rather than a concatenated array: `const` cannot splice
    /// slices, and both halves are `'static`, so chaining them is free.
    pub fn all_keys() -> impl Iterator<Item = Key> {
        Self::ALL.iter().chain(Self::EXTRA).copied()
    }

    /// Section this key belongs to in the new format.
    ///
    /// Grouped by meaning rather than putting everything into `[Settings]`,
    /// so a new feature shows at a glance where its key belongs.
    ///
    /// Numeric keys are classified by name prefix, which keeps the table
    /// structure unchanged: adding one still costs a single row.
    #[must_use]
    pub fn section(self) -> &'static str {
        match self {
            Key::IconSize
            | Key::ZoomSize
            | Key::Spacing
            | Key::Margin
            | Key::Padding
            | Key::Radius
            | Key::Opacity
            | Key::Blur
            | Key::Border => "appearance",

            Key::BgMode | Key::BgColor1 | Key::BgColor2 | Key::AccentTone => {
                "appearance.background"
            }

            Key::Mode | Key::Edge | Key::Monitor | Key::Reserve => "layout",

            Key::Zoom
            | Key::Dots
            | Key::Labels
            | Key::Bounce
            | Key::Preview
            | Key::ShowRunning
            | Key::Lock
            | Key::Clock
            | Key::Notes
            | Key::MinAnim
            | Key::FolderFan
            | Key::WebAppMode => "behavior",

            // Reflection: the switch and its three parameters share a section.
            Key::Reflect => "appearance.reflect",

            Key::Theme | Key::UiTheme | Key::PlateRadius | Key::DotStyle => "theme",

            Key::Hotkey => "advanced",

            Key::Numeric(i) => section_of_numeric(NUMERIC_KEYS[i as usize].0),
        }
    }

    /// Key name in the new format (lower snake case).
    ///
    /// The names differ from the old CamelCase ones, and the rename is not
    /// cosmetic: `Mode` -> `hide_mode` and `BgMode` -> `fill` both read as
    /// the wrong thing next to each other.
    ///
    /// Numeric keys keep their old names: most map to features that are not
    /// implemented yet, so naming them together with their semantics can
    /// wait.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Key::IconSize => "icon_size",
            Key::ZoomSize => "zoom_size",
            Key::Spacing => "spacing",
            Key::Margin => "margin",
            Key::Padding => "padding",
            Key::Radius => "radius",
            Key::Opacity => "opacity",
            Key::Blur => "blur",
            Key::Zoom => "zoom",
            Key::Theme => "dock_theme",
            Key::Dots => "dots",
            Key::Labels => "labels",
            Key::Bounce => "bounce",
            Key::WebAppMode => "web_app_mode",
            Key::Border => "border",
            Key::BgMode => "fill",
            Key::Mode => "hide_mode",
            Key::Reserve => "reserve",
            Key::Edge => "edge",
            Key::Monitor => "monitor",
            Key::Preview => "preview",
            Key::ShowRunning => "show_running",
            Key::Lock => "lock",
            Key::Hotkey => "hotkey",
            Key::DotStyle => "dot_style",
            Key::PlateRadius => "plate_radius",
            Key::BgColor1 => "color1",
            Key::BgColor2 => "color2",
            Key::AccentTone => "accent_tone",
            Key::Reflect => "reflect",
            Key::Clock => "clock",
            Key::FolderFan => "folder_fan",
            Key::MinAnim => "min_anim",
            Key::Notes => "notes",
            Key::UiTheme => "settings_theme",
            Key::Numeric(i) => NUMERIC_KEYS[i as usize].0,
        }
    }

    /// The old-format key name. Used only for reading legacy config files.
    ///
    /// Kept so an old config can be upgraded in place: `from_ini` falls back
    /// to this name in the `[Settings]` section when the new name is absent.
    /// New code should not use it.
    #[must_use]
    pub const fn legacy_name(self) -> &'static str {
        match self {
            Key::IconSize => "IconSize",
            Key::ZoomSize => "ZoomSize",
            Key::Spacing => "Spacing",
            Key::Margin => "Margin",
            Key::Padding => "Padding",
            Key::Radius => "Radius",
            Key::Opacity => "Opacity",
            Key::Blur => "Blur",
            Key::Zoom => "Zoom",
            Key::Theme => "Theme",
            Key::Dots => "Dots",
            Key::Labels => "Labels",
            Key::Bounce => "Bounce",
            Key::WebAppMode => "WebAppMode",
            Key::Border => "Border",
            Key::BgMode => "BgMode",
            Key::Mode => "Mode",
            Key::Reserve => "Reserve",
            Key::Edge => "Edge",
            Key::Monitor => "Monitor",
            Key::Preview => "Preview",
            Key::ShowRunning => "ShowRunning",
            Key::Lock => "Lock",
            Key::Hotkey => "Hotkey",
            Key::DotStyle => "DotStyle",
            Key::PlateRadius => "PlateRadius",
            Key::BgColor1 => "BgColor1",
            Key::BgColor2 => "BgColor2",
            Key::AccentTone => "AccentTone",
            Key::Reflect => "Reflect",
            Key::Clock => "Clock",
            Key::FolderFan => "FolderFan",
            Key::MinAnim => "MinAnim",
            Key::Notes => "Notes",
            Key::UiTheme => "UiTheme",
            // Numeric keys were not renamed.
            Key::Numeric(i) => NUMERIC_KEYS[i as usize].0,
        }
    }

    /// Case-insensitive key name lookup. A hand-written `iconsize=64` counts.
    #[must_use]
    pub fn from_name(s: &str) -> Option<Self> {
        let s = s.trim();
        // The new name first, then the old CamelCase one: hand-written
        // configs, fragments left by older versions, and both formats this
        // program writes must all be recognized.
        Self::ALL
            .iter()
            .copied()
            .find(|k| k.name().eq_ignore_ascii_case(s))
            .or_else(|| {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|k| k.legacy_name().eq_ignore_ascii_case(s))
            })
    }

    /// Hard clamp interval `(min, max)` used when loading. This is a safety
    /// boundary, not the UI hint range.
    ///
    /// The match deliberately has no `_` arm: a new key without a range makes
    /// the compiler report a non-exhaustive match here, instead of letting it
    /// fall silently into the boolean range `(0, 1)`.
    #[must_use]
    pub fn range(self) -> (i32, i32) {
        match self {
            Key::Numeric(i) => {
                let row = NUMERIC_KEYS[i as usize];
                (row.1, row.2)
            }
            // Geometry. The upper bound of 256 leaves room for high DPI
            // (128 px at 200%).
            Key::IconSize => (16, 256),
            Key::ZoomSize => (16, 256),
            Key::Spacing => (0, 64),
            Key::Padding => (0, 64),
            Key::Radius => (0, 120),
            // Margin only had a lower bound; the upper bound is added because
            // it feeds the window size calculation and a large value would
            // push the window past the screen.
            Key::Margin => (0, 400),
            Key::Opacity => (0, 100),
            Key::Blur => (1, 32),
            Key::PlateRadius => (0, 50),
            Key::Monitor => (0, 63),
            // Enums: the interval is for UI display only, the value is
            // clamped by code.
            Key::Theme | Key::UiTheme | Key::BgMode | Key::Mode => (0, 2),
            Key::Edge => (0, 3),
            Key::DotStyle => (0, 1),
            // Colors: -1 means unset, 0..=0xFFFFFF is a COLORREF.
            Key::BgColor1 | Key::BgColor2 => (-1, 0x00FF_FFFF),
            // Bools: only 0/1 count.
            Key::Zoom
            | Key::Dots
            | Key::Labels
            | Key::Bounce
            | Key::WebAppMode
            | Key::Border
            | Key::Reserve
            | Key::Preview
            | Key::ShowRunning
            | Key::Lock
            | Key::Hotkey
            | Key::AccentTone
            | Key::Reflect
            | Key::Clock
            | Key::FolderFan
            | Key::MinAnim
            | Key::Notes => (0, 1),
        }
    }

    /// Slider hint range in the settings UI.
    ///
    /// Narrower than `range`: this is the recommended adjustable range, while
    /// `range` is the boundary that keeps the program working. Keys without a
    /// slider return `None`.
    #[must_use]
    pub const fn slider_range(self) -> Option<(i32, i32)> {
        Some(match self {
            Key::IconSize => (24, 128),
            Key::ZoomSize => (32, 220),
            Key::Spacing => (0, 30),
            Key::Padding => (2, 30),
            Key::Radius => (0, 40),
            Key::Opacity => (0, 100),
            Key::Blur => (1, 14),
            Key::Margin => (0, 150),
            Key::PlateRadius => (0, 50),
            Key::FRAME_CAP => (0, 240),
            Key::HIDE_DELAY | Key::SHOW_DELAY | Key::HIDE_DELAY_MAX | Key::SHOW_DELAY_MAX => {
                (0, 3000)
            }
            Key::WIDGET_OPACITY => (0, 100),
            Key::WIDGET_RADIUS => (0, 40),
            Key::WIDGET_BLUR => (0, 14),
            Key::WIDGET_GAP => (0, 24),
            Key::PANEL_OPACITY | Key::PANEL_OVERLAY => (0, 100),
            _ => return None,
        })
    }

    /// Whether the key maps to an enum (decides `from_code` vs numeric clamp).
    ///
    /// Table-driven numeric keys are not enums: they are raw ints and an
    /// out-of-range value is clamped to the range, not folded into a default
    /// variant.
    #[must_use]
    pub const fn is_enum(self) -> bool {
        matches!(
            self,
            Key::Theme | Key::UiTheme | Key::BgMode | Key::Mode | Key::Edge | Key::DotStyle
        )
    }

    /// Whether the key has boolean semantics (written back as 0/1).
    #[must_use]
    pub fn is_bool(self) -> bool {
        if let Key::Numeric(i) = self {
            return NUMERIC_KEYS[i as usize].4;
        }
        matches!(
            self,
            Key::Zoom
                | Key::Dots
                | Key::Labels
                | Key::Bounce
                | Key::WebAppMode
                | Key::Border
                | Key::Reserve
                | Key::Preview
                | Key::ShowRunning
                | Key::Lock
                | Key::Hotkey
                | Key::AccentTone
                | Key::Reflect
                | Key::Clock
                | Key::FolderFan
                | Key::MinAnim
                | Key::Notes
        )
    }

    /// Whether the key is a length (96 DPI logical pixels, scaled by
    /// `dpi / 96` at render time).
    ///
    /// Icon base size, spacing, corner radius and padding live here.
    /// Percentages (opacity, reflection ratio), angles, enums and colors are
    /// not: scaling those would change their meaning.
    ///
    /// The migration (`CONFIG_VERSION` 3 -> 4) and the renderer share this
    /// one check, so adding a length key only touches the last column of
    /// [`NUMERIC_KEYS`].
    #[must_use]
    pub const fn is_length(self) -> bool {
        match self {
            Key::IconSize
            | Key::ZoomSize
            | Key::Spacing
            | Key::Padding
            | Key::Margin
            | Key::Radius => true,
            Key::Numeric(i) => NUMERIC_KEYS[i as usize].5,
            _ => false,
        }
    }
}
