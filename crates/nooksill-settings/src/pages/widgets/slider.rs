//! Slider rows; the range comes from `core::config`, the look is self-drawn.

use super::*;

/// Slider row; the range comes from `core::config`, preferring `slider_range`.
pub(crate) fn slider(ctx: &mut Ctx<'_>, ui: &mut Ui, key: Key, label: &str, suffix: &str) {
    let (lo, hi) = key.slider_range().unwrap_or_else(|| key.range());
    let before = ctx.cfg.get_i32(key);
    let mut v = before;
    row(ui, ctx.state, label, "", |ui, id| {
        slider_line(ui, id, &mut v, lo, hi, suffix);
    });
    if v != before {
        ctx.cfg.apply_i32(key, v);
        ctx.out.changed = true;
    }
}

/// Fluent slider: 5 px rail, solid accent fill, white knob with an accent
/// stroke, and a value bubble while dragging.
///
/// Not egui's `Slider`: its rail and knob share `widgets.inactive.bg_fill`, so
/// a grey rail with a white knob is impossible. Pointer mapping and the arrow,
/// Home and End keys are handled below.
fn slider_line(ui: &mut Ui, label_id: egui::Id, value: &mut i32, lo: i32, hi: i32, suffix: &str) {
    let value_w = 48.0;
    let rail_w = SLIDER_W - value_w;
    let (area, _) = ui.allocate_exact_size(egui::vec2(SLIDER_W, 24.0), Sense::hover());
    let rail = egui::Rect::from_center_size(
        egui::pos2(area.left() + rail_w / 2.0, area.center().y),
        egui::vec2(rail_w, 5.0),
    );
    // The hit area is larger than the rail; 5 px is too thin to hit directly.
    let resp = ui.interact(
        rail.expand(9.0),
        egui::Id::unique("slider").with(label_id),
        Sense::click_and_drag(),
    );
    resp.widget_info(|| WidgetInfo::selected(egui::Role::Slider, true, false, ""));
    let mut resp = resp.labelled_by(label_id);

    // Click and drag both map pointer x onto the rail.
    if let Some(pos) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() {
            let p = ((pos.x - rail.left()) / rail.width()).clamp(0.0, 1.0);
            let nv = (lo as f32 + p * (hi - lo) as f32).round() as i32;
            if nv != *value {
                *value = nv;
                resp.mark_changed();
            }
        }
    }
    // Arrow step is 1/20 of the range; Home and End jump to the bounds.
    if resp.has_focus() {
        let step = ((hi - lo) / 20).clamp(1, 10);
        let mut nv = *value;
        for (key, d) in [(egui::Key::ArrowLeft, -step), (egui::Key::ArrowRight, step)] {
            if ui.input(|i| i.key_pressed(key)) {
                nv = (*value + d).clamp(lo, hi);
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Home)) {
            nv = lo;
        }
        if ui.input(|i| i.key_pressed(egui::Key::End)) {
            nv = hi;
        }
        if nv != *value {
            *value = nv;
            resp.mark_changed();
        }
    }
    if resp.clicked() {
        resp.request_focus();
    }

    let vis = ui.visuals();
    let accent = vis.selection.stroke.color;
    let p = (*value - lo) as f32 / (hi - lo) as f32;
    let active = resp.dragged() || resp.is_pointer_button_down_on();

    // Rail plus the filled portion in solid accent.
    ui.painter()
        .rect_filled(rail, CornerRadius::same(2), field_bg(ui));
    if p > 0.0 {
        let fill = egui::Rect::from_min_max(
            rail.min,
            egui::pos2(rail.left() + rail.width() * p, rail.max.y),
        );
        ui.painter()
            .rect_filled(fill, CornerRadius::same(2), accent);
    }
    // Knob grows while dragged or hovered, and a value bubble appears.
    let r = if active || resp.hovered() { 9.0 } else { 7.5 };
    let cx = rail.left() + rail.width() * p.clamp(0.0, 1.0);
    let knob = egui::pos2(cx, rail.center().y);
    ui.painter().circle_filled(knob, r, Color32::WHITE);
    ui.painter()
        .circle_stroke(knob, r, Stroke::new(if active { 2.5 } else { 2.0 }, accent));
    if resp.has_focus() {
        ui.painter()
            .circle_stroke(knob, r + 2.5, Stroke::new(1.5, accent.linear_multiply(0.5)));
    }
    if active {
        let text = format!("{}{}", value, suffix);
        let font = FontId::proportional(11.0);
        let galley = ui.painter().layout_no_wrap(text, font, Color32::WHITE);
        let tip_w = galley.size().x + 14.0;
        let tip = egui::Rect::from_center_size(
            egui::pos2(knob.x, rail.top() - 16.0),
            egui::vec2(tip_w, 17.0),
        );
        ui.painter().rect_filled(tip, CornerRadius::same(5), accent);
        ui.painter().galley(
            egui::Align2::CENTER_CENTER
                .align_size_within_rect(galley.size(), tip)
                .min,
            galley,
            Color32::WHITE,
        );
    }
    // Permanent value readout on the right.
    let vtext = format!("{}{}", value, suffix);
    ui.painter().text(
        egui::pos2(area.right(), area.center().y),
        Align2::RIGHT_CENTER,
        vtext,
        FontId::proportional(12.0),
        if active {
            accent
        } else {
            vis.weak_text_color()
        },
    );
}
