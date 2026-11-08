//! Buttons: full-width (emits a `Command`) and small list-row ones.

use super::*;

/// Draw a button that queues `cmd` when pressed; this layer never runs it.
pub(crate) fn button(ctx: &mut Ctx<'_>, ui: &mut Ui, label: &str, cmd: Command) {
    let font = FontId::new(
        ui.style().text_styles[&egui::TextStyle::Body].size,
        egui::FontFamily::Proportional,
    );
    // Laid out with PLACEHOLDER: the real colour is passed to `galley` below.
    // Hard-coding white would make the label invisible on light buttons.
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER);
    let size = egui::vec2((galley.size().x + 44.0).max(72.0), 30.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::CLICK);
    resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, label));

    // Click, or Space/Enter while focused. `Command` is not `Copy`, so both
    // paths must converge before it is moved.
    let activated = resp.clicked()
        || (resp.has_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter)));
    if activated {
        ctx.out.command = Some(cmd);
        resp.mark_changed();
        resp.request_focus();
    }

    // Three states: the fill, then hover on top, then a second hover layer
    // while pressed. Translucent layers over the card give a gradual tonal
    // ramp without a solid colour per theme.
    let vis = ui.visuals();
    ui.painter()
        .rect_filled(rect, CornerRadius::same(10), field_bg(ui));
    if resp.is_pointer_button_down_on() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(10), hover(ui));
        ui.painter()
            .rect_filled(rect, CornerRadius::same(10), hover(ui));
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(10), hover(ui));
    }
    if resp.has_focus() {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(10),
            Stroke::new(1.5, vis.selection.stroke.color),
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().galley(
        egui::Align2::CENTER_CENTER
            .align_size_within_rect(galley.size(), rect)
            .min,
        galley,
        vis.text_color(),
    );
}

/// Small button that only reports a click; the caller builds the command.
///
/// List rows need the command to carry an index, which is unknown when the
/// button would have to be built for [`button`].
pub(crate) fn small_button(ui: &mut Ui, label: &str, tip: &str) -> bool {
    let font = FontId::new(
        ui.style().text_styles[&egui::TextStyle::Body].size,
        egui::FontFamily::Proportional,
    );
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER);
    let size = egui::vec2((galley.size().x + 22.0).max(38.0), 26.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::CLICK);
    resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, label));
    if !tip.is_empty() {
        resp = resp.on_hover_text(tip);
    }
    let activated = resp.clicked()
        || (resp.has_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter)));
    if activated {
        resp.mark_changed();
    }

    let vis = ui.visuals();
    ui.painter()
        .rect_filled(rect, CornerRadius::same(8), field_bg(ui));
    if resp.is_pointer_button_down_on() || resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), hover(ui));
    }
    if resp.has_focus() {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(8),
            Stroke::new(1.5, vis.selection.stroke.color),
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().galley(
        egui::Align2::CENTER_CENTER
            .align_size_within_rect(galley.size(), rect)
            .min,
        galley,
        vis.text_color(),
    );
    activated
}
