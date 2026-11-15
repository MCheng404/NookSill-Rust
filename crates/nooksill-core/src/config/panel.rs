//! Declarative registry for the panels opened from Dock widgets.
//!
//! Identity, placement policy and background material live here as pure data.
//! The gpu crate only turns a policy into Win32 geometry and a frost layer.

use super::{BgMode, Config, Key};
use crate::layout::WidgetKind;

/// Every panel that shares the popup frame style.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PanelKind {
    Calendar,
    ControlCenter,
    Resource,
    Tool,
    Notes,
    Group,
    Editor,
}

impl PanelKind {
    pub const ALL: [Self; 7] = [
        Self::Calendar,
        Self::ControlCenter,
        Self::Resource,
        Self::Tool,
        Self::Notes,
        Self::Group,
        Self::Editor,
    ];

    /// Stable index into [`REGISTRY`].
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Calendar => 0,
            Self::ControlCenter => 1,
            Self::Resource => 2,
            Self::Tool => 3,
            Self::Notes => 4,
            Self::Group => 5,
            Self::Editor => 6,
        }
    }

    /// Slot in the packed per-panel override; the modal editor has none.
    #[must_use]
    pub const fn override_slot(self) -> Option<u32> {
        match self {
            Self::Calendar => Some(0),
            Self::ControlCenter => Some(1),
            Self::Resource => Some(2),
            Self::Tool => Some(3),
            Self::Notes => Some(4),
            Self::Group => Some(5),
            Self::Editor => None,
        }
    }

    /// Card that supplies the anchor rect; `None` means the caller passes one.
    #[must_use]
    pub const fn anchor_card(self) -> Option<WidgetKind> {
        match self {
            Self::Calendar => Some(WidgetKind::Clock),
            Self::ControlCenter => Some(WidgetKind::CtrlCenter),
            Self::Resource => Some(WidgetKind::Resource),
            Self::Tool => Some(WidgetKind::Tool),
            Self::Notes | Self::Group | Self::Editor => None,
        }
    }

    /// Panel opened by clicking a widget card.
    #[must_use]
    pub const fn from_widget(kind: WidgetKind) -> Option<Self> {
        match kind {
            WidgetKind::Clock => Some(Self::Calendar),
            WidgetKind::CtrlCenter => Some(Self::ControlCenter),
            WidgetKind::Resource => Some(Self::Resource),
            WidgetKind::Tool => Some(Self::Tool),
            _ => None,
        }
    }
}

/// Where a panel opens.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Above (or beside, for a vertical dock) the widget that opened it.
    FollowWidget,
    /// Centred in the work area of the dock's monitor.
    ScreenCenter,
    /// Follow the widget when the panel fits, otherwise centre it.
    Auto,
}

impl Placement {
    #[must_use]
    pub const fn from_code(v: i32) -> Self {
        match v {
            1 => Self::ScreenCenter,
            2 => Self::Auto,
            _ => Self::FollowWidget,
        }
    }

    #[must_use]
    pub const fn code(self) -> i32 {
        match self {
            Self::FollowWidget => 0,
            Self::ScreenCenter => 1,
            Self::Auto => 2,
        }
    }
}

/// Background material of a panel.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Backdrop {
    /// Use the bar's `fill` mode.
    FollowBar,
    Solid,
    Gradient,
    Frost,
}

impl Backdrop {
    #[must_use]
    pub const fn from_code(v: i32) -> Self {
        match v {
            1 => Self::Solid,
            2 => Self::Gradient,
            3 => Self::Frost,
            _ => Self::FollowBar,
        }
    }

    #[must_use]
    pub const fn code(self) -> i32 {
        match self {
            Self::FollowBar => 0,
            Self::Solid => 1,
            Self::Gradient => 2,
            Self::Frost => 3,
        }
    }

    /// Resolve `FollowBar` against the bar's material.
    #[must_use]
    pub const fn effective(self, bar: BgMode) -> Self {
        match self {
            Self::FollowBar => match bar {
                BgMode::Solid => Self::Solid,
                BgMode::Gradient => Self::Gradient,
                BgMode::Frost => Self::Frost,
            },
            other => other,
        }
    }
}

/// Declaration of one panel. Adding a panel is one row, not seven match arms.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Descriptor {
    pub kind: PanelKind,
    pub anchor_card: Option<WidgetKind>,
    /// Placement before the unified policy existed.
    pub legacy: Placement,
    pub overridable: bool,
}

pub const REGISTRY: [Descriptor; 7] = [
    Descriptor {
        kind: PanelKind::Calendar,
        anchor_card: Some(WidgetKind::Clock),
        legacy: Placement::FollowWidget,
        overridable: true,
    },
    Descriptor {
        kind: PanelKind::ControlCenter,
        anchor_card: Some(WidgetKind::CtrlCenter),
        legacy: Placement::FollowWidget,
        overridable: true,
    },
    Descriptor {
        kind: PanelKind::Resource,
        anchor_card: Some(WidgetKind::Resource),
        legacy: Placement::ScreenCenter,
        overridable: true,
    },
    Descriptor {
        kind: PanelKind::Tool,
        anchor_card: Some(WidgetKind::Tool),
        legacy: Placement::ScreenCenter,
        overridable: true,
    },
    Descriptor {
        kind: PanelKind::Notes,
        anchor_card: None,
        legacy: Placement::ScreenCenter,
        overridable: true,
    },
    Descriptor {
        kind: PanelKind::Group,
        anchor_card: None,
        legacy: Placement::FollowWidget,
        overridable: true,
    },
    Descriptor {
        kind: PanelKind::Editor,
        anchor_card: None,
        legacy: Placement::ScreenCenter,
        overridable: false,
    },
];

/// Declaration for `kind`.
#[must_use]
pub fn spec(kind: PanelKind) -> &'static Descriptor {
    &REGISTRY[kind.index()]
}

/// Six panels times two bits; zero in a slot means inherit the global policy.
pub const OVERRIDE_MAX: i32 = (1 << 12) - 1;

/// Per-panel override, or `None` when the slot inherits.
#[must_use]
pub fn override_get(packed: i32, kind: PanelKind) -> Option<Placement> {
    let slot = kind.override_slot()?;
    let bits = (packed.max(0) >> (slot * 2)) & 3;
    match bits {
        1 => Some(Placement::FollowWidget),
        2 => Some(Placement::ScreenCenter),
        3 => Some(Placement::Auto),
        _ => None,
    }
}

/// Write one slot, leaving every other slot untouched.
#[must_use]
pub fn override_set(packed: i32, kind: PanelKind, value: Option<Placement>) -> i32 {
    let Some(slot) = kind.override_slot() else {
        return packed.clamp(0, OVERRIDE_MAX);
    };
    let bits = match value {
        None => 0,
        Some(Placement::FollowWidget) => 1,
        Some(Placement::ScreenCenter) => 2,
        Some(Placement::Auto) => 3,
    };
    let shift = slot * 2;
    let mask = 3 << shift;
    ((packed.max(0) & !mask) | (bits << shift)).min(OVERRIDE_MAX)
}

/// Process-wide panel policy, rebuilt whenever the config changes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub global: Placement,
    pub overrides: i32,
    pub backdrop: Backdrop,
    /// Blur radius in capture texels; `WidgetBlur` overrides the global `Blur`.
    pub blur: i32,
}

impl Prefs {
    /// Read the policy from config.
    #[must_use]
    pub fn from_cfg(cfg: &Config) -> Self {
        // `WidgetBlur` was the popup override; one `Blur` now covers every
        // frosted surface. `boot` merges a tuned override into `Blur` once.
        let blur = cfg.get_i32(Key::Blur).clamp(1, 14);
        Self {
            global: Placement::from_code(cfg.get_i32(Key::PANEL_POS)),
            overrides: cfg.get_i32(Key::PANEL_POS_OVERRIDE).clamp(0, OVERRIDE_MAX),
            // One material for every surface: the bar's `Background material`
            // also decides the popups. `PanelBg` is a legacy key, not read.
            backdrop: match cfg.get_i32(Key::BgMode) {
                0 => Backdrop::Solid,
                1 => Backdrop::Gradient,
                _ => Backdrop::Frost,
            },
            blur,
        }
    }

    /// Placement of one panel: per-panel override, then the global policy.
    #[must_use]
    pub fn placement(self, kind: PanelKind) -> Placement {
        if kind == PanelKind::Editor {
            return Placement::ScreenCenter;
        }
        override_get(self.overrides, kind).unwrap_or(self.global)
    }

    /// Material of one panel after resolving `FollowBar`.
    #[must_use]
    pub fn material(self, bar: BgMode) -> Backdrop {
        self.backdrop.effective(bar)
    }

    /// Whether this panel needs a live desktop capture.
    #[must_use]
    pub fn frost_wanted(self, bar: BgMode) -> bool {
        self.material(bar) == Backdrop::Frost
    }
}

impl Default for Prefs {
    fn default() -> Self {
        Self::from_cfg(&Config::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_indexed_by_kind_and_unique() {
        for kind in PanelKind::ALL {
            assert_eq!(spec(kind).kind, kind);
            assert_eq!(spec(kind).anchor_card, kind.anchor_card());
            assert_eq!(spec(kind).overridable, kind.override_slot().is_some());
        }
        assert_eq!(
            PanelKind::from_widget(WidgetKind::Clock),
            Some(PanelKind::Calendar)
        );
        assert_eq!(PanelKind::from_widget(WidgetKind::Tray), None);
    }

    #[test]
    fn override_slots_are_independent() {
        let mut packed = override_set(0, PanelKind::Calendar, Some(Placement::ScreenCenter));
        packed = override_set(packed, PanelKind::Tool, Some(Placement::Auto));
        assert_eq!(
            override_get(packed, PanelKind::Calendar),
            Some(Placement::ScreenCenter)
        );
        assert_eq!(override_get(packed, PanelKind::Tool), Some(Placement::Auto));
        assert_eq!(override_get(packed, PanelKind::Resource), None);
        packed = override_set(packed, PanelKind::Calendar, None);
        assert_eq!(override_get(packed, PanelKind::Calendar), None);
        assert_eq!(override_get(packed, PanelKind::Tool), Some(Placement::Auto));
        assert_eq!(packed, 3 << (3 * 2));
    }

    #[test]
    fn override_is_robust_to_bad_values() {
        assert_eq!(override_get(-1, PanelKind::Notes), None);
        assert_eq!(override_get(i32::MAX, PanelKind::Editor), None);
        assert_eq!(
            override_set(i32::MAX, PanelKind::Editor, None),
            OVERRIDE_MAX
        );
        assert_eq!(override_set(-7, PanelKind::Group, None), 0);
    }

    #[test]
    fn placement_resolves_override_then_global_then_editor() {
        let mut prefs = Prefs {
            global: Placement::ScreenCenter,
            ..Prefs::default()
        };
        assert_eq!(prefs.placement(PanelKind::Tool), Placement::ScreenCenter);
        prefs.overrides = override_set(0, PanelKind::Tool, Some(Placement::FollowWidget));
        assert_eq!(prefs.placement(PanelKind::Tool), Placement::FollowWidget);
        assert_eq!(prefs.placement(PanelKind::Notes), Placement::ScreenCenter);
        assert_eq!(prefs.placement(PanelKind::Editor), Placement::ScreenCenter);
    }

    #[test]
    fn material_resolves_follow_bar_and_explicit_modes() {
        let follow = Prefs {
            backdrop: Backdrop::FollowBar,
            ..Prefs::default()
        };
        assert_eq!(follow.material(BgMode::Solid), Backdrop::Solid);
        assert_eq!(follow.material(BgMode::Gradient), Backdrop::Gradient);
        assert_eq!(follow.material(BgMode::Frost), Backdrop::Frost);
        let solid = Prefs {
            backdrop: Backdrop::Solid,
            ..Prefs::default()
        };
        assert!(!solid.frost_wanted(BgMode::Frost));
        let frost = Prefs {
            backdrop: Backdrop::Frost,
            ..Prefs::default()
        };
        assert!(frost.frost_wanted(BgMode::Gradient));
    }

    #[test]
    fn defaults_follow_the_widget_and_the_bar() {
        let cfg = Config::default();
        let prefs = Prefs::from_cfg(&cfg);
        assert_eq!(prefs.global, Placement::FollowWidget);
        assert_eq!(prefs.overrides, 0);
        assert_eq!(
            prefs.backdrop,
            Backdrop::Frost,
            "the bar material is the one switch"
        );
        assert!(prefs.frost_wanted(cfg.bg_mode));
        assert!(prefs.blur >= 1 && prefs.blur <= 14);
    }

    #[test]
    fn preload_panels_is_on_by_default_and_lives_in_widgets() {
        let mut cfg = Config::default();
        assert_eq!(cfg.get_i32(Key::PRELOAD_PANELS), 1);
        assert_eq!(Key::PRELOAD_PANELS.section(), "widgets");
        cfg.apply_i32(Key::PRELOAD_PANELS, 0);
        assert_eq!(cfg.get_i32(Key::PRELOAD_PANELS), 0);
        let mut ini = crate::ini::Ini::default();
        cfg.write_ini(&mut ini);
        assert_eq!(Config::from_ini(&ini).get_i32(Key::PRELOAD_PANELS), 0);
    }

    #[test]
    fn glow_strength_defaults_to_middle_and_round_trips() {
        let mut cfg = Config::default();
        assert_eq!(cfg.get_i32(Key::GLOW_STRENGTH), 50);
        assert_eq!(Key::GLOW_STRENGTH.section(), "widgets");
        cfg.apply_i32(Key::GLOW_STRENGTH, 0);
        assert_eq!(cfg.get_i32(Key::GLOW_STRENGTH), 0);
        let mut ini = crate::ini::Ini::default();
        cfg.write_ini(&mut ini);
        assert_eq!(Config::from_ini(&ini).get_i32(Key::GLOW_STRENGTH), 0);
    }

    #[test]
    fn one_blur_covers_every_surface() {
        let mut cfg = Config::default();
        cfg.apply_i32(Key::Blur, 11);
        assert_eq!(Prefs::from_cfg(&cfg).blur, 11);
        // The per-popup override is a legacy key now; it must not win.
        cfg.apply_i32(Key::WIDGET_BLUR, 4);
        assert_eq!(Prefs::from_cfg(&cfg).blur, 11);
    }
}
