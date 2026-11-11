//! `[Settings]` model: typed enums, defaults, clamps and ini I/O.
//!
//! Key count and order are fixed; bare int keys live in [`NUMERIC_KEYS`], so a
//! new key costs one table row instead of a new match arm.

use crate::hotkey::Hotkey;
use crate::ini::Ini;
use crate::units;

/// Config format version. Bump it when keys or sections change.
///
/// Version 4 stores length keys as 96 DPI logical pixels; older versions stored
/// physical pixels, see [`Config::migrate_units`].
pub const CONFIG_VERSION: i32 = 4;

/// Section holding metadata such as the version.
pub const META_SECTION: &str = "meta";

/// Section the old format put everything into. Read path only.
pub const LEGACY_SECTION: &str = "Settings";

mod apply;
mod ini_io;
mod key_query;
mod keys;
mod migrate;

pub use keys::{Key, NUMERIC_KEYS};

/// Dock theme. `0 = dark`, `1 = light`, `2 = follow system`.
///
/// Codes are inverted relative to [`UiTheme`]; swapping them breaks existing ini files.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum DockTheme {
    #[default]
    Dark = 0,
    Light = 1,
    System = 2,
}

/// Settings window theme. `0 = light`, `1 = dark`, `2 = follow system`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum UiTheme {
    Light = 0,
    Dark = 1,
    #[default]
    System = 2,
}

/// Background fill. `0 = solid`, `1 = gradient`, `2 = frosted glass`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum BgMode {
    Solid = 0,
    Gradient = 1,
    #[default]
    Frost = 2,
}

/// Hide behavior. `0 = always visible`, `1 = auto hide`, `2 = smart hide`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum DockMode {
    #[default]
    Always = 0,
    AutoHide = 1,
    SmartHide = 2,
}

/// Docking edge.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Edge {
    #[default]
    Bottom = 0,
    Top = 1,
    Left = 2,
    Right = 3,
}

impl Edge {
    /// Returns true for the top and bottom edges.
    #[must_use]
    pub const fn is_horizontal(self) -> bool {
        matches!(self, Edge::Bottom | Edge::Top)
    }
}

/// Running indicator shape: bar or dot.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum DotStyle {
    #[default]
    Bar = 0,
    Dot = 1,
}

macro_rules! code_enum {
    ($name:ident { $($variant:ident = $code:expr),+ $(,)? }, default = $default:expr) => {
        impl $name {
            #[must_use]
            pub const fn from_code(v: i32) -> Self {
                match v {
                    $($code => Self::$variant,)+
                    _ => $default,
                }
            }
            #[must_use]
            pub const fn code(self) -> i32 {
                match self {
                    $(Self::$variant => $code,)+
                }
            }
        }
    };
}

code_enum!(DockTheme { Dark = 0, Light = 1, System = 2 }, default = DockTheme::Dark);
code_enum!(UiTheme { Light = 0, Dark = 1, System = 2 }, default = UiTheme::System);
code_enum!(BgMode { Solid = 0, Gradient = 1, Frost = 2 }, default = BgMode::Frost);
code_enum!(DockMode { Always = 0, AutoHide = 1, SmartHide = 2 }, default = DockMode::Always);
code_enum!(Edge { Bottom = 0, Top = 1, Left = 2, Right = 3 }, default = Edge::Bottom);
code_enum!(DotStyle { Bar = 0, Dot = 1 }, default = DotStyle::Bar);

/// Full `[Settings]` model. Lengths are 96 DPI logical pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub icon_size: i32,
    pub zoom_size: i32,
    pub spacing: i32,
    pub margin: i32,
    pub pad: i32,
    pub radius: i32,
    pub opacity: i32,
    pub blur: i32,

    pub zoom: bool,
    pub dots: bool,
    pub labels: bool,
    pub bounce: bool,
    pub web_app: bool,
    pub border: bool,
    pub reserve: bool,
    pub preview: bool,
    pub show_running: bool,
    pub lock: bool,
    /// Enable the global toggle hotkey. The combination lives in `[Hotkeys]`.
    pub hotkey: bool,
    /// Key combination for the global toggle, parsed from `[Hotkeys] Toggle`.
    pub hotkey_combo: Hotkey,
    pub accent_tone: bool,
    pub reflect: bool,
    pub clock: bool,
    pub folder_fan: bool,
    pub min_anim: bool,
    pub notes: bool,

    pub theme: DockTheme,
    pub ui_theme: UiTheme,
    pub bg_mode: BgMode,
    pub mode: DockMode,
    pub edge: Edge,
    pub dot_style: DotStyle,

    pub monitor: i32,
    pub plate_radius: i32,
    /// `-1` = not set, otherwise a `COLORREF` (`0x00BBGGRR`).
    pub bg_color1: i32,
    pub bg_color2: i32,

    /// Values of the table-driven keys, in [`NUMERIC_KEYS`] order.
    ///
    /// A flat vector keeps a new key a one-row change instead of a new field.
    pub numeric: Vec<i32>,

    /// Preferred GPU adapter name; empty means automatic.
    ///
    /// Stored by name, not index, because indices shift on driver updates and GPU
    /// hot-plug. Kept in `[Graphics]` so `[Settings]` keeps mirroring the ini order.
    pub adapter: String,
    /// Control-center tile order (`[widgets] Order`, comma-separated ids).
    ///
    /// Empty means never dragged; ids are validated by `control::parse_order`.
    pub cc_order: String,
    /// UI language tag; empty means follow the OS display language.
    pub language: String,
    /// Runtime accent color (RGB 0..=255) resolved from the system accent.
    ///
    /// Not persisted: `None` means the theme default.
    pub runtime_accent: Option<[u8; 3]>,
    /// Runtime icon accent, independent of `runtime_accent`. Not persisted.
    pub runtime_icon_accent: Option<[u8; 3]>,
}

#[cfg(test)]
mod tests;
