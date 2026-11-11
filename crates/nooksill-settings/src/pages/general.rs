//! General page: startup, locking, hotkey and data folder.

use egui::Ui;
use nooksill_core::config::Key;
use nooksill_core::hotkey::Hotkey;

use super::options::UI_THEME_OPTS;
use super::widgets::{button, card, combo, row, switch_row, toggle, Ctx};
use super::Command;

pub(super) fn page(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Startup & General"),
        "",
        |ctx, ui| {
            // Autostart has no config key: the value is the registry's truth and
            // only a command leaves here, so the config is never marked dirty.
            let before = ctx.state.auto_start;
            if let Some(auto) = switch_row(
                ui,
                ctx.state,
                nooksill_core::i18n::t("Launch at sign-in"),
                nooksill_core::i18n::t("Shows the real state from the registry"),
                before,
            ) {
                ctx.state.auto_start = auto;
                ctx.out.command = Some(Command::ToggleAutostart(auto));
            }

            toggle(
                ctx,
                ui,
                Key::Lock,
                nooksill_core::i18n::t("Lock Dock layout"),
                nooksill_core::i18n::t("Prevent accidental dragging and removal"),
            );
            toggle(
                ctx,
                ui,
                Key::ShowRunning,
                nooksill_core::i18n::t("Show unpinned running apps"),
                nooksill_core::i18n::t(
                    "Show running apps after the separator; drag one in to pin it",
                ),
            );
            toggle(
                ctx,
                ui,
                Key::Hotkey,
                nooksill_core::i18n::t("Global hotkey"),
                nooksill_core::i18n::t("Show or hide the Dock at any time"),
            );
            if ctx.state.hotkey_input.is_empty() {
                ctx.state.hotkey_input = ctx.cfg.hotkey_combo.display();
            }
            let mut text = ctx.state.hotkey_input.clone();
            let changed = row(
                ui,
                ctx.state,
                nooksill_core::i18n::t("Toggle hotkey"),
                nooksill_core::i18n::t("Modifier plus a key, e.g. Ctrl+Alt+D"),
                |ui, id| {
                    let resp = ui.add(egui::TextEdit::singleline(&mut text).desired_width(180.0));
                    resp.labelled_by(id).changed()
                },
            );
            if changed {
                if let Some(hk) = Hotkey::parse(&text) {
                    ctx.cfg.hotkey_combo = hk;
                    ctx.out.changed = true;
                    ctx.state.hotkey_input = hk.display();
                    ctx.state.hotkey_error = false;
                } else {
                    ctx.state.hotkey_input = text.clone();
                    ctx.state.hotkey_error = true;
                }
            } else {
                ctx.state.hotkey_input = text.clone();
            }
            if ctx.state.hotkey_error {
                let color = ui.visuals().error_fg_color;
                ui.colored_label(
                    color,
                    nooksill_core::i18n::t("Invalid hotkey; use e.g. Ctrl+Alt+D"),
                );
            }
            toggle(
                ctx,
                ui,
                Key::WebAppMode,
                nooksill_core::i18n::t("Open web pages in app windows"),
                nooksill_core::i18n::t("Use the --app mode of Edge or Chrome"),
            );
        },
    );

    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Settings window"),
        "",
        |ctx, ui| {
            combo(
                ctx,
                ui,
                Key::UiTheme,
                nooksill_core::i18n::t("Settings window theme"),
                UI_THEME_OPTS,
            );
            // Scales fonts, buttons, row heights and card gaps through egui's ppp;
            // the host rebuilds the window in place rather than reopening it.
            combo(
                ctx,
                ui,
                Key::UI_SCALE,
                nooksill_core::i18n::t("UI scale"),
                &[
                    (0, "Follow system"),
                    (100, "100%"),
                    (110, "110%"),
                    (125, "125%"),
                    (150, "150%"),
                    (175, "175%"),
                    (200, "200%"),
                ],
            );
            super::widgets::row(
                ui,
                ctx.state,
                nooksill_core::i18n::t("Language"),
                nooksill_core::i18n::t("System default follows the OS display language"),
                |ui, id| {
                    let before = ctx.cfg.language.clone();
                    let mut cur = before.clone();
                    let display = if cur.is_empty() {
                        nooksill_core::i18n::t("System default").to_owned()
                    } else {
                        nooksill_core::i18n::Language::from_tag(&cur)
                            .map_or_else(|| cur.clone(), |lang| lang.name().to_owned())
                    };
                    egui::ComboBox::from_id_salt("ui_language")
                        .selected_text(display)
                        .width(180.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut cur,
                                String::new(),
                                nooksill_core::i18n::t("System default"),
                            );
                            for lang in nooksill_core::i18n::Language::ALL {
                                ui.selectable_value(&mut cur, lang.tag().to_owned(), lang.name());
                            }
                        })
                        .response
                        .labelled_by(id);
                    if cur != before {
                        ctx.cfg.language = cur;
                        ctx.out.changed = true;
                    }
                },
            );
        },
    );

    card(ctx, ui, nooksill_core::i18n::t("Data"), "", |ctx, ui| {
        ui.horizontal(|ui| {
            ui.label(nooksill_core::i18n::t("Config folder"));
            ui.monospace(&ctx.state.config_dir);
        });
        ui.add_space(4.0);
        button(
            ctx,
            ui,
            nooksill_core::i18n::t("Open config folder"),
            Command::OpenConfigDir,
        );
    });
}
