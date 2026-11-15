//! Shared visual style for popup panels.
//! Every rectangle gets at least `R_SMALL` corners; pills use half their short side.

use egui::{Color32, CornerRadius, Painter, Rect, Stroke, StrokeKind};
use nooksill_core::config::panel::{Backdrop, Prefs};
use nooksill_core::config::{Config, Key};
use std::sync::atomic::{AtomicU8, Ordering};

/// Panel prefs, synced from config each frame.
///
/// Panel constructors only take `dark`, so config reaches them via these statics.
static PANEL_OPACITY: AtomicU8 = AtomicU8::new(0);
static PANEL_OVERLAY: AtomicU8 = AtomicU8::new(28);

/// Refresh the panel prefs after a config load or edit.
pub fn sync_prefs(cfg: &Config) {
    crate::widgets::framework::prefs::sync_prefs(cfg);
    crate::frost::set_glow_strength(cfg.get_i32(Key::GLOW_STRENGTH));
    PANEL_OPACITY.store(
        cfg.get_i32(Key::PANEL_OPACITY).clamp(0, 100) as u8,
        Ordering::Relaxed,
    );
    PANEL_OVERLAY.store(
        cfg.get_i32(Key::PANEL_OVERLAY).clamp(0, 100) as u8,
        Ordering::Relaxed,
    );
}

/// Panel visual parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelStyle {
    pub dark: bool,
    /// Whether a frosted backdrop already exists behind the panel.
    ///
    /// Then the base fill becomes a translucent tint so the blur shows through.
    pub frosted: bool,
    /// Resolved background material; `FollowBar` never reaches this field.
    pub material: Backdrop,
    /// User opacity percent; `0` picks an automatic value from `frosted`.
    opacity: u8,
    /// Overlay strength, `0..100`.
    overlay: u8,
}

impl PanelStyle {
    /// Outer panel corner radius.
    pub const R_PANEL: u8 = 18;
    /// Sub-card corner radius.
    pub const R_CARD: u8 = 12;
    /// Input field and progress bar corner radius.
    pub const R_FIELD: u8 = 10;
    /// Smallest radius; no rectangle keeps a sharp corner.
    pub const R_SMALL: u8 = 8;

    /// Builds from config (tests and offscreen renders). Production uses [`Self::current`].
    #[must_use]
    pub fn from_cfg(cfg: &Config, dark: bool, frosted: bool) -> Self {
        Self {
            dark,
            frosted,
            material: Prefs::from_cfg(cfg).material(cfg.bg_mode),
            opacity: cfg.get_i32(Key::PANEL_OPACITY).clamp(0, 100) as u8,
            overlay: cfg.get_i32(Key::PANEL_OVERLAY).clamp(0, 100) as u8,
        }
    }

    /// Reads the current prefs. `frosted` comes from `FrameRenderer::frost_on()`.
    #[must_use]
    pub fn current(dark: bool, frosted: bool) -> Self {
        Self {
            dark,
            frosted,
            material: crate::widgets::framework::prefs::material(),
            opacity: PANEL_OPACITY.load(Ordering::Relaxed),
            overlay: PANEL_OVERLAY.load(Ordering::Relaxed),
        }
    }

    /// Builds with the shipped defaults (offscreen previews).
    #[must_use]
    pub fn preview(dark: bool, frosted: bool) -> Self {
        Self {
            dark,
            frosted,
            material: Backdrop::Frost,
            opacity: 0,
            overlay: 28,
        }
    }

    /// Alpha of the panel base fill.
    ///
    /// Auto mode follows the frost: 78% when blurred so the texture shows
    /// through, 97% otherwise so text does not wash out over black.
    fn alpha(&self) -> u8 {
        let pct = if self.opacity > 0 {
            self.opacity as f32
        } else if self.frosted && self.material == Backdrop::Frost {
            78.0
        } else {
            97.0
        };
        (pct / 100.0 * 255.0).round().clamp(0.0, 255.0) as u8
    }

    /// Panel base fill.
    #[must_use]
    pub fn bg(&self) -> Color32 {
        let a = self.alpha();
        if self.dark {
            Color32::from_rgba_unmultiplied(24, 25, 30, a)
        } else {
            Color32::from_rgba_unmultiplied(246, 247, 250, a)
        }
    }

    /// Hazy overlay painted on top of the frost.
    ///
    /// Keeps the background texture while lifting text contrast; also drawn
    /// without frost so the surface looks the same either way.
    #[must_use]
    pub fn overlay(&self) -> Color32 {
        let a = (self.overlay as f32 / 100.0 * 64.0).round() as u8;
        if self.dark {
            Color32::from_rgba_unmultiplied(12, 14, 20, a)
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, a)
        }
    }

    /// Hairline border around the panel.
    #[must_use]
    pub fn border(&self) -> Color32 {
        if self.dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 30)
        } else {
            Color32::from_rgba_unmultiplied(0, 0, 0, 20)
        }
    }

    /// Primary text color. Light mode must be dark.
    ///
    /// This uses RGBA order; `argbc(a,r,g,b)` takes alpha first.
    #[must_use]
    pub fn ink(&self) -> Color32 {
        if self.dark {
            Color32::from_rgba_unmultiplied(240, 240, 245, 255)
        } else {
            Color32::from_rgba_unmultiplied(28, 28, 34, 255)
        }
    }

    /// Secondary text color.
    #[must_use]
    pub fn ink_sub(&self) -> Color32 {
        if self.dark {
            Color32::from_rgba_unmultiplied(162, 166, 180, 255)
        } else {
            Color32::from_rgba_unmultiplied(100, 104, 118, 255)
        }
    }

    /// Faintest text, e.g. captions beside a divider.
    #[must_use]
    pub fn faint(&self) -> Color32 {
        if self.dark {
            Color32::from_rgba_unmultiplied(200, 200, 210, 255)
        } else {
            Color32::from_rgba_unmultiplied(90, 90, 100, 255)
        }
    }

    /// Divider line color.
    #[must_use]
    pub fn line(&self) -> Color32 {
        if self.dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 38)
        } else {
            Color32::from_rgba_unmultiplied(0, 0, 0, 30)
        }
    }

    /// Sub-card fill, one step lighter than the panel base.
    #[must_use]
    pub fn card(&self) -> Color32 {
        let extra = (self.overlay as f32 / 100.0 * 20.0).round() as u8;
        if self.dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 20u8.saturating_add(extra))
        } else {
            Color32::from_rgba_unmultiplied(0, 0, 0, 14u8.saturating_add(extra / 2))
        }
    }

    /// Sub-card fill while hovered.
    #[must_use]
    pub fn card_hover(&self) -> Color32 {
        if self.dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 58)
        } else {
            Color32::from_rgba_unmultiplied(0, 0, 0, 28)
        }
    }

    /// Pill radius: half the short side, capped at 255 (`CornerRadius` is `u8`).
    #[must_use]
    pub fn pill(h: f32) -> CornerRadius {
        CornerRadius::same((h * 0.5).round().clamp(2.0, 255.0) as u8)
    }

    /// Paints the panel frame: base fill, overlay, then the hairline border.
    pub fn paint_frame(&self, painter: &Painter, rect: Rect) {
        let radius = CornerRadius::same(Self::R_PANEL);
        self.paint_base(painter, rect);
        painter.rect_filled(rect, radius, self.overlay());
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, self.border()),
            StrokeKind::Middle,
        );
    }

    /// Base fill for every material.
    ///
    /// Three panels draw their own overlay and border; they must still call
    /// this so a material switch is not silently ignored there.
    pub fn paint_base(&self, painter: &Painter, rect: Rect) {
        let radius = CornerRadius::same(Self::R_PANEL);
        if self.material != Backdrop::Gradient {
            painter.rect_filled(rect, radius, self.bg());
            return;
        }
        // Flat two-colour fill; the outer corners stay rounded on the end bands.
        const STEPS: i32 = 16;
        let h = rect.height();
        if h <= 0.0 {
            painter.rect_filled(rect, radius, self.bg());
            return;
        }
        let (top, bottom) = if self.dark {
            ((24_u8, 25_u8, 30_u8), (40_u8, 43_u8, 52_u8))
        } else {
            ((246_u8, 247_u8, 250_u8), (224_u8, 228_u8, 238_u8))
        };
        let a = self.bg().a();
        for i in 0..STEPS {
            let t = (f64::from(i) + 0.5) / f64::from(STEPS);
            let mix = |lo: u8, hi: u8| {
                (f64::from(lo) + (f64::from(hi) - f64::from(lo)) * t)
                    .round()
                    .clamp(0.0, 255.0) as u8
            };
            let fill = Color32::from_rgba_unmultiplied(
                mix(top.0, bottom.0),
                mix(top.1, bottom.1),
                mix(top.2, bottom.2),
                a,
            );
            let y0 = rect.top() + h * i as f32 / STEPS as f32;
            let y1 = rect.top() + h * (i + 1) as f32 / STEPS as f32;
            let band =
                Rect::from_min_max(egui::pos2(rect.left(), y0), egui::pos2(rect.right(), y1));
            let corners = CornerRadius {
                nw: if i == 0 { radius.nw } else { 0 },
                ne: if i == 0 { radius.ne } else { 0 },
                se: if i == STEPS - 1 { radius.se } else { 0 },
                sw: if i == STEPS - 1 { radius.sw } else { 0 },
            };
            painter.rect_filled(band, corners, fill);
        }
    }

    /// Paints a pill-shaped element (button, tag, switch border).
    pub fn paint_pill(&self, painter: &Painter, rect: Rect, fill: Color32) {
        painter.rect_filled(rect, Self::pill(rect.height()), fill);
    }

    /// Paints a sub-card.
    pub fn paint_card(&self, painter: &Painter, rect: Rect, hovered: bool) {
        painter.rect_filled(
            rect,
            CornerRadius::same(Self::R_CARD),
            if hovered {
                self.card_hover()
            } else {
                self.card()
            },
        );
    }
}

impl Default for PanelStyle {
    fn default() -> Self {
        Self::preview(true, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ink_is_opaque_and_contrasts_with_its_theme() {
        let light = PanelStyle::preview(false, false);
        let [r, g, b, a] = light.ink().to_srgba_unmultiplied();
        assert_eq!(a, 255);
        assert!(r < 80 && g < 80 && b < 80, "浅色模式主文字必须深色");

        let dark = PanelStyle::preview(true, false);
        let [r, g, b, a] = dark.ink().to_srgba_unmultiplied();
        assert_eq!(a, 255);
        assert!(r > 200 && g > 200 && b > 200, "深色模式主文字必须亮色");

        for s in [&light, &dark] {
            for c in [s.ink_sub(), s.faint()] {
                assert_eq!(c.a(), 255, "文字不该半透明");
            }
        }
    }

    #[test]
    fn frost_softens_the_base_but_user_opacity_wins() {
        let solid = PanelStyle::preview(true, false);
        let frosted = PanelStyle::preview(true, true);
        assert!(frosted.bg().a() < solid.bg().a(), "有磨砂时底色应更透");

        let mut custom = PanelStyle::preview(true, false);
        custom.opacity = 40;
        assert_eq!(custom.bg().a(), 102, "用户值必须覆盖自动值");
        assert!(custom.bg().a() < solid.bg().a());
    }

    #[test]
    fn solid_stays_opaque_when_a_stale_backdrop_is_present() {
        let mut s = PanelStyle::preview(true, true);
        s.material = Backdrop::Solid;
        assert_eq!(s.bg().a(), PanelStyle::preview(true, false).bg().a());
    }

    #[test]
    fn material_resolves_follow_bar_from_config() {
        let mut cfg = Config::default();
        assert_eq!(
            PanelStyle::from_cfg(&cfg, true, false).material,
            Backdrop::Frost
        );
        cfg.apply_i32(Key::BgMode, 0);
        assert_eq!(
            PanelStyle::from_cfg(&cfg, true, false).material,
            Backdrop::Solid
        );
        cfg.apply_i32(Key::BgMode, 1);
        assert_eq!(
            PanelStyle::from_cfg(&cfg, true, false).material,
            Backdrop::Gradient
        );
        cfg.apply_i32(Key::PANEL_BG, 1);
        assert_eq!(
            PanelStyle::from_cfg(&cfg, true, false).material,
            Backdrop::Gradient,
            "PanelBg is a legacy key; the bar material decides"
        );
    }

    #[test]
    fn every_radius_is_rounded_and_pill_is_half_height() {
        const {
            assert!(PanelStyle::R_PANEL > 0);
            assert!(PanelStyle::R_CARD > 0);
            assert!(PanelStyle::R_FIELD > 0);
            assert!(PanelStyle::R_SMALL > 0);
        }
        assert_eq!(PanelStyle::pill(30.0).nw, 15);
        assert_eq!(PanelStyle::pill(1000.0).nw, 255, "上限要夹住 u8");
    }
}
