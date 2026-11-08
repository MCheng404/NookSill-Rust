//! Switch rows and the Fluent pill switch they draw.

use super::*;

/// Switch row: title on the left, switch on the right, as in Windows settings.
///
/// The title itself is not clickable; only the switch is, matching the system.
pub(crate) fn toggle(ctx: &mut Ctx<'_>, ui: &mut Ui, key: Key, label: &str, note: &str) {
    let before = ctx.cfg.get_i32(key);
    let mut v = before != 0;
    row(ui, ctx.state, label, note, |ui, id| {
        switch_widget(ui, id, &mut v);
    });
    if v != (before != 0) {
        ctx.cfg.apply_i32(key, i32::from(v));
        ctx.out.changed = true;
    }
}

/// Same pill switch and row layout as `toggle`, but the value comes from
/// runtime state instead of a config key (autostart reads the registry).
///
/// Returns the new value only: it never touches `Config` or sets `changed`,
/// because such state is either written by a `Command` or not persisted.
pub(crate) fn switch_row(
    ui: &mut Ui,
    state: &mut SettingsState,
    label: &str,
    note: &str,
    value: bool,
) -> Option<bool> {
    let mut v = value;
    row(ui, state, label, note, |ui, id| {
        switch_widget(ui, id, &mut v);
    });
    (v != value).then_some(v)
}

/// Switch whose boolean is derived from the config rather than stored in a key,
/// such as frost (`bg_mode == Frost`) or window scanning, which drives both
/// `dots` and `show_running`. `apply` writes whatever the key set requires.
pub(crate) fn toggle_derived(
    ctx: &mut Ctx<'_>,
    ui: &mut Ui,
    label: &str,
    note: &str,
    value: bool,
    apply: impl FnOnce(&mut Config, bool),
) {
    let mut v = value;
    row(ui, ctx.state, label, note, |ui, id| {
        switch_widget(ui, id, &mut v);
    });
    if v != value {
        apply(ctx.cfg, v);
        ctx.out.changed = true;
    }
}

/// Fluent pill switch: on is an accent fill with the white knob right, off is
/// grey with the knob left. The knob follows an egui bool animation (0.12 s).
fn switch_widget(ui: &mut Ui, label_id: egui::Id, checked: &mut bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(SWITCH_W, SWITCH_H),
        Sense::CLICK | Sense::focusable_noninteractive(),
    );
    let mut resp = resp.labelled_by(label_id);
    resp.widget_info(|| WidgetInfo::selected(egui::Role::Switch, true, *checked, ""));

    if resp.clicked() {
        *checked = !*checked;
        resp.request_focus();
    }
    if resp.has_focus()
        && ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter))
    {
        *checked = !*checked;
        resp.mark_changed();
    }

    let vis = ui.visuals();
    let accent = vis.selection.stroke.color;
    let t = ui
        .ctx()
        .animate_bool_with_time(label_id.with("switch-on"), *checked, 0.12);

    // Fill blends grey to accent with the animation; hover adds a tonal layer.
    let bg = blend(field_bg(ui), accent, t);
    ui.painter()
        .rect_filled(rect, CornerRadius::same(SWITCH_H as u8 / 2), bg);
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(SWITCH_H as u8 / 2), hover(ui));
    }
    // The off state needs an outline, or grey on a white card has no edge.
    if t < 0.5 {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(SWITCH_H as u8 / 2),
            Stroke::new(1.0, vis.weak_text_color().linear_multiply(0.6)),
            egui::StrokeKind::Inside,
        );
    }
    // White knob sliding left to right.
    let r = SWITCH_H / 2.0 - 3.0;
    let cx = egui::emath::lerp((rect.left() + r + 3.0)..=(rect.right() - r - 3.0), t);
    ui.painter()
        .circle_filled(egui::pos2(cx, rect.center().y), r, Color32::WHITE);
    if resp.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            CornerRadius::same(SWITCH_H as u8 / 2 + 2),
            Stroke::new(1.5, accent.linear_multiply(0.5)),
            egui::StrokeKind::Inside,
        );
    }
    resp
}
