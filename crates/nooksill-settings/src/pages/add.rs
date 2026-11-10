//! Add page: new items, import and export, reset.

use egui::{Id, Sense, Ui};

use super::widgets::{button, card, Ctx};
use super::Command;
use crate::text_field::{text_field, FieldOpts, FIELD_H};

pub(super) fn page(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(ctx, ui, nooksill_core::i18n::t("Add"), "", |ctx, ui| {
        ui.horizontal_wrapped(|ui| {
            button(
                ctx,
                ui,
                nooksill_core::i18n::t("Add app / file…"),
                Command::AddFile,
            );
            button(
                ctx,
                ui,
                nooksill_core::i18n::t("Add folder…"),
                Command::AddFolder,
            );
            button(
                ctx,
                ui,
                nooksill_core::i18n::t("New app group"),
                Command::NewGroup,
            );
            button(
                ctx,
                ui,
                nooksill_core::i18n::t("Add Notes"),
                Command::AddNote,
            );
            button(
                ctx,
                ui,
                nooksill_core::i18n::t("Add Recycle Bin"),
                Command::AddTrash,
            );
            button(
                ctx,
                ui,
                nooksill_core::i18n::t("Add separator"),
                Command::AddSeparator,
            );
        });
    });

    // Inline field instead of a modal: the settings window is already in
    // front, and Enter submits.
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Add web app"),
        nooksill_core::i18n::t("The URL opens in an app window or the default browser."),
        |ctx, ui| {
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 92.0).max(160.0);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(width, FIELD_H), Sense::hover());
                text_field(
                    ui,
                    rect,
                    Id::unique("add-url-field"),
                    &mut ctx.state.url_input,
                    &FieldOpts {
                        hint: "https://example.com",
                        clearable: true,
                        ..FieldOpts::default()
                    },
                );
                let ready = !ctx.state.url_input.trim().is_empty();
                if ready {
                    button(ctx, ui, nooksill_core::i18n::t("Add"), Command::AddUrl);
                }
                let entered = rect
                    .contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default()))
                    && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ready && entered {
                    ctx.out.command = Some(Command::AddUrl);
                }
            });
            ui.label(
                egui::RichText::new("例：https://github.com —— 名称取域名，图标用首字占位。")
                    .small()
                    .weak(),
            );
        },
    );

    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Import & export"),
        nooksill_core::i18n::t("Exports the whole ini, including items and groups."),
        |ctx, ui| {
            ui.horizontal(|ui| {
                button(
                    ctx,
                    ui,
                    nooksill_core::i18n::t("Export full config…"),
                    Command::ExportAll,
                );
                button(
                    ctx,
                    ui,
                    nooksill_core::i18n::t("Import config…"),
                    Command::Import,
                );
            });
        },
    );

    card(ctx, ui, nooksill_core::i18n::t("Reset"), "", |ctx, ui| {
        button(
            ctx,
            ui,
            nooksill_core::i18n::t("Restore defaults"),
            Command::ResetDefaults,
        );
    });
}
