//! Transient overlays drawn outside the bar body.
//! Insert mark, release hint and the notice bubble; all of them come after
//! the bar content in z-order, so they live apart from it.

use super::*;

impl Dock {
    /// Capsule mark in the reserved slot during an external drop.
    pub(super) fn paint_insert_mark(&self, painter: &egui::Painter, p: Proj, dark: bool) {
        let Some(u) = self.geom.insert_l else {
            return;
        };
        let m = &self.metrics;
        let slot_w = m.s + m.gap;
        let mark_w = 4.0;
        let h = m.bar_h * 0.72;
        let top = self.geom.bar_top + (m.bar_h - h) * 0.5;
        let rect = self.map(u + (slot_w - mark_w) * 0.5, top, mark_w, h, p);
        let color = if dark {
            Color32::from_rgba_unmultiplied(120, 190, 255, 235)
        } else {
            Color32::from_rgba_unmultiplied(46, 123, 246, 235)
        };
        painter.rect_filled(rect, CornerRadius::same(3), color);
    }

    /// "Release to remove" hint while dragging outside the bar.
    pub(super) fn paint_drag_hint(&self, painter: &egui::Painter, p: Proj, bar: EguiRect) {
        if !self.drag_outside() {
            return;
        }
        let text = nooksill_core::i18n::t("Release to remove · drag back to cancel");
        let font = FontId::proportional(13.0);
        let galley = painter.layout_no_wrap(text.to_owned(), font, Color32::WHITE);
        let size = galley.size() + egui::vec2(22.0, 12.0);
        let rect = EguiRect::from_center_size(
            egui::pos2(bar.center().x, bar.top() - size.y / 2.0 - 10.0),
            size,
        );
        painter.rect_filled(
            rect,
            CornerRadius::same(9),
            Color32::from_rgba_unmultiplied(150, 48, 48, 236),
        );
        painter.galley(
            egui::Align2::CENTER_CENTER
                .align_size_within_rect(galley.size(), rect)
                .min,
            galley,
            Color32::WHITE,
        );
        let _ = p;
    }

    /// Centered notice bubble above the bar, fading in its last 300 ms.
    pub(super) fn paint_notice(&self, painter: &egui::Painter, p: Proj, bar: EguiRect, dark: bool) {
        let Some((text, at)) = &self.notice else {
            return;
        };
        let alpha = Self::notice_alpha(at.elapsed().as_millis());
        if alpha <= 0.0 {
            return;
        }
        let font = FontId::proportional(12.5);
        let galley = painter.layout_no_wrap(text.clone(), font, Color32::WHITE);
        let pad = egui::vec2(14.0, 8.0);
        let size = galley.size() + pad * 2.0;
        let rect = EguiRect::from_center_size(
            egui::pos2(bar.center().x, bar.top() - size.y / 2.0 - 10.0 * p.k),
            size,
        );
        let a = (alpha * 236.0) as u8;
        let (bg, ink) = if dark {
            (
                Color32::from_rgba_unmultiplied(32, 33, 39, a),
                Color32::from_rgba_unmultiplied(240, 240, 245, a),
            )
        } else {
            (
                Color32::from_rgba_unmultiplied(28, 28, 32, a),
                Color32::from_rgba_unmultiplied(250, 250, 252, a),
            )
        };
        painter.rect_filled(rect, CornerRadius::same(9), bg);
        painter.galley(
            egui::Align2::CENTER_CENTER
                .align_size_within_rect(galley.size(), rect)
                .min,
            galley,
            ink,
        );
    }
}
