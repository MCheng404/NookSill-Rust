//! Extras page: clock, reflection, widgets, browser.

use egui::Ui;
use nooksill_core::config::Key;

use super::options::BROWSER_OPTS;
use super::widgets::{button, card, combo, slider, toggle, Ctx};
use super::Command;

pub(super) fn page(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Dock extras"),
        "",
        |ctx, ui| {
            toggle(
                ctx,
                ui,
                Key::Clock,
                nooksill_core::i18n::t("Show time and date on the right of the Dock"),
                nooksill_core::i18n::t("Refreshes every 30 seconds at almost no cost"),
            );
            toggle(
                ctx,
                ui,
                Key::Reflect,
                nooksill_core::i18n::t("Icon reflection"),
                nooksill_core::i18n::t("Only effective when docked at the bottom"),
            );
            slider(
                ctx,
                ui,
                Key::REFL_SIZE,
                nooksill_core::i18n::t("Reflection size"),
                " %",
            );
            slider(
                ctx,
                ui,
                Key::REFL_GAP,
                nooksill_core::i18n::t("Reflection gap"),
                " px",
            );
            slider(
                ctx,
                ui,
                Key::REFL_OPACITY,
                nooksill_core::i18n::t("Reflection opacity"),
                " %",
            );
        },
    );

    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Widgets"),
        nooksill_core::i18n::t(
            "The strip on the right of the Dock. Each widget has its own switch.",
        ),
        |ctx, ui| {
            toggle(
                ctx,
                ui,
                Key::NOTIFY_W,
                nooksill_core::i18n::t("Notifications"),
                "",
            );
            toggle(
                ctx,
                ui,
                Key::CTRL_CENTER,
                nooksill_core::i18n::t("Control Center"),
                "",
            );
            toggle(
                ctx,
                ui,
                Key::CC_ENERGY,
                nooksill_core::i18n::t("Control Center · power saver tile"),
                "",
            );
            toggle(
                ctx,
                ui,
                Key::CC_HOTSPOT,
                nooksill_core::i18n::t("Control Center · mobile hotspot tile"),
                "",
            );
            toggle(
                ctx,
                ui,
                Key::RES_MON,
                nooksill_core::i18n::t("Resource Monitor"),
                "",
            );
            toggle(
                ctx,
                ui,
                Key::TOOL_W,
                nooksill_core::i18n::t("Timer / To-do"),
                "",
            );
            toggle(
                ctx,
                ui,
                Key::PANEL_DISMISS,
                nooksill_core::i18n::t("Dismiss by clicking outside"),
                nooksill_core::i18n::t("Applies to Calendar, Control Center, Resource Monitor and Timer; off means clicking the card again"),
            );
            super::widgets::row(
                ui,
                ctx.state,
                nooksill_core::i18n::t("Widget side"),
                "",
                |ui, id| {
                    let mut cur = ctx.cfg.get_i32(Key::W_SIDE);
                    egui::ComboBox::from_id_salt(Key::W_SIDE.name())
                        .selected_text(if cur == 0 {
                            nooksill_core::i18n::t("Right")
                        } else {
                            nooksill_core::i18n::t("Left")
                        })
                        .width(168.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut cur, 0, nooksill_core::i18n::t("Right"));
                            ui.selectable_value(&mut cur, 1, nooksill_core::i18n::t("Left"));
                        })
                        .response
                        .labelled_by(id);
                    if cur != ctx.cfg.get_i32(Key::W_SIDE) {
                        ctx.cfg.apply_i32(Key::W_SIDE, cur);
                        ctx.out.changed = true;
                    }
                },
            );
            super::widgets::row(
                ui,
                ctx.state,
                nooksill_core::i18n::t("Widget order"),
                "",
                |ui, id| {
                    let mut cur = ctx.cfg.get_i32(Key::W_ORDER);
                    egui::ComboBox::from_id_salt(Key::W_ORDER.name())
                        .selected_text(if cur == 0 {
                            nooksill_core::i18n::t("Clock first")
                        } else {
                            nooksill_core::i18n::t("Notifications first")
                        })
                        .width(168.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut cur, 0, nooksill_core::i18n::t("Clock first"));
                            ui.selectable_value(
                                &mut cur,
                                1,
                                nooksill_core::i18n::t("Notifications first"),
                            );
                        })
                        .response
                        .labelled_by(id);
                    if cur != ctx.cfg.get_i32(Key::W_ORDER) {
                        ctx.cfg.apply_i32(Key::W_ORDER, cur);
                        ctx.out.changed = true;
                    }
                },
            );
        },
    );

    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Utilities"),
        "",
        |ctx, ui| {
            combo(
                ctx,
                ui,
                Key::BROWSER,
                nooksill_core::i18n::t("Browser for web pages"),
                BROWSER_OPTS,
            );
            toggle(
                ctx,
                ui,
                Key::Notes,
                nooksill_core::i18n::t("Notes (click to jot down text)"),
                nooksill_core::i18n::t("Adds a Notes icon to the Dock"),
            );
            // Shortcut next to the switch: the dock icon alone is hard to find.
            button(
                ctx,
                ui,
                nooksill_core::i18n::t("Open Notes…"),
                Command::OpenNotes,
            );
        },
    );
}
