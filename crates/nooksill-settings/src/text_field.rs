//! Shared hand-drawn text field shell.
//!
//! `TextEdit` sizes itself from text plus padding and keeps a fixed corner
//! radius, so a bare one looks like two designs at 22px and 30px heights.
//! The shell (fill, focus ring, icon, shortcut, clear button) is painted here
//! and `TextEdit` only edits text, so every window shares one look.

use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Id, Layout, Margin, Rect, Sense, Stroke,
    StrokeKind, Ui,
};

/// Height and corner radius shared by all fields, so curvature stays uniform.
pub const FIELD_H: f32 = 34.0;
pub const FIELD_R: f32 = 12.0;

/// Leading icon.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum FieldIcon {
    #[default]
    None,
    Search,
}

/// Field configuration; the default has no icon, hint or clear button.
#[derive(Copy, Clone, Debug, Default)]
pub struct FieldOpts<'a> {
    /// Placeholder shown while empty.
    pub hint: &'a str,
    pub icon: FieldIcon,
    /// Trailing hint such as `Ctrl K`, replaced by the clear button when set.
    pub trailing: Option<&'a str>,
    /// Shows a clear button while non-empty.
    pub clearable: bool,
}

/// Draw a field shell, returning whether the text changed.
///
/// `rect` is the shell rect (prefer [`FIELD_H`] for its height). `id` must be
/// stable across frames, since shortcuts like Ctrl K request focus by it.
pub fn text_field(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    text: &mut String,
    opts: &FieldOpts<'_>,
) -> bool {
    // Copy the colours out first to end the visuals borrow before interacting.
    let (accent, faint, field, focus_bg) = {
        let v = ui.visuals();
        let dark = v.dark_mode;
        (
            v.selection.stroke.color,
            v.weak_text_color(),
            if dark {
                Color32::from_rgba_unmultiplied(255, 255, 255, 16)
            } else {
                Color32::from_rgba_unmultiplied(23, 26, 35, 14)
            },
            if dark {
                Color32::from_rgb(38, 39, 47)
            } else {
                Color32::WHITE
            },
        )
    };
    let focused = ui.ctx().memory(|m| m.has_focus(id));
    let has_text = !text.is_empty();
    let r = CornerRadius::same(FIELD_R as u8);

    ui.painter()
        .rect_filled(rect, r, if focused { focus_bg } else { field });
    if focused {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            r,
            Stroke::new(2.0, accent.linear_multiply(0.55)),
            StrokeKind::Inside,
        );
    }

    // Must be registered before the right-side buttons: egui hit-tests in
    // registration order, so a later shell would swallow the clear click.
    let shell_resp = ui.interact(rect, id.with("shell"), Sense::click());
    if shell_resp.clicked() {
        ui.memory_mut(|m| m.request_focus(id));
    }

    let cy = rect.center().y;
    let left_pad = match opts.icon {
        FieldIcon::None => 12.0,
        FieldIcon::Search => {
            let cx = rect.left() + 17.0;
            let glass = Stroke::new(1.6, faint);
            ui.painter()
                .circle_stroke(egui::pos2(cx, cy - 1.5), 5.0, glass);
            ui.painter().line_segment(
                [
                    egui::pos2(cx + 3.8, cy + 2.3),
                    egui::pos2(cx + 8.0, cy + 6.5),
                ],
                glass,
            );
            32.0
        }
    };

    let mut right_pad = 12.0;
    let mut cleared = false;
    if opts.clearable && has_text {
        let clear =
            Rect::from_center_size(egui::pos2(rect.right() - 20.0, cy), egui::vec2(22.0, 22.0));
        let clear_resp = ui.interact(clear, id.with("clear"), Sense::click());
        if clear_resp.hovered() {
            ui.painter().rect_filled(
                clear,
                CornerRadius::same(8),
                ui.visuals().selection.bg_fill.linear_multiply(0.75),
            );
        }
        let c = clear.center();
        let d = 3.5;
        let stroke = Stroke::new(1.6, faint);
        ui.painter().line_segment(
            [egui::pos2(c.x - d, c.y - d), egui::pos2(c.x + d, c.y + d)],
            stroke,
        );
        ui.painter().line_segment(
            [egui::pos2(c.x + d, c.y - d), egui::pos2(c.x - d, c.y + d)],
            stroke,
        );
        clear_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "清空输入"));
        if clear_resp.clicked() {
            text.clear();
            cleared = true;
            ui.memory_mut(|m| m.request_focus(id));
        }
        right_pad = 44.0;
    } else if let Some(shortcut) = opts.trailing {
        ui.painter().text(
            egui::pos2(rect.right() - 13.0, cy),
            Align2::RIGHT_CENTER,
            shortcut,
            FontId::proportional(10.5),
            faint,
        );
        right_pad = 58.0;
    }

    // `align` centres the text; `vertical_align` is deprecated in egui 0.37.
    let text_rect = Rect::from_min_max(
        egui::pos2(rect.left() + left_pad, rect.top() + 2.0),
        egui::pos2(rect.right() - right_pad, rect.bottom() - 2.0),
    );
    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(text_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let hint = hint_for(focused, text.is_empty(), opts.hint);
    let resp = inner.add_sized(
        text_rect.size(),
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .id(id)
            .frame(egui::Frame::NONE.inner_margin(Margin::ZERO))
            .align(Align2::LEFT_CENTER)
            .desired_width(text_rect.width()),
    );
    resp.changed() || cleared
}

/// Hide the hint while focused and empty: a zero-margin `TextEdit` puts the
/// caret on top of the hint's first glyph. A pure function since the painted
/// hint is not an AccessKit node and cannot be queried from kittest.
fn hint_for(focused: bool, empty: bool, hint: &str) -> &str {
    if focused && empty {
        ""
    } else {
        hint
    }
}

#[cfg(test)]
mod tests {
    use super::hint_for;

    #[test]
    fn 焦点且空值时收起提示() {
        assert_eq!(hint_for(true, true, "搜索设置…"), "");
        assert_eq!(hint_for(false, true, "搜索设置…"), "搜索设置…");
        assert_eq!(hint_for(true, false, "搜索设置…"), "搜索设置…");
    }
}
