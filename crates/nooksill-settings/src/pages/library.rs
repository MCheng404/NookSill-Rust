//! Icon library page: bulk icon assignment from a folder.

use egui::{RichText, Ui};

use super::widgets::{button, card, Ctx};
use super::Command;

pub(super) fn page(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Apply icons from a folder"),
        "",
        |ctx, ui| {
            ui.horizontal(|ui| {
                button(
                    ctx,
                    ui,
                    nooksill_core::i18n::t("Choose icon folder…"),
                    Command::PickIconDir,
                );
                let picked = !ctx.state.icon_dir.is_empty();
                if picked {
                    button(
                        ctx,
                        ui,
                        nooksill_core::i18n::t("Match and apply automatically"),
                        Command::AutoMatch,
                    );
                }
            });
            if ctx.state.icon_dir.is_empty() {
                ui.label(
                    RichText::new(nooksill_core::i18n::t("No folder selected yet."))
                        .small()
                        .weak(),
                );
            } else {
                ui.label(
                    RichText::new(nooksill_core::i18n::fmt(
                        "Icon folder: {0}",
                        &[ctx.state.icon_dir.to_string().as_str()],
                    ))
                    .small()
                    .weak(),
                );
            }
            ui.label(
            RichText::new(
                nooksill_core::i18n::t("Files are matched by name against app names; only .ico is read, the native Windows icon format."),
            )
            .small()
            .weak(),
        );
        },
    );
}
