//! Interaction page: hover zoom, folder fan, animation, reveal delays.

use egui::Ui;
use nooksill_core::config::Key;

use super::widgets::{card, slider, toggle, Ctx};

pub(super) fn page(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Interaction"),
        "",
        |ctx, ui| {
            toggle(
                ctx,
                ui,
                Key::Zoom,
                nooksill_core::i18n::t("Zoom icons on hover"),
                "",
            );
            toggle(
                ctx,
                ui,
                Key::FolderFan,
                nooksill_core::i18n::t("Fan folders out on click"),
                nooksill_core::i18n::t("List recent files like macOS; click one to open it"),
            );
            toggle(
                ctx,
                ui,
                Key::SEP_SCALE,
                nooksill_core::i18n::t("Drag a separator vertically to resize the Dock"),
                nooksill_core::i18n::t(
                    "Drag the line between icons to scale icon and zoom sizes together",
                ),
            );
        },
    );

    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Animation"),
        "",
        |ctx, ui| {
            toggle(
                ctx,
                ui,
                Key::Bounce,
                nooksill_core::i18n::t("Bounce when launching an app"),
                "",
            );
        },
    );

    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Show / hide sensitivity · normal"),
        nooksill_core::i18n::t("Hiding waits for the cursor to leave; showing waits for it to stay. Higher values stop edge flicker; milliseconds"),
        |ctx, ui| {
            slider(ctx, ui, Key::HIDE_DELAY, nooksill_core::i18n::t("Hide delay after leaving"), "ms");
            slider(ctx, ui, Key::SHOW_DELAY, nooksill_core::i18n::t("Show delay when hidden"), "ms");
        },
    );
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Show / hide sensitivity · maximised windows"),
        nooksill_core::i18n::t("A maximised window sits on the screen edge and is easy to hit by accident; use longer delays here, in milliseconds"),
        |ctx, ui| {
            slider(ctx, ui, Key::HIDE_DELAY_MAX, nooksill_core::i18n::t("Hide delay after leaving"), "ms");
            slider(ctx, ui, Key::SHOW_DELAY_MAX, nooksill_core::i18n::t("Show delay when hidden"), "ms");
        },
    );
}
