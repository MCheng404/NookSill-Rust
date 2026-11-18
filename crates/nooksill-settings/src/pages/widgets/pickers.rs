//! Colour rows: self-drawn switches and swatches in the settings element style.

use super::*;

/// Swatch size; wide enough to read the colour, narrow enough for a row.
const SWATCH_W: f32 = 46.0;
const SWATCH_H: f32 = 26.0;

/// Windows accent colours, kept as sRGB triples for the palette tiles.
const PRESETS: [[u8; 3]; 12] = [
    [0, 120, 212],
    [135, 100, 184],
    [194, 57, 179],
    [232, 17, 35],
    [234, 92, 15],
    [255, 185, 0],
    [16, 124, 16],
    [3, 131, 135],
    [105, 121, 126],
    [0, 69, 120],
    [255, 255, 255],
    [0, 0, 0],
];

/// Current colour of `key` as an sRGB triple; `-1` reads as a readable blue.
fn rgb_of(cfg: &Config, key: Key) -> [u8; 3] {
    let c = color::from_colorref(cfg.get_i32(key).max(0) as u32);
    [color::red(c), color::green(c), color::blue(c)]
}

fn hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let s = text.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8])
}

/// Ink that stays readable on a preset tile.
fn ink_on(rgb: [u8; 3]) -> Color32 {
    let luma = rgb[0] as f32 * 0.299 + rgb[1] as f32 * 0.587 + rgb[2] as f32 * 0.114;
    if luma > 150.0 {
        Color32::from_rgb(20, 22, 28)
    } else {
        Color32::WHITE
    }
}

fn store(ctx: &mut Ctx<'_>, key: Key, rgb: [u8; 3]) {
    let v = color::to_colorref(color::rgb(rgb[0], rgb[1], rgb[2])) as i32;
    if v != ctx.cfg.get_i32(key) {
        ctx.cfg.apply_i32(key, v);
        ctx.out.changed = true;
    }
}

/// Self-drawn colour button: rounded fill, hover wash, accent focus ring.
fn swatch(ui: &mut Ui, rgb: [u8; 3], label_id: egui::Id, open: &mut bool) {
    let (rect, mut resp) = ui.allocate_exact_size(
        egui::vec2(SWATCH_W, SWATCH_H),
        Sense::CLICK | Sense::focusable_noninteractive(),
    );
    resp = resp.labelled_by(label_id);
    resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, hex(rgb)));
    if resp.clicked()
        || (resp.has_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter)))
    {
        *open = !*open;
        resp.mark_changed();
    }
    let radius = CornerRadius::same(8);
    ui.painter()
        .rect_filled(rect, radius, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
    if resp.hovered() || resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, radius, hover(ui));
    }
    ui.painter().rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, ui.visuals().weak_text_color().linear_multiply(0.7)),
        egui::StrokeKind::Inside,
    );
    if resp.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            CornerRadius::same(10),
            Stroke::new(1.5, ui.visuals().selection.stroke.color),
            egui::StrokeKind::Inside,
        );
    }
}

/// Expanded palette: preset tiles plus an exact HEX field.
fn palette(ctx: &mut Ctx<'_>, ui: &mut Ui, key: Key, current: [u8; 3]) {
    ui.add_space(6.0);
    for band in PRESETS.chunks(6) {
        ui.horizontal(|ui| {
            for preset in band {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::click());
                let fill = Color32::from_rgb(preset[0], preset[1], preset[2]);
                ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
                if *preset == current {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "✓",
                        FontId::proportional(15.0),
                        ink_on(*preset),
                    );
                }
                resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, hex(*preset)));
                if resp.clicked() {
                    store(ctx, key, *preset);
                    ctx.state.color_hex = hex(*preset);
                    ctx.state.color_edit = None;
                }
            }
        });
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let label = ui.label(RichText::new("HEX").small().weak());
        let mut text = ctx.state.color_hex.clone();
        let resp = ui.text_edit_singleline(&mut text);
        let changed = resp.changed();
        resp.labelled_by(label.id);
        if changed {
            if let Some(rgb) = parse_hex(&text) {
                store(ctx, key, rgb);
            }
        }
        ctx.state.color_hex = text;
    });
}

/// Accent source: one pill switch, with a swatch for the custom branch.
pub(crate) fn accent_row(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    let key = Key::UI_ACCENT;
    let cur = ctx.cfg.get_i32(key);
    let mut custom = cur >= 0;
    let rgb = rgb_of(ctx.cfg, key);
    let was_open = ctx.state.color_edit == Some(key);
    let mut open = was_open && custom;
    row(
        ui,
        ctx.state,
        nooksill_core::i18n::t("Source"),
        nooksill_core::i18n::t("Follow the Windows accent or pick your own colour"),
        |ui, id| {
            switch_widget(ui, id, &mut custom);
            if custom {
                swatch(ui, rgb, id, &mut open);
            }
        },
    );
    if custom != (cur >= 0) {
        // Switching to custom starts from a readable blue, not black.
        ctx.cfg
            .apply_i32(key, if custom { 0x0099_6633 } else { -1 });
        ctx.out.changed = true;
        open = false;
    } else if custom {
        store(ctx, key, rgb);
    }
    if open != was_open {
        ctx.state.color_edit = if open { Some(key) } else { None };
        ctx.state.color_hex = hex(rgb);
    }
    if open {
        palette(ctx, ui, key, rgb);
    }
}
