//! Prefab icons for built-in item kinds.

use super::canvas::pix;
use super::*;

/// Paints a built-in Dock icon filling `rect` (logical points).
///
/// Start uses `style.accent`; Launchpad lights up to nine dots by
/// `state.level`, which is the hook for animated icons.
pub fn paint_tile(painter: &Painter, rect: EguiRect, kind: Builtin, style: IconStyle, dark: bool) {
    let t = rect.shrink(rect.width() * 0.04);
    let s = t.width().min(t.height());
    let radius = CornerRadius::same(pix(s * 0.26));
    match kind {
        Builtin::Start => {
            painter.rect_filled(t, radius, style.accent);
            // A 2x2 grid of rounded white windows; gaps and radius scale with s.
            let q = s * 0.27;
            let gp = s * 0.07;
            let o = (s - 2.0 * q - gp) / 2.0;
            let qc = CornerRadius::same(pix(q * 0.22));
            for i in 0..4 {
                let x = t.left() + o + (i % 2) as f32 * (q + gp);
                let y = t.top() + o + (i / 2) as f32 * (q + gp);
                painter.rect_filled(
                    EguiRect::from_min_size(Pos2::new(x, y), egui::vec2(q, q)),
                    qc,
                    Color32::WHITE,
                );
            }
        }
        Builtin::Launchpad => {
            painter.rect_filled(
                t,
                radius,
                if dark {
                    Color32::from_rgb(46, 48, 58)
                } else {
                    Color32::from_rgb(247, 248, 252)
                },
            );
            // Dots instead of squares: the grid keeps no sharp corners.
            const COLS: [Color32; 9] = [
                Color32::from_rgb(238, 108, 100),
                Color32::from_rgb(240, 168, 76),
                Color32::from_rgb(104, 196, 112),
                Color32::from_rgb(80, 160, 238),
                Color32::from_rgb(168, 124, 222),
                Color32::from_rgb(238, 146, 96),
                Color32::from_rgb(100, 198, 220),
                Color32::from_rgb(234, 116, 152),
                Color32::from_rgb(126, 202, 148),
            ];
            let step = s * 0.265;
            let r = s * 0.075;
            let lit = (style.state.level.clamp(0.0, 1.0) * 9.0).round() as usize;
            for (i, col) in COLS.iter().enumerate() {
                let x = t.center().x + ((i % 3) as f32 - 1.0) * step;
                let y = t.center().y + ((i / 3) as f32 - 1.0) * step;
                let c = if i < lit {
                    *col
                } else {
                    col.linear_multiply(0.35)
                };
                painter.circle_filled(Pos2::new(x, y), r, c);
            }
        }
        Builtin::Notes => {
            // A sheet: accent base, white note, three ruled lines. Real text
            // would blur at small sizes.
            painter.rect_filled(t, radius, style.accent);
            let pad = s * 0.24;
            let paper = EguiRect::from_min_size(
                Pos2::new(t.left() + pad, t.top() + pad),
                egui::vec2(s - 2.0 * pad, s - 2.0 * pad),
            );
            painter.rect_filled(paper, CornerRadius::same(pix(s * 0.055)), Color32::WHITE);
            let x0 = paper.left() + paper.width() * 0.20;
            let line_w = paper.width() * 0.60;
            for k in 0..3 {
                let y = paper.top() + paper.height() * (0.30 + k as f32 * 0.22);
                painter.line_segment(
                    [Pos2::new(x0, y), Pos2::new(x0 + line_w, y)],
                    Stroke::new(s * 0.035, style.accent),
                );
            }
        }
        // Per-kind fallback icons share one visual language: accent rounded
        // base, white glyph, only circles, rounded rects and segments.
        Builtin::Folder => {
            painter.rect_filled(t, radius, style.accent);
            let pad = s * 0.22;
            let tab = EguiRect::from_min_size(
                Pos2::new(t.left() + pad, t.top() + pad * 0.85),
                egui::vec2(s * 0.34, s * 0.18),
            );
            painter.rect_filled(tab, CornerRadius::same(pix(s * 0.05)), Color32::WHITE);
            let body = EguiRect::from_min_max(
                Pos2::new(t.left() + pad, t.top() + pad * 1.35),
                Pos2::new(t.right() - pad, t.bottom() - pad),
            );
            painter.rect_filled(body, CornerRadius::same(pix(s * 0.06)), Color32::WHITE);
        }
        Builtin::File => {
            painter.rect_filled(t, radius, style.accent);
            let pad = s * 0.24;
            let doc = EguiRect::from_min_size(
                Pos2::new(t.left() + pad, t.top() + pad),
                egui::vec2(s - 2.0 * pad, s - 2.0 * pad),
            );
            painter.rect_filled(doc, CornerRadius::same(pix(s * 0.05)), Color32::WHITE);
            let x0 = doc.left() + doc.width() * 0.20;
            let lw = doc.width() * 0.60;
            for k in 0..3 {
                let y = doc.top() + doc.height() * (0.36 + k as f32 * 0.19);
                painter.line_segment(
                    [Pos2::new(x0, y), Pos2::new(x0 + lw, y)],
                    Stroke::new(s * 0.035, style.accent),
                );
            }
        }
        Builtin::Url => {
            painter.rect_filled(t, radius, style.accent);
            let (cx, cy) = (t.center().x, t.center().y);
            let r = s * 0.27;
            let w = Stroke::new(s * 0.05, Color32::WHITE);
            painter.circle_stroke(Pos2::new(cx, cy), r, w);
            painter.line_segment([Pos2::new(cx - r, cy), Pos2::new(cx + r, cy)], w);
            painter.line_segment([Pos2::new(cx, cy - r), Pos2::new(cx, cy + r)], w);
        }
        Builtin::Group => {
            painter.rect_filled(t, radius, style.accent);
            let q = s * 0.22;
            let gap = s * 0.13;
            let o = (s - 2.0 * q - gap) / 2.0;
            for i in 0..4 {
                let x = t.left() + o + (i % 2) as f32 * (q + gap) + q / 2.0;
                let y = t.top() + o + (i / 2) as f32 * (q + gap) + q / 2.0;
                painter.circle_filled(Pos2::new(x, y), q / 2.0, Color32::WHITE);
            }
        }
        Builtin::Trash => {
            painter.rect_filled(t, radius, style.accent);
            let pad = s * 0.26;
            let lid_y = t.top() + pad * 1.15;
            painter.line_segment(
                [
                    Pos2::new(t.left() + pad * 0.75, lid_y),
                    Pos2::new(t.right() - pad * 0.75, lid_y),
                ],
                Stroke::new(s * 0.055, Color32::WHITE),
            );
            let body = EguiRect::from_min_max(
                Pos2::new(t.left() + pad, lid_y + s * 0.04),
                Pos2::new(t.right() - pad, t.bottom() - pad),
            );
            painter.rect_filled(body, CornerRadius::same(pix(s * 0.05)), Color32::WHITE);
        }
        Builtin::Separator => {
            painter.rect_filled(t, radius, style.accent);
            let (cx, cy) = (t.center().x, t.center().y);
            painter.line_segment(
                [Pos2::new(cx, cy - s * 0.25), Pos2::new(cx, cy + s * 0.25)],
                Stroke::new(s * 0.07, Color32::WHITE),
            );
        }
    }
}

/// Paints a prefab icon for an item kind without reading its name.
///
/// Used as fallback when the shell icon is missing; the exhaustive match in
/// `from_kind` forces every new `ItemKind` to get an icon.
pub fn paint_kind(painter: &Painter, rect: EguiRect, kind: ItemKind, style: IconStyle, dark: bool) {
    paint_tile(painter, rect, Builtin::from_kind(kind), style, dark);
}

/// Paints a first-letter placeholder on a neutral field.
pub fn paint_monogram(painter: &Painter, rect: EguiRect, ch: char, style: IconStyle, dark: bool) {
    let s = rect.width().min(rect.height());
    let radius = CornerRadius::same(pix(s * 0.26));
    let bg = if dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, 26)
    } else {
        Color32::from_rgba_unmultiplied(23, 26, 35, 18)
    };
    painter.rect_filled(rect, radius, bg);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(
            1.0,
            if dark {
                Color32::from_rgba_unmultiplied(255, 255, 255, 24)
            } else {
                Color32::from_rgba_unmultiplied(23, 26, 35, 14)
            },
        ),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        ch.to_string(),
        FontId::proportional(s * 0.46),
        style.accent,
    );
}
