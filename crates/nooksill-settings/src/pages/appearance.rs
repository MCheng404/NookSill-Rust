//! Appearance page: size, background, colours, borders.

use egui::Ui;
use nooksill_core::config::Key;

use super::options::{BG_OPTS, MENU_THEME_OPTS, THEME_OPTS};
use super::widgets::{accent_row, card, color_row, combo, slider, toggle, Ctx};

pub(super) fn page(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Accent colour"),
        "",
        |ctx, ui| {
            accent_row(ctx, ui);
            toggle(
                ctx,
                ui,
                Key::UI_ACCENT_DOCK,
                nooksill_core::i18n::t("Apply to the Dock bar"),
                nooksill_core::i18n::t("Use the accent in the bar gradient and running dots"),
            );
            toggle(
                ctx,
                ui,
                Key::UI_ACCENT_DOCK_ICONS,
                nooksill_core::i18n::t("Apply accent to Dock icons"),
                nooksill_core::i18n::t(
                    "Tint the Start and Launchpad icon plates; off falls back to blue",
                ),
            );
            toggle(
                ctx,
                ui,
                Key::UI_ACCENT_CC_ICONS,
                nooksill_core::i18n::t("Apply accent to Control Center icons"),
                nooksill_core::i18n::t("Tint tile outlines; off uses a neutral foreground"),
            );
        },
    );

    card(ctx, ui, nooksill_core::i18n::t("Size"), "", |ctx, ui| {
        slider(
            ctx,
            ui,
            Key::IconSize,
            nooksill_core::i18n::t("Icon size"),
            " px",
        );
        slider(
            ctx,
            ui,
            Key::ZoomSize,
            nooksill_core::i18n::t("Hovered icon size"),
            " px",
        );
        slider(
            ctx,
            ui,
            Key::Spacing,
            nooksill_core::i18n::t("Icon spacing"),
            " px",
        );
        slider(
            ctx,
            ui,
            Key::Padding,
            nooksill_core::i18n::t("Bar padding (bar height)"),
            " px",
        );
        slider(
            ctx,
            ui,
            Key::Radius,
            nooksill_core::i18n::t("Bar corner radius"),
            " px",
        );
    });

    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Background"),
        nooksill_core::i18n::t("The bar uses a gradient over a translucent plate. Frost on the three overlays (Launchpad, Settings, context menu) has its own switches and blurs a real desktop capture."),
        |ctx, ui| {
            combo(ctx, ui, Key::Theme, nooksill_core::i18n::t("Dock theme"), THEME_OPTS);
            combo(ctx, ui, Key::BgMode, nooksill_core::i18n::t("Background material"), BG_OPTS);
            slider(ctx, ui, Key::Opacity, nooksill_core::i18n::t("Opacity"), " %");
            slider(ctx, ui, Key::Blur, nooksill_core::i18n::t("Frost blur strength"), "");
            toggle(ctx, ui, Key::Border, nooksill_core::i18n::t("Show bar border"), "");
            toggle(
                ctx,
                ui,
                Key::AccentTone,
                nooksill_core::i18n::t("Follow the Windows accent (solid / gradient)"),
                nooksill_core::i18n::t("Generate a Material You palette from the Windows accent"),
            );
        },
    );

    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Custom colours"),
        "",
        |ctx, ui| {
            color_row(ctx, ui, Key::BgColor1, nooksill_core::i18n::t("Colour 1"));
            color_row(
                ctx,
                ui,
                Key::BgColor2,
                nooksill_core::i18n::t("Colour 2 (gradient end)"),
            );
            color_row(
                ctx,
                ui,
                Key::BG_COLOR3,
                nooksill_core::i18n::t("Colour 3 (gradient midpoint, optional)"),
            );
            slider(
                ctx,
                ui,
                Key::GRAD_A1,
                nooksill_core::i18n::t("Colour 1 opacity"),
                " %",
            );
            slider(
                ctx,
                ui,
                Key::GRAD_A2,
                nooksill_core::i18n::t("Colour 2 opacity"),
                " %",
            );
            slider(
                ctx,
                ui,
                Key::GRAD_A3,
                nooksill_core::i18n::t("Colour 3 opacity"),
                " %",
            );
            slider(
                ctx,
                ui,
                Key::GRAD_ANGLE,
                nooksill_core::i18n::t("Gradient angle"),
                "°",
            );
        },
    );

    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Border & menu"),
        "",
        |ctx, ui| {
            slider(
                ctx,
                ui,
                Key::BORDER_WIDTH,
                nooksill_core::i18n::t("Border width"),
                " px",
            );
            combo(
                ctx,
                ui,
                Key::MENU_THEME,
                nooksill_core::i18n::t("Context menu theme"),
                MENU_THEME_OPTS,
            );
        },
    );

    // Widget look reuses the shared card and slider layout.
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Widget appearance"),
        nooksill_core::i18n::t("0 follows the bar or global frost. Card plate, radius, strip gap and overlay blur are independent"),
        |ctx, ui| {
            slider(ctx, ui, Key::WIDGET_OPACITY, nooksill_core::i18n::t("Card opacity"), " %");
            slider(ctx, ui, Key::WIDGET_RADIUS, nooksill_core::i18n::t("Card corner radius"), " px");
            slider(ctx, ui, Key::WIDGET_GAP, nooksill_core::i18n::t("Strip member gap"), " px");
            slider(ctx, ui, Key::WIDGET_BLUR, nooksill_core::i18n::t("Overlay blur radius"), " px");
            slider(ctx, ui, Key::PANEL_OPACITY, nooksill_core::i18n::t("Panel opacity"), " %");
            slider(ctx, ui, Key::PANEL_OVERLAY, nooksill_core::i18n::t("Panel overlay"), " %");
        },
    );
}
