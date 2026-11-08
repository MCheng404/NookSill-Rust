//! UI accent color: follows the system DWM accent or a user COLORREF.
//!
//! Shared by settings controls and the optional dock tint. It lives in the GPU
//! layer because only this crate reaches both `nooksill_platform` (DWM) and
//! egui's `Visuals`.

use egui::{Color32, CornerRadius, Ui};
use nooksill_core::color;
use nooksill_core::config::{Config, Key};

/// Resolve the active accent as RGB 0..255. `None` means the theme default.
///
/// `UiAccent >= 0` is a user COLORREF, used verbatim. `-1` follows the system:
/// DWM luminance varies wildly, so the max channel is scaled to 190 to keep the
/// hue readable on light backgrounds. Near-grey colors fall back to `None`.
#[must_use]
pub fn resolve(cfg: &Config) -> Option<[u8; 3]> {
    let v = cfg.get_i32(Key::UI_ACCENT);
    if v >= 0 {
        let c = color::from_colorref(v as u32);
        return Some([color::red(c), color::green(c), color::blue(c)]);
    }
    let raw = nooksill_platform::theme::accent_raw()?;
    let max = raw.iter().copied().max()? as f32;
    let min = raw.iter().copied().min()? as f32;
    if max < 1.0 || max - min < 8.0 {
        return None;
    }
    let k = 190.0 / max;
    Some([
        (raw[0] as f32 * k).min(255.0) as u8,
        (raw[1] as f32 * k).min(255.0) as u8,
        (raw[2] as f32 * k).min(255.0) as u8,
    ])
}

/// Push the accent into an egui `Visuals` and unify control corner radii.
///
/// Uses `Ui::visuals_mut` rather than `Context::set_visuals`: the latter marks
/// the style dirty and forces a repaint every frame, violating the idle
/// no-submit contract. This one only affects the current Ui and its children.
pub fn apply_visuals(ui: &mut Ui, cfg: &Config) {
    let Some(rgb) = resolve(cfg) else { return };
    let accent = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);

    let vis = ui.visuals_mut();
    // Selection and slider fill at 45% alpha. The slider track consumes this
    // value undiluted, so it must stay visible; large fills in widgets.rs
    // (row_hover, nav_item, hint_button) dim it further on their own.
    vis.selection.bg_fill = accent.linear_multiply(0.45);
    vis.selection.stroke.color = accent;
    vis.widgets.active.bg_fill = accent.linear_multiply(0.85);
    vis.widgets.active.weak_bg_fill = accent.linear_multiply(0.16);
    vis.widgets.hovered.weak_bg_fill = accent.linear_multiply(0.10);
    vis.hyperlink_color = accent;
    // Sliders draw no trailing fill unless this is set; without it the accent
    // never shows on the track.
    vis.slider_trailing_fill = true;

    // egui's dark default is `gray(140)` (~4.5:1 on cards); the frosted
    // backdrop adds another stop and drops it to ~3:1. Only `noninteractive`
    // is raised: other states sit on accent fills or are colored by widgets,
    // and `weak_text_color()` derives from this at 60%.
    vis.widgets.noninteractive.fg_stroke.color = if vis.dark_mode {
        Color32::from_rgb(232, 234, 240)
    } else {
        Color32::from_rgb(28, 32, 38)
    };

    // Unify corner radius: 10 for interactive controls, 12 for popups. egui's
    // 2-4px default clashes with the 18px cards.
    let r = CornerRadius::same(10);
    vis.widgets.noninteractive.corner_radius = r;
    vis.widgets.inactive.corner_radius = r;
    vis.widgets.hovered.corner_radius = r;
    vis.widgets.active.corner_radius = r;
    vis.widgets.open.corner_radius = r;
    vis.menu_corner_radius = CornerRadius::same(12);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A custom accent parses as COLORREF (0x00BBGGRR) and is used verbatim.
    #[test]
    fn 自定义主题色按_colorref_解析() {
        let mut cfg = Config::defaults();
        cfg.apply_i32(Key::UI_ACCENT, 0x0088_4422);
        assert_eq!(resolve(&cfg), Some([0x22, 0x44, 0x88]));

        // Out-of-range values clamp instead of falling back to follow-system.
        cfg.apply_i32(Key::UI_ACCENT, 0x00FF_FFFF);
        assert_eq!(resolve(&cfg), Some([0xFF, 0xFF, 0xFF]));
    }
}
