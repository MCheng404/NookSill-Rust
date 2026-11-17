//! Resource monitor rows: choose which readings appear and in what order.

use egui::Ui;
use nooksill_core::i18n::t;
use nooksill_core::resource::{self, ResKind};

use super::widgets::{card, row, small_button, switch_widget, Ctx};

pub(super) fn page(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(
        ctx,
        ui,
        t("Resource Monitor"),
        t("Choose which readings appear and in what order."),
        |ctx, ui| {
            let mut order = resource::parse_order(&ctx.cfg.res_order);
            let mut changed = false;
            // Visible rows first, hidden ones below so they are easy to find.
            let mut rows = order.clone();
            for kind in ResKind::ALL {
                if !rows.contains(&kind) {
                    rows.push(kind);
                }
            }
            for kind in rows {
                let visible = order.contains(&kind);
                let mut checked = visible;
                row(ui, ctx.state, t(kind.label()), "", |ui, id| {
                    switch_widget(ui, id, &mut checked);
                    if visible {
                        if small_button(ui, "↓", "") && resource::move_down(&mut order, kind) {
                            changed = true;
                        }
                        if small_button(ui, "↑", "") && resource::move_up(&mut order, kind) {
                            changed = true;
                        }
                    }
                });
                if checked != visible {
                    changed |= if checked {
                        resource::add(&mut order, kind)
                    } else {
                        resource::remove(&mut order, kind)
                    };
                }
            }
            if changed {
                ctx.cfg.res_order = resource::order_text(&order);
                ctx.out.changed = true;
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_label_is_registered() {
        assert_eq!(ResKind::ALL.len(), 7);
        for kind in ResKind::ALL {
            assert!(
                nooksill_core::i18n::has_key(kind.label()),
                "{}",
                kind.label()
            );
        }
    }
}
