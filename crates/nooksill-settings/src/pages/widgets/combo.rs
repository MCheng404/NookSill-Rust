//! Combo row; options are `(integer code, display text)` pairs.

use super::*;

/// Combo row; `opts` is `(code stored in the INI, display text)`.
pub(crate) fn combo(
    ctx: &mut Ctx<'_>,
    ui: &mut Ui,
    key: Key,
    label: &str,
    opts: &'static [(i32, &'static str)],
) {
    let before = ctx.cfg.get_i32(key);
    let mut cur = before;
    row(ui, ctx.state, label, "", |ui, id| {
        let text = opts
            .iter()
            .find(|(v, _)| *v == cur)
            .map_or("—", |(_, label)| nooksill_core::i18n::t(label));
        egui::ComboBox::from_id_salt(key.name())
            .selected_text(text)
            .width(180.0)
            .show_ui(ui, |ui| {
                for (v, label) in opts {
                    ui.selectable_value(&mut cur, *v, nooksill_core::i18n::t(label));
                }
            })
            .response
            .labelled_by(id);
    });
    if cur != before {
        ctx.cfg.apply_i32(key, cur);
        ctx.out.changed = true;
    }
}
