//! Color rows: accent source and the optional custom color picker.

use super::*;

/// Accent source: a combo of system or custom, plus a picker when custom.
///
/// Unlike `color_row`, "follow the system" is a valid value (-1) rather than
/// "disabled", so the two sources are shown as a combo instead of a checkbox.
pub(crate) fn accent_row(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    let cur = ctx.cfg.get_i32(Key::UI_ACCENT);
    let mut custom = cur >= 0;
    let mut rgb = {
        let c = color::from_colorref(cur.max(0) as u32);
        [color::red(c), color::green(c), color::blue(c)]
    };
    let before = custom;
    row(
        ui,
        ctx.state,
        nooksill_core::i18n::t("Source"),
        nooksill_core::i18n::t("Follow the Windows accent or pick your own colour"),
        |ui, id| {
            egui::ComboBox::from_id_salt("ui-accent-mode")
                .selected_text(if custom {
                    nooksill_core::i18n::t("Custom")
                } else {
                    nooksill_core::i18n::t("Follow system")
                })
                .width(112.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut custom,
                        false,
                        nooksill_core::i18n::t("Follow system"),
                    )
                    .labelled_by(id);
                    ui.selectable_value(&mut custom, true, nooksill_core::i18n::t("Custom"))
                        .labelled_by(id);
                })
                .response
                .labelled_by(id);
            if custom {
                ui.color_edit_button_srgb(&mut rgb).labelled_by(id);
            }
        },
    );
    if custom != before {
        // Switching to custom starts from a readable blue, not black.
        ctx.cfg
            .apply_i32(Key::UI_ACCENT, if custom { 0x0099_6633 } else { -1 });
        ctx.out.changed = true;
    } else if custom {
        let v = color::to_colorref(color::rgb(rgb[0], rgb[1], rgb[2])) as i32;
        if v != cur {
            ctx.cfg.apply_i32(Key::UI_ACCENT, v);
            ctx.out.changed = true;
        }
    }
}

/// Custom colour: `-1` disables it, otherwise the value is a COLORREF.
pub(crate) fn color_row(ctx: &mut Ctx<'_>, ui: &mut Ui, key: Key, label: &str) {
    let cur = ctx.cfg.get_i32(key);
    let mut enabled = cur >= 0;
    let mut arr = {
        let c = color::from_colorref(cur.max(0) as u32);
        [color::red(c), color::green(c), color::blue(c)]
    };
    row(ui, ctx.state, label, "", |ui, id| {
        let resp = ui.checkbox(&mut enabled, nooksill_core::i18n::t("Enable"));
        resp.labelled_by(id);
        if enabled {
            ui.color_edit_button_srgb(&mut arr).labelled_by(id);
        }
    });
    if enabled != (cur >= 0) {
        let v = if enabled { 0x0033_6699 } else { -1 };
        ctx.cfg.apply_i32(key, v);
        ctx.out.changed = true;
    } else if enabled {
        let v = color::to_colorref(color::rgb(arr[0], arr[1], arr[2])) as i32;
        if v != cur {
            ctx.cfg.apply_i32(key, v);
            ctx.out.changed = true;
        }
    }
}
