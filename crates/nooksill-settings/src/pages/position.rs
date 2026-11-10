//! Position page: dock edge, monitor and display mode.

use egui::Ui;
use nooksill_core::config::Key;

use super::options::{EDGE_OPTS, MODE_OPTS};
use super::widgets::{card, combo, slider, toggle, Ctx};

pub(super) fn page(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Position"),
        "",
        |ctx, ui| {
            combo(
                ctx,
                ui,
                Key::Edge,
                nooksill_core::i18n::t("Dock position (centred)"),
                EDGE_OPTS,
            );

            // Monitor list is runtime data, so the options are built per frame.
            let before = ctx.cfg.monitor;
            let mut cur = before;
            let monitors = ctx.state.monitors.clone();
            super::widgets::row(
                ui,
                ctx.state,
                nooksill_core::i18n::t("Show on"),
                "",
                |ui, id| {
                    let text = monitors
                        .get(cur.max(0) as usize)
                        .cloned()
                        .unwrap_or_else(|| {
                            nooksill_core::i18n::fmt(
                                "Monitor {0}",
                                &[(cur + 1).to_string().as_str()],
                            )
                        });
                    egui::ComboBox::from_id_salt("monitor")
                        .selected_text(text)
                        .width(168.0)
                        .show_ui(ui, |ui| {
                            for (i, name) in monitors.iter().enumerate() {
                                ui.selectable_value(&mut cur, i as i32, name);
                            }
                        })
                        .response
                        .labelled_by(id);
                },
            );
            if cur != before {
                ctx.cfg.apply_i32(Key::Monitor, cur);
                ctx.out.changed = true;
            }

            slider(
                ctx,
                ui,
                Key::Margin,
                nooksill_core::i18n::t("Distance from screen edge"),
                " px",
            );
        },
    );

    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Visibility mode"),
        "",
        |ctx, ui| {
            combo(
                ctx,
                ui,
                Key::Mode,
                nooksill_core::i18n::t("Mode"),
                MODE_OPTS,
            );
            toggle(
                ctx,
                ui,
                Key::Reserve,
                nooksill_core::i18n::t("Keep clear of maximised windows"),
                nooksill_core::i18n::t(
                    "Only in Always visible mode; reserves desktop space like macOS",
                ),
            );
        },
    );
}
