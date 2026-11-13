//! Live Dock preview: a pure-egui sketch driven by the current Config.
//!
//! Every frame re-reads the config, so moving a slider changes the sketch in
//! the same frame. Length keys are logical pixels; the preview scales them to
//! fit its canvas instead of applying a DPI factor a second time.

use egui::{Align2, Color32, CornerRadius, FontId, Rect, Stroke, Ui, Vec2};
use nooksill_core::color;
use nooksill_core::config::{BgMode, Config, DockTheme, Edge, Key};

/// Total height of the caption row, canvas and trailing gap.
const H: f32 = 158.0;
/// Height of the caption label above the canvas.
const CAPTION_H: f32 = 22.0;
/// Room above the bar for the hover name bubble.
const LABEL_ROOM: f32 = 26.0;
/// How many sample icons the sketch holds.
const ICONS: usize = 7;
/// Inset from the canvas edges.
const INSET: f32 = 14.0;

/// Geometry in canvas points after fitting the configured bar into the canvas.
struct Geom {
    icon: f32,
    gap: f32,
    pad: f32,
    radius: f32,
    bar: Rect,
    vertical: bool,
}

/// Draw the preview into a fixed-height canvas.
pub(super) fn preview(cfg: &Config, ui: &mut Ui) {
    let width = ui.available_width().max(1.0);
    // A real label, not painter text: screen readers and kittest can see it.
    ui.label(
        egui::RichText::new(nooksill_core::i18n::t("Live preview"))
            .small()
            .strong(),
    );
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(width, H - CAPTION_H - 10.0), egui::Sense::hover());
    let painter = ui.painter();
    let dark = match cfg.theme {
        DockTheme::Light => false,
        DockTheme::Dark => true,
        DockTheme::System => ui.visuals().dark_mode,
    };

    painter.rect_filled(rect, CornerRadius::same(16), desktop(dark));
    wallpaper(painter, rect, dark, cfg.bg_mode == BgMode::Frost);

    let geom = geometry(cfg, rect);
    paint_bar(painter, cfg, &geom, dark);
    paint_icons(painter, cfg, &geom, dark);
    if cfg.reflect && !geom.vertical {
        paint_reflection(painter, cfg, &geom);
    }
    if cfg.get_i32(Key::Labels) != 0 {
        paint_label(painter, &geom, dark);
    }
    // Drawn last so the rounded corner stays visible over the wallpaper.
    painter.rect_stroke(
        rect,
        CornerRadius::same(16),
        Stroke::new(
            1.0,
            if dark {
                Color32::from_rgba_unmultiplied(255, 255, 255, 22)
            } else {
                Color32::from_rgba_unmultiplied(23, 26, 35, 18)
            },
        ),
        egui::StrokeKind::Inside,
    );
    ui.add_space(10.0);
}

/// A neutral wallpaper with soft colour blobs; a frosted bar reads against it.
fn desktop(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(24, 27, 36)
    } else {
        Color32::from_rgb(226, 231, 240)
    }
}

fn wallpaper(painter: &egui::Painter, rect: Rect, dark: bool, frost: bool) {
    let blobs: [(f32, f32, f32, Color32); 3] = [
        (
            0.24,
            0.62,
            0.34,
            Color32::from_rgba_unmultiplied(75, 104, 190, if dark { 70 } else { 60 }),
        ),
        (
            0.72,
            0.36,
            0.30,
            Color32::from_rgba_unmultiplied(190, 96, 140, if dark { 60 } else { 52 }),
        ),
        (
            0.52,
            0.86,
            0.26,
            Color32::from_rgba_unmultiplied(70, 160, 150, if dark { 52 } else { 44 }),
        ),
    ];
    for (fx, fy, fr, c) in blobs {
        let r = rect.width().min(rect.height()) * fr;
        let center = rect.min + Vec2::new(rect.width() * fx, rect.height() * fy);
        // Concentric alpha rings approximate a defocused highlight; egui has
        // no radial gradient primitive.
        for step in (1..=6).rev() {
            let t = step as f32 / 6.0;
            painter.circle_filled(
                center,
                r * t,
                Color32::from_rgba_unmultiplied(
                    c.r(),
                    c.g(),
                    c.b(),
                    (c.a() as f32 * (1.0 - t) * 0.24).round() as u8,
                ),
            );
        }
    }
    if frost {
        painter.rect_filled(
            rect,
            CornerRadius::same(16),
            Color32::from_rgba_unmultiplied(255, 255, 255, if dark { 6 } else { 26 }),
        );
    }
}

/// Fit the configured bar into the canvas, then clamp every derived length to
/// a non-negative value before it reaches `clamp` or a rectangle constructor.
fn geometry(cfg: &Config, rect: Rect) -> Geom {
    let icon = (cfg.get_i32(Key::IconSize) as f32).max(0.0);
    let gap = (cfg.get_i32(Key::Spacing) as f32).max(0.0);
    let pad = (cfg.get_i32(Key::Padding) as f32).max(0.0);
    let radius = (cfg.get_i32(Key::Radius) as f32).max(0.0);
    let vertical = matches!(cfg.edge, Edge::Left | Edge::Right);

    let span = ICONS as f32 * icon + (ICONS - 1) as f32 * gap + 2.0 * pad;
    let avail = if vertical {
        (rect.height() - LABEL_ROOM - 16.0).max(1.0)
    } else {
        (rect.width() - 2.0 * INSET - 16.0).max(1.0)
    };
    let scale = (avail / span.max(1.0)).clamp(0.05, 1.0);
    let icon = icon * scale;
    let gap = gap * scale;
    let pad = pad * scale;
    let radius = (radius * scale).min(icon * 0.5);
    let bar_w = ICONS as f32 * icon + (ICONS - 1) as f32 * gap + 2.0 * pad;
    let bar_h = icon + 2.0 * pad;
    let (w, h) = if vertical {
        (bar_h, bar_w)
    } else {
        (bar_w, bar_h)
    };

    // Reserve room for the mirrored row: without it the reflection would be
    // painted onto the cards below the canvas.
    let mirror = if cfg.reflect && !vertical {
        icon * (cfg.get_i32(Key::REFL_SIZE) as f32 / 100.0).clamp(0.1, 1.0)
            + (cfg.get_i32(Key::REFL_GAP) as f32).max(0.0)
    } else {
        0.0
    };
    let free = (rect.height() - LABEL_ROOM - h - mirror).max(0.0);
    let bias = match cfg.edge {
        Edge::Top => 0.08,
        Edge::Bottom => 0.72,
        _ => 0.42,
    };
    let bar_top = rect.top() + LABEL_ROOM + free * bias;
    let x = match cfg.edge {
        Edge::Left => rect.left() + INSET + w * 0.5,
        Edge::Right => rect.right() - INSET - w * 0.5,
        _ => rect.center().x,
    };
    Geom {
        icon,
        gap,
        pad,
        radius,
        bar: Rect::from_center_size(
            egui::pos2(x, bar_top + h * 0.5),
            Vec2::new(w.max(1.0), h.max(1.0)),
        ),
        vertical,
    }
}

/// How far a vertical strip at `p` must be inset to stay inside a rounded end.
fn round_inset(p: f32, lo: f32, hi: f32, r: f32) -> f32 {
    if r <= 0.0 {
        return 0.0;
    }
    let d = (p - lo).min(hi - p).max(0.0);
    if d >= r {
        return 0.0;
    }
    r - (r * r - (r - d) * (r - d)).max(0.0).sqrt()
}

/// Bar plate: solid, two-stop gradient or a frosted wash, with optional edge.
fn paint_bar(painter: &egui::Painter, cfg: &Config, g: &Geom, dark: bool) {
    let alpha = ((cfg.get_i32(Key::Opacity) as f32 / 100.0).clamp(0.0, 1.0) * 255.0).round() as u8;
    let a = custom_or(
        cfg.get_i32(Key::BgColor1),
        if dark { (42, 45, 56) } else { (245, 246, 250) },
    );
    let b = custom_or(
        cfg.get_i32(Key::BgColor2),
        if dark { (64, 68, 84) } else { (214, 220, 232) },
    );
    let radius = CornerRadius::same(g.radius.round() as u8);

    match cfg.bg_mode {
        BgMode::Solid => {
            painter.rect_filled(
                g.bar,
                radius,
                Color32::from_rgba_unmultiplied(a.0, a.1, a.2, alpha),
            );
        }
        BgMode::Gradient => {
            let steps = (g.bar.width().max(g.bar.height()) / 2.0)
                .round()
                .clamp(1.0, 200.0) as usize;
            let along_x = !g.vertical;
            let total = if along_x {
                g.bar.width()
            } else {
                g.bar.height()
            };
            let step = total / steps as f32;
            for i in 0..steps {
                let t = i as f32 / steps as f32;
                let c = lerp(a, b, t);
                let p0 = t * total;
                let p1 = p0 + step;
                // Strips shrink towards both rounded ends, so no square corner
                // leaks outside the bar's radius.
                let lo = if along_x { g.bar.left() } else { g.bar.top() };
                let inset = round_inset(p0, 0.0, total, g.radius)
                    .max(round_inset(p1, 0.0, total, g.radius));
                let strip = if along_x {
                    Rect::from_min_max(
                        egui::pos2(lo + p0, g.bar.top() + inset),
                        egui::pos2(lo + p1 + 1.0, g.bar.bottom() - inset),
                    )
                } else {
                    Rect::from_min_max(
                        egui::pos2(g.bar.left() + inset, lo + p0),
                        egui::pos2(g.bar.right() - inset, lo + p1 + 1.0),
                    )
                };
                painter.rect_filled(
                    strip,
                    CornerRadius::same(0),
                    Color32::from_rgba_unmultiplied(c.0, c.1, c.2, alpha),
                );
            }
        }
        BgMode::Frost => {
            painter.rect_filled(
                g.bar,
                radius,
                Color32::from_rgba_unmultiplied(a.0, a.1, a.2, (alpha as f32 * 0.72).round() as u8),
            );
            painter.rect_filled(
                g.bar.shrink(2.0),
                radius,
                Color32::from_rgba_unmultiplied(255, 255, 255, if dark { 12 } else { 40 }),
            );
        }
    }
    if cfg.get_i32(Key::Border) != 0 {
        painter.rect_stroke(
            g.bar,
            radius,
            Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(255, 255, 255, if dark { 46 } else { 90 }),
            ),
            egui::StrokeKind::Inside,
        );
    }
}

/// Sample icons, with the middle one magnified when hover zoom is on.
fn paint_icons(painter: &egui::Painter, cfg: &Config, g: &Geom, dark: bool) {
    let plate = cfg.get_i32(Key::PLATE_MODE);
    let plate_alpha = if plate == 2 { 150 } else { 235 };
    let dot_style = cfg.get_i32(Key::DotStyle);
    let dots = cfg.get_i32(Key::Dots) != 0;
    let zoom_on = cfg.get_i32(Key::Zoom) != 0;
    let zoom_ratio = (cfg.get_i32(Key::ZoomSize) as f32 / cfg.get_i32(Key::IconSize).max(1) as f32)
        .clamp(1.0, 2.2);

    for i in 0..ICONS {
        let size = if zoom_on && i == ICONS / 2 {
            g.icon * zoom_ratio
        } else {
            g.icon
        };
        let along = i as f32 * (g.icon + g.gap) + g.pad + g.icon * 0.5;
        let center = if g.vertical {
            g.bar.min + Vec2::new(g.bar.width() * 0.5, along)
        } else {
            g.bar.min + Vec2::new(along, g.bar.height() * 0.5)
        };
        let icon_rect = Rect::from_center_size(center, Vec2::splat(size.max(1.0)));
        if plate != 0 {
            painter.rect_filled(
                icon_rect,
                CornerRadius::same((size * (cfg.plate_radius as f32 / 100.0)).round() as u8),
                Color32::from_rgba_unmultiplied(
                    if dark { 250 } else { 255 },
                    if dark { 250 } else { 255 },
                    if dark { 252 } else { 255 },
                    plate_alpha,
                ),
            );
        }
        let (r, gr, b) = palette(i, cfg.get_i32(Key::UI_ACCENT));
        painter.rect_filled(
            icon_rect.shrink((size * 0.10).max(0.5)),
            CornerRadius::same((size * 0.22).round() as u8),
            Color32::from_rgb(r, gr, b),
        );
        if dots && i % 2 == 0 {
            let below = if g.vertical {
                icon_rect.right_center() + Vec2::new(3.0, 0.0)
            } else {
                icon_rect.center() + Vec2::new(0.0, g.bar.height() * 0.5 - 8.0)
            };
            let c = if dark {
                Color32::from_rgb(235, 238, 245)
            } else {
                Color32::from_rgb(48, 52, 62)
            };
            if dot_style == 0 {
                painter.rect_filled(
                    Rect::from_center_size(below, Vec2::new((size * 0.22).max(3.0), 2.0)),
                    CornerRadius::same(1),
                    c,
                );
            } else {
                painter.circle_filled(below, (size * 0.055).max(1.5), c);
            }
        }
    }
}

/// A low-opacity mirrored row under the bar; only the bottom edge supports it.
fn paint_reflection(painter: &egui::Painter, cfg: &Config, g: &Geom) {
    let strength = (cfg.get_i32(Key::REFL_OPACITY) as f32 / 100.0).clamp(0.0, 1.0);
    let size = g.icon * (cfg.get_i32(Key::REFL_SIZE) as f32 / 100.0).clamp(0.1, 1.0);
    let gap = (cfg.get_i32(Key::REFL_GAP) as f32).max(0.0);
    let alpha = (strength * 90.0).round() as u8;
    for i in 0..ICONS {
        let along = i as f32 * (g.icon + g.gap) + g.pad + g.icon * 0.5;
        let center = g.bar.min + Vec2::new(along, g.bar.height() + gap + size * 0.5);
        let (r, gr, b) = palette(i, -1);
        painter.rect_filled(
            Rect::from_center_size(center, Vec2::new(g.icon * 0.8, size)),
            CornerRadius::same((g.icon * 0.18).round() as u8),
            Color32::from_rgba_unmultiplied(r, gr, b, alpha),
        );
    }
}

/// Hover name bubble above the magnified icon.
fn paint_label(painter: &egui::Painter, g: &Geom, dark: bool) {
    let above = g.bar.min + Vec2::new(g.bar.width() * 0.5, -12.0);
    let box_w = (g.icon * 1.6).clamp(48.0, 96.0);
    let r = Rect::from_center_size(above, Vec2::new(box_w, 22.0));
    painter.rect_filled(
        r,
        CornerRadius::same(8),
        if dark {
            Color32::from_rgba_unmultiplied(48, 50, 60, 235)
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, 240)
        },
    );
    painter.text(
        r.center(),
        Align2::CENTER_CENTER,
        nooksill_core::i18n::t("Preview"),
        FontId::proportional(10.0),
        if dark {
            Color32::from_rgb(235, 238, 245)
        } else {
            Color32::from_rgb(40, 44, 54)
        },
    );
}

fn custom_or(value: i32, fallback: (u8, u8, u8)) -> (u8, u8, u8) {
    if value < 0 {
        return fallback;
    }
    let c = color::from_colorref(value as u32);
    (color::red(c), color::green(c), color::blue(c))
}

fn lerp(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

fn palette(i: usize, accent: i32) -> (u8, u8, u8) {
    const BASE: [(u8, u8, u8); 7] = [
        (72, 133, 232),
        (232, 118, 92),
        (104, 186, 140),
        (180, 128, 224),
        (230, 182, 84),
        (92, 190, 210),
        (216, 112, 156),
    ];
    if i == 0 && accent >= 0 {
        let c = color::from_colorref(accent as u32);
        return (color::red(c), color::green(c), color::blue(c));
    }
    BASE[i % BASE.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas() -> Rect {
        Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(600.0, H - CAPTION_H - 10.0))
    }

    #[test]
    fn round_inset_only_shrinks_near_the_ends() {
        assert!((round_inset(0.0, 0.0, 100.0, 10.0) - 10.0).abs() < 1e-3);
        assert!((round_inset(100.0, 0.0, 100.0, 10.0) - 10.0).abs() < 1e-3);
        assert_eq!(round_inset(50.0, 0.0, 100.0, 10.0), 0.0);
        assert_eq!(round_inset(5.0, 0.0, 100.0, 0.0), 0.0);
    }

    /// Reflection, hover bubble and the bar itself must stay inside the canvas
    /// for every material, edge and reflection switch; otherwise the sketch
    /// paints over the settings cards below it.
    #[test]
    fn bar_bubble_and_reflection_never_leave_the_canvas() {
        let rect = canvas();
        for (bg, edge, reflect) in [
            (BgMode::Frost, Edge::Bottom, false),
            (BgMode::Gradient, Edge::Bottom, true),
            (BgMode::Solid, Edge::Top, true),
            (BgMode::Frost, Edge::Left, false),
            (BgMode::Frost, Edge::Right, false),
        ] {
            let mut cfg = Config::defaults();
            cfg.bg_mode = bg;
            cfg.edge = edge;
            cfg.reflect = reflect;
            let g = geometry(&cfg, rect);
            assert!(g.bar.min.x >= rect.min.x - 0.5, "{bg:?} {edge:?} left");
            assert!(g.bar.max.x <= rect.max.x + 0.5, "{bg:?} {edge:?} right");
            assert!(g.bar.min.y >= rect.min.y + 22.0, "{bg:?} {edge:?} bubble");
            assert!(g.bar.max.y <= rect.max.y, "{bg:?} {edge:?} bottom");
            if reflect && !g.vertical {
                let size = g.icon * (cfg.get_i32(Key::REFL_SIZE) as f32 / 100.0).clamp(0.1, 1.0);
                let gap = (cfg.get_i32(Key::REFL_GAP) as f32).max(0.0);
                assert!(
                    g.bar.max.y + gap + size <= rect.max.y + 0.5,
                    "{bg:?} {edge:?} reflection spills out"
                );
            }
        }
    }
}
