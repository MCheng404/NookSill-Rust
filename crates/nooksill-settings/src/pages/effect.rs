//! Effect page: ambient smoke around the bar, one key per setting.

use egui::Ui;
use nooksill_core::config::Key;

use super::options::{
    SMOKE_COLOR2_OPTS, SMOKE_COLOR_OPTS, SMOKE_DENSITY_OPTS, SMOKE_OPACITY_OPTS, SMOKE_REACH_OPTS,
    SMOKE_SPEED_OPTS, SMOKE_THICK_OPTS,
};
use super::widgets::{card, combo, toggle, Ctx};

pub(super) fn page(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Ambient smoke"),
        nooksill_core::i18n::t("Ported from Windhawk's Taskbar Smoke: a real fluid simulation that rises, curls and yields to the cursor. Only enabled while docked at the bottom, as the original was tuned for the taskbar; at the top edge the smoke would rise off-screen."),
        |ctx, ui| {
            toggle(ctx, ui, Key::SMOKE, nooksill_core::i18n::t("Enable ambient smoke"), "");
            combo(ctx, ui, Key::SMOKE_COLOR, nooksill_core::i18n::t("Primary colour"), SMOKE_COLOR_OPTS);
            combo(ctx, ui, Key::SMOKE_COLOR2, nooksill_core::i18n::t("Second colour"), SMOKE_COLOR2_OPTS);
            combo(ctx, ui, Key::SMOKE_OPACITY, nooksill_core::i18n::t("Opacity"), SMOKE_OPACITY_OPTS);
            combo(
                ctx,
                ui,
                Key::SMOKE_TONE,
                nooksill_core::i18n::t("Tone"),
                &[(0, "Smoke (dark)"), (1, "Glow (bright)")],
            );
            toggle(ctx, ui, Key::SMOKE_CLOUDS, nooksill_core::i18n::t("Rising puffs"), "");
        },
    );

    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Smoke feel"),
        nooksill_core::i18n::t("Density and speed drive the fluid directly; thickness also sets how far the glow reaches past the bar."),
        |ctx, ui| {
            combo(ctx, ui, Key::SMOKE_DENSITY, nooksill_core::i18n::t("Density"), SMOKE_DENSITY_OPTS);
            combo(ctx, ui, Key::SMOKE_SPEED, nooksill_core::i18n::t("Speed"), SMOKE_SPEED_OPTS);
            combo(ctx, ui, Key::SMOKE_THICK, nooksill_core::i18n::t("Glow thickness"), SMOKE_THICK_OPTS);
            combo(
                ctx,
                ui,
                Key::SMOKE_MOUSE,
                nooksill_core::i18n::t("Mouse response"),
                &[(0, "Off"), (1, "Soft"), (2, "Strong")],
            );
            combo(ctx, ui, Key::SMOKE_REACH, nooksill_core::i18n::t("Mouse reach"), SMOKE_REACH_OPTS);
        },
    );
}
