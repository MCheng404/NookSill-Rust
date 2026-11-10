//! Icons page: plate, separators, running indicator, labels.

use egui::{RichText, Ui};
use nooksill_core::config::Key;

use super::options::{DOT_OPTS, PLATE_OPTS, PLATE_THEME_OPTS};
use super::widgets::{card, combo, slider, toggle, Ctx};

pub(super) fn page(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Icon plate (macOS-style rounded square)"),
        "",
        |ctx, ui| {
            combo(
                ctx,
                ui,
                Key::PLATE_MODE,
                nooksill_core::i18n::t("Plate type"),
                PLATE_OPTS,
            );
            combo(
                ctx,
                ui,
                Key::PLATE_THEME,
                nooksill_core::i18n::t("Plate colour"),
                PLATE_THEME_OPTS,
            );
            slider(
                ctx,
                ui,
                Key::PLATE_OPACITY,
                nooksill_core::i18n::t("Plate opacity"),
                " %",
            );
            slider(
                ctx,
                ui,
                Key::PlateRadius,
                nooksill_core::i18n::t("Plate corner radius"),
                " %",
            );
        },
    );

    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Separator"),
        "",
        |ctx, ui| {
            slider(
                ctx,
                ui,
                Key::SEP_WIDTH,
                nooksill_core::i18n::t("Separator width"),
                " px",
            );
            slider(
                ctx,
                ui,
                Key::SEP_GAP,
                nooksill_core::i18n::t("Separator gap"),
                " px",
            );
            ui.label(
                RichText::new(nooksill_core::i18n::t(
                    "0% is square, 22% is the default rounded square, 50% is a circle.",
                ))
                .small()
                .weak(),
            );
        },
    );

    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Running dot & labels"),
        "",
        |ctx, ui| {
            toggle(
                ctx,
                ui,
                Key::Dots,
                nooksill_core::i18n::t("Show running indicator"),
                "",
            );
            combo(
                ctx,
                ui,
                Key::DotStyle,
                nooksill_core::i18n::t("Indicator style"),
                DOT_OPTS,
            );
            toggle(
                ctx,
                ui,
                Key::Labels,
                nooksill_core::i18n::t("Show name label on hover"),
                "",
            );
        },
    );
}
