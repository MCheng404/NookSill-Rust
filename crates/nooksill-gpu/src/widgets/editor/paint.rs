//! Edit dialog painter: fields, buttons and previews.

use super::layout::field_height;
use super::win::EditState;
use super::*;

/// Offscreen entry point (`examples/panels_shot.rs`).
///
/// Screenshots must use the real window's painting code. This builds no
/// Win32 window and runs no modal loop, and `focused` is forced to true so
/// the offscreen render does not register anything in egui's focus table.
pub fn paint_preview(
    ui: &mut egui::Ui,
    req: &EditRequest,
    values: &[String],
    dark: bool,
    frosted: bool,
) {
    let mut st = EditState {
        multiline: req.fields.iter().any(|f| f.multiline),
        req: req.clone(),
        values: values.to_vec(),
        done: false,
        accepted: false,
        ppp: 1.0,
        dark,
        frosted,
        input: Input::default(),
        ime: Ime::new(),
        focused: true,
        caret: (0.0, 0.0),
    };
    paint(ui, &mut st);
}

/// Paints the dialog: title, input fields, accept and cancel.
pub(super) fn paint(ui: &mut egui::Ui, st: &mut EditState) {
    let rect = ui.max_rect();
    let dark = st.dark;
    // Same base fill, overlay and text tiers as the other popup panels.
    let style = PanelStyle::current(dark, st.frosted);
    let (bg, border, ink, faint) = (style.bg(), style.border(), style.ink(), style.ink_sub());

    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(PanelStyle::R_PANEL), bg);
    painter.rect_filled(
        rect,
        CornerRadius::same(PanelStyle::R_PANEL),
        style.overlay(),
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(PanelStyle::R_PANEL),
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );

    // Title
    painter.text(
        egui::pos2(rect.left() + PAD, rect.top() + PAD),
        Align2::LEFT_TOP,
        &st.req.title,
        FontId::proportional(16.0),
        ink,
    );

    // Fields
    //
    // `painter` is an immutable borrow of `ui` while `text_field` needs
    // `&mut Ui`. The borrow checker ends the borrow at the last use, so the
    // title must be painted before the field loop and field labels take a
    // fresh `ui.painter()`.
    let mut y = rect.top() + PAD + TITLE_H;
    for (i, field) in st.req.fields.iter().enumerate() {
        ui.painter().text(
            egui::pos2(rect.left() + PAD, y),
            Align2::LEFT_TOP,
            &field.label,
            FontId::proportional(12.0),
            faint,
        );
        let h = if field.multiline { MULTI_H } else { FIELD_H };
        let fr = EguiRect::from_min_size(
            egui::pos2(rect.left() + PAD, y + LABEL_H),
            egui::vec2(rect.width() - PAD * 2.0, h),
        );
        let id = Id::unique(("edit-field", i));
        // The first field takes focus automatically so typing works right
        // after the dialog opens.
        if !st.focused {
            ui.memory_mut(|m| m.request_focus(id));
            st.focused = true;
        }
        if ui.memory(|m| m.has_focus(id)) {
            // The candidate window follows the focused field, not a fixed
            // corner. A multiline field uses the first line's centre, not
            // the centre of the whole 170 px box.
            st.caret = (fr.left() + 10.0, fr.top() + FIELD_H / 2.0);
        }
        let mut value = st.values[i].clone();
        if field.multiline {
            paint_multiline(ui, fr, id, &field.hint, &mut value, st.dark);
        } else {
            text_field(
                ui,
                fr,
                id,
                &mut value,
                &FieldOpts {
                    hint: &field.hint,
                    ..FieldOpts::default()
                },
            );
        }
        st.values[i] = value;
        y += field_height(field);
    }

    // Buttons
    let ok_w = 92.0;
    let cancel_w = 76.0;
    let by = rect.bottom() - PAD - BTN_H;
    let ok_rect = EguiRect::from_min_size(
        egui::pos2(rect.right() - PAD - ok_w, by),
        egui::vec2(ok_w, BTN_H),
    );
    let cancel_rect = EguiRect::from_min_size(
        egui::pos2(ok_rect.left() - 10.0 - cancel_w, by),
        egui::vec2(cancel_w, BTN_H),
    );

    if button(ui, cancel_rect, "取消", false, ink, faint) {
        st.accepted = false;
        st.done = true;
    }
    if button(ui, ok_rect, &st.req.ok_label, true, ink, faint) {
        st.accepted = true;
        st.done = true;
    }
}

/// Paints a multiline input.
///
/// Does not reuse `text_field`: that widget is a fixed `TextEdit::singleline`
/// with its shell and height sized for one line. Only its color convention
/// (focused fill plus radius) is borrowed here, the content is a
/// `TextEdit::multiline`.
pub(super) fn paint_multiline(
    ui: &mut egui::Ui,
    rect: EguiRect,
    id: Id,
    hint: &str,
    value: &mut String,
    dark: bool,
) {
    let focused = ui.ctx().memory(|m| m.has_focus(id));
    let (field, focus_bg, border) = if dark {
        (
            Color32::from_rgba_unmultiplied(255, 255, 255, 16),
            Color32::from_rgb(38, 39, 47),
            Color32::from_rgba_unmultiplied(255, 255, 255, 34),
        )
    } else {
        (
            Color32::from_rgba_unmultiplied(23, 26, 35, 14),
            Color32::WHITE,
            Color32::from_rgba_unmultiplied(0, 0, 0, 26),
        )
    };
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(nooksill_settings::text_field::FIELD_R as u8),
        if focused { focus_bg } else { field },
    );
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(nooksill_settings::text_field::FIELD_R as u8),
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(10.0);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.add_sized(
        inner.size(),
        egui::TextEdit::multiline(value)
            .id(id)
            .hint_text(hint)
            .frame(egui::Frame::NONE.inner_margin(egui::Margin::ZERO))
            .desired_width(inner.width()),
    );
}

/// Paints one button. Returns whether it was clicked.
pub(super) fn button(
    ui: &mut egui::Ui,
    rect: EguiRect,
    label: &str,
    primary: bool,
    ink: Color32,
    faint: Color32,
) -> bool {
    let id = Id::unique(("edit-btn", label));
    let resp = ui.interact(rect, id, Sense::click());
    let accent = ui.visuals().selection.bg_fill;
    let (bg, fg) = if primary {
        (accent, Color32::WHITE)
    } else {
        (
            if ui.visuals().dark_mode {
                Color32::from_rgba_unmultiplied(255, 255, 255, 18)
            } else {
                Color32::from_rgba_unmultiplied(23, 26, 35, 14)
            },
            ink,
        )
    };
    ui.painter().rect_filled(rect, CornerRadius::same(9), bg);
    if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(9),
            if primary {
                accent.linear_multiply(1.12)
            } else {
                faint.linear_multiply(0.18)
            },
        );
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(13.0),
        fg,
    );
    resp.clicked()
}
