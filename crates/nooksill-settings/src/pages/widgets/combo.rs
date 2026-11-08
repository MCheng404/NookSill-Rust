//! Combo row; options are `(integer code, display text)` pairs.

use super::*;

/// Combo row; `opts` is `(code stored in the INI, display text)`.
pub(crate) fn combo(ctx: &mut Ctx<'_>, ui: &mut Ui, key: Key, label: &str, opts: &[(i32, &str)]) {
    let before = ctx.cfg.get_i32(key);
    let mut cur = before;
    row(ui, ctx.state, label, "", |ui, id| {
        let text = opts
            .iter()
            .find(|(v, _)| *v == cur)
            .map_or("—", |(_, t)| *t);
        egui::ComboBox::from_id_salt(key.name())
            .selected_text(text)
            .width(180.0)
            .show_ui(ui, |ui| {
                for (v, t) in opts {
                    ui.selectable_value(&mut cur, *v, *t);
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
