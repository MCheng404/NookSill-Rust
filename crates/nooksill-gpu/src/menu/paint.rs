//! Menu colours, painting and text measurement.

use super::*;

/// Menu palette, one place per theme.
pub(super) struct Colors {
    bg: Color32,
    border: Color32,
    ink: Color32,
    danger: Color32,
    hover: Color32,
    sep: Color32,
}

impl Colors {
    fn for_theme(dark: bool) -> Self {
        if dark {
            Self {
                bg: Color32::from_rgba_unmultiplied(30, 30, 34, 244),
                border: Color32::from_rgba_unmultiplied(255, 255, 255, 34),
                ink: Color32::from_rgba_unmultiplied(240, 240, 245, 255),
                danger: Color32::from_rgba_unmultiplied(255, 116, 116, 255),
                hover: Color32::from_rgba_unmultiplied(255, 255, 255, 30),
                sep: Color32::from_rgba_unmultiplied(255, 255, 255, 30),
            }
        } else {
            Self {
                bg: Color32::from_rgba_unmultiplied(250, 250, 252, 248),
                border: Color32::from_rgba_unmultiplied(0, 0, 0, 26),
                ink: Color32::from_rgba_unmultiplied(28, 28, 32, 255),
                danger: Color32::from_rgba_unmultiplied(198, 40, 40, 255),
                hover: Color32::from_rgba_unmultiplied(0, 0, 0, 20),
                sep: Color32::from_rgba_unmultiplied(0, 0, 0, 22),
            }
        }
    }
}

impl Menu {
    /// Paints in the `ui` logical space; (0,0) is the menu window origin.
    ///
    /// The size must match [`Menu::logical_size`] and [`Menu::hit`] exactly,
    /// or the menu is drawn where it is not clickable.
    pub fn paint(&self, ui: &egui::Ui, dark: bool, frost: bool) {
        let (w, h) = self.logical_size();
        let c = Colors::for_theme(dark);
        let painter = ui.painter();
        let rect = EguiRect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, h));
        let radius = CornerRadius::same(RADIUS as u8);

        painter.rect_filled(
            rect,
            radius,
            if frost {
                crate::frost::panel_bg(dark, true)
            } else {
                c.bg
            },
        );
        painter.rect_stroke(rect, radius, Stroke::new(1.0, c.border), StrokeKind::Inside);

        let mut top = PAD_Y;
        let gutter = self.gutter();
        for (i, e) in self.entries.iter().enumerate() {
            if self.hover == Some(i) {
                let hr = EguiRect::from_min_size(egui::pos2(4.0, top), egui::vec2(w - 8.0, ITEM_H));
                painter.rect_filled(hr, CornerRadius::same(6), c.hover);
            }
            if e.checked {
                // The tick is drawn, not typed: U+2713 lives in Dingbats and
                // is missing from our font, which would render a tofu box.
                let cy = top + ITEM_H / 2.0;
                let x0 = PAD_X + 2.0;
                painter.line_segment(
                    [egui::pos2(x0, cy), egui::pos2(x0 + 4.0, cy + 4.0)],
                    Stroke::new(1.6, c.ink),
                );
                painter.line_segment(
                    [
                        egui::pos2(x0 + 4.0, cy + 4.0),
                        egui::pos2(x0 + 11.0, cy - 4.5),
                    ],
                    Stroke::new(1.6, c.ink),
                );
            }
            painter.text(
                egui::pos2(PAD_X + gutter, top + ITEM_H / 2.0),
                Align2::LEFT_CENTER,
                &e.label,
                FontId::proportional(13.0),
                if e.danger { c.danger } else { c.ink },
            );
            top += ITEM_H;
            if e.sep_after {
                let y = top + SEP_H / 2.0;
                painter.line_segment(
                    [egui::pos2(PAD_X, y), egui::pos2(w - PAD_X, y)],
                    Stroke::new(1.0, c.sep),
                );
                top += SEP_H;
            }
        }
    }
}

/// Estimates the pixel width of one label at `FontId::proportional(13.0)`.
///
/// Approximate on purpose: exact measurement needs a font handle, and this
/// must stay runnable in unit tests.
pub(super) fn estimate_width(s: &str) -> f32 {
    let mut w = 0.0;
    for c in s.chars() {
        // CJK and full-width characters are 13 px, the rest 7 px; the menus
        // mix Latin and CJK labels.
        w += if (c as u32) >= 0x2E80 { 13.0 } else { 7.0 };
    }
    w
}
