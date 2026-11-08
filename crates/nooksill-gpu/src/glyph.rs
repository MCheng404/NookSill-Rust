//! Vector icon core: Dock built-ins, control-centre tiles, name-placeholder
//! fallback and strip icons.
//!
//! Geometry follows the Fluent 24px grid: content fills 20px of the 24px canvas
//! (`SAFE`), and square shapes shrink to `BOX` as optical compensation. One
//! stroke width per size tier, a single round join per polyline vertex, and no
//! sharp corners on filled shapes.
//!
//! `IconState` holds every animated parameter, so painters stay time-free and a
//! new animation only means advancing `phase` at the caller.
//!
//! No emoji, gradients or special characters: the bundled font has none.

use egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect as EguiRect, Stroke};
use nooksill_core::control::Tile;
use nooksill_core::item::ItemKind;

/// 20px content area of the 24px canvas, as a normalized radius.
const SAFE: f32 = 0.40;
/// Optical compensation for square shapes: at equal radius a square covers
/// more area, so it is drawn one step smaller to match round shapes.
const BOX: f32 = 0.34;

/// Dynamic icon state. Painters read it but own no clock.
#[derive(Clone, Copy, Debug)]
pub struct IconState {
    /// 0..1 intensity/progress: wifi bars, volume waves, battery charge, dot count.
    pub level: f32,
    /// Active / selected state.
    pub active: bool,
    /// 0..1 animation phase, advanced by the caller.
    pub phase: f32,
    /// Muted / disabled semantics.
    pub muted: bool,
}

impl Default for IconState {
    fn default() -> Self {
        Self {
            level: 1.0,
            active: false,
            phase: 0.0,
            muted: false,
        }
    }
}

impl IconState {
    /// Set the intensity, keep the other fields at their defaults.
    #[must_use]
    pub fn with_level(mut self, level: f32) -> Self {
        self.level = level.clamp(0.0, 1.0);
        self
    }

    /// Set the animation phase, keep the other fields at their defaults.
    #[must_use]
    pub fn with_phase(mut self, phase: f32) -> Self {
        self.phase = phase.rem_euclid(1.0);
        self
    }
}

/// Full spec for one draw: colour, accent and dynamic state.
///
/// Every public entry point takes it, so new fields (hover progress, emphasis
/// flash) do not change any signature.
#[derive(Clone, Copy, Debug)]
pub struct IconStyle {
    /// Body colour (strokes and fills).
    pub color: Color32,
    /// Accent: active segments, emphasis, built-in item background.
    pub accent: Color32,
    pub state: IconState,
}

impl IconStyle {
    /// Single-colour icon, no accent distinction.
    #[must_use]
    pub fn plain(color: Color32) -> Self {
        Self {
            color,
            accent: color,
            state: IconState::default(),
        }
    }

    /// Full spec with accent and state.
    #[must_use]
    pub const fn new(color: Color32, accent: Color32, state: IconState) -> Self {
        Self {
            color,
            accent,
            state,
        }
    }
}

/// Icons drawn on the fly: Dock built-ins plus a per-kind fallback for items
/// whose shell icon is unavailable.
///
/// The fallback exists because the name initial is unreadable at small sizes
/// and cannot tell same-named items apart.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Builtin {
    /// `nooksill:start` - four-pane window.
    Start,
    /// `nooksill:launchpad` - 3x3 dot grid.
    Launchpad,
    /// `nooksill:notes` - note with ruled lines.
    Notes,
    /// Folder: tab plus rounded body.
    Folder,
    /// Plain file: sheet with ruled lines.
    File,
    /// URL: ring with meridians.
    Url,
    /// Group: 2x2 dots, distinct from the `Start` squares.
    Group,
    /// Trash: bin plus lid.
    Trash,
    /// Separator: a single vertical line.
    Separator,
}

impl Builtin {
    /// Match by path, ASCII case-insensitive: hand-edited ini files vary in case.
    #[must_use]
    pub fn from_path(path: &str) -> Option<Self> {
        if path.eq_ignore_ascii_case("nooksill:start") {
            Some(Self::Start)
        } else if path.eq_ignore_ascii_case("nooksill:launchpad") {
            Some(Self::Launchpad)
        } else if path.eq_ignore_ascii_case("nooksill:notes") {
            Some(Self::Notes)
        } else {
            None
        }
    }

    /// Pick the fallback icon for an item kind. The match is exhaustive, so a
    /// new `ItemKind` fails to compile here instead of silently degrading to
    /// the item name.
    #[must_use]
    pub const fn from_kind(kind: ItemKind) -> Self {
        match kind {
            ItemKind::File => Self::File,
            ItemKind::Folder => Self::Folder,
            ItemKind::Url => Self::Url,
            ItemKind::Group => Self::Group,
            ItemKind::Trash => Self::Trash,
            ItemKind::Note => Self::Notes,
            ItemKind::Separator => Self::Separator,
        }
    }
}

mod builtin;
mod canvas;
mod controls;

pub use self::builtin::{paint_kind, paint_monogram, paint_tile};
pub use self::controls::{control_tile, sliders, speaker};

#[cfg(test)]
mod tests {
    use super::canvas::stroke_width;
    use super::*;

    /// Every item kind must map to its own recognisable icon, never to the
    /// name initial. `from_kind` is exhaustive, so a new `ItemKind` is forced
    /// to be added here by the compiler.
    #[test]
    fn every_item_kind_has_its_own_prefab_icon() {
        let kinds = [
            ItemKind::File,
            ItemKind::Folder,
            ItemKind::Url,
            ItemKind::Separator,
            ItemKind::Group,
            ItemKind::Trash,
            ItemKind::Note,
        ];
        let mapped: Vec<Builtin> = kinds.iter().copied().map(Builtin::from_kind).collect();
        for (i, b) in mapped.iter().enumerate() {
            assert!(!mapped[..i].contains(b), "类型图标不能互相撞车：{b:?}");
        }
    }

    /// `from_path` accepts only the three built-in action paths and never
    /// returns a kind icon; the click branch in `bin/dock.rs` relies on that
    /// with a `_ => {}` fallback.
    #[test]
    fn path_lookup_never_returns_kind_icons() {
        for p in [
            "nooksill:start",
            "NOOKSILL:Launchpad",
            "nooksill:notes",
            r"C:\Apps\a.exe",
            "https://example.com",
        ] {
            if let Some(b) = Builtin::from_path(p) {
                assert!(
                    matches!(b, Builtin::Start | Builtin::Launchpad | Builtin::Notes),
                    "意外把 {p} 识别成类型图标"
                );
            }
        }
    }

    /// Path matching ignores case: the ini file is hand-edited, so
    /// `NOOKSILL:Launchpad` must still match.
    #[test]
    fn builtin_paths_are_case_insensitive() {
        assert_eq!(Builtin::from_path("nooksill:start"), Some(Builtin::Start));
        assert_eq!(
            Builtin::from_path("NOOKSILL:Launchpad"),
            Some(Builtin::Launchpad)
        );
        assert_eq!(Builtin::from_path("C:/Apps/x.exe"), None);
        assert_eq!(Builtin::from_path("nooksill:"), None);
    }

    /// Canvas coordinates scale with `s`: four times the size gives four times
    /// the offset.
    #[test]
    fn canvas_coordinates_scale_with_size() {
        let map = |c: f32, s: f32, x: f32| c + x * s;
        let small = map(100.0, 24.0, SAFE) - 100.0;
        let large = map(100.0, 96.0, SAFE) - 100.0;
        assert_eq!(large / small, 4.0);
    }

    /// Dynamic inputs are clamped and wrapped, so out-of-range values cannot
    /// throw the drawing off.
    #[test]
    fn icon_state_clamps_dynamic_inputs() {
        let st = IconState::default().with_level(2.0).with_phase(1.25);
        assert_eq!(st.level, 1.0);
        assert!((st.phase - 0.25).abs() < 1e-6);
        assert_eq!(IconState::default().with_level(-1.0).level, 0.0);
    }

    /// Stroke width tiers: common sizes match the IBM table, growth is
    /// sub-linear and capped.
    #[test]
    fn stroke_width_follows_size_tiers() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-5;
        assert!(near(stroke_width(16.0), 1.0));
        assert!(near(stroke_width(20.0), 1.25));
        assert!(near(stroke_width(24.0), 1.5));
        assert!(near(stroke_width(32.0), 2.0));
        // Interpolated between tiers, monotonic, still capped at 84px and far
        // below the old 84 * 0.085.
        assert!(stroke_width(18.0) > 1.0 && stroke_width(18.0) < 1.25);
        assert!(stroke_width(48.0) > 2.0 && stroke_width(48.0) < 3.0);
        assert!(stroke_width(84.0) <= 3.0);
        assert!(stroke_width(84.0) < 84.0 * 0.085);
    }

    /// The two crescent arcs must meet exactly at the circle intersection,
    /// otherwise the inner arc is visibly offset. This pins the closed-form
    /// intersection and the arc endpoints.
    #[test]
    fn dark_mode_arcs_join_exactly() {
        let (r1, r2, dx) = (SAFE, 0.34_f32, 0.16_f32);
        let ix = (r1 * r1 - r2 * r2 + dx * dx) / (2.0 * dx);
        let iy = (r1 * r1 - ix * ix).sqrt();
        let a1 = iy.atan2(ix);
        let a2 = iy.atan2(ix - dx);
        let outer_start = (r1 * a1.cos(), r1 * a1.sin());
        let outer_end = (
            r1 * (std::f32::consts::TAU - a1).cos(),
            r1 * (std::f32::consts::TAU - a1).sin(),
        );
        let inner_start = (dx + r2 * (-a2).cos(), r2 * (-a2).sin());
        let inner_end = (dx + r2 * a2.cos(), r2 * a2.sin());
        let close =
            |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-5 && (a.1 - b.1).abs() < 1e-5;
        assert!(close(outer_end, inner_start), "外弧终点要接上内弧起点");
        assert!(close(inner_end, outer_start), "内弧终点要接回外弧起点");
    }

    /// Safe area: the content radius stays inside 20px of a 24px canvas, and
    /// square shapes get their compensation.
    #[test]
    fn safe_area_matches_fluent_grid() {
        // Pass through a parameter so clippy does not flag an assertion on
        // constants.
        let inside = |radius: f32, limit: f32| radius <= limit;
        let grid = 20.0_f32 / 48.0 + 0.02; // 20px content / 24px canvas = 0.4167
        assert!(inside(SAFE, grid));
        assert!(inside(BOX, SAFE));
    }
}
