//! Performance page: GPU, live usage, costly features, compatibility, presets.

use egui::{RichText, Ui};
use nooksill_core::config::{BgMode, Config, Key};

use super::options::{CLOCK_SHOW_OPTS, DATE_FMT_OPTS, MSAA_OPTS};
use super::widgets::{button, card, combo, row, slider, toggle, toggle_derived, Ctx};
use super::Command;

pub(super) fn page(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Graphics"),
        "",
        |ctx, ui| {
            adapter_combo(ctx, ui);
            ui.label(
            RichText::new(nooksill_core::i18n::t("Automatic: the backend picks by power preference, usually the integrated GPU. Applies on next start."))
                .small()
                .weak(),
        );
        },
    );

    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Frame rate & sync"),
        nooksill_core::i18n::t(
            "Unlimited by default; cap the frame rate or align to the refresh rate to save power.",
        ),
        |ctx, ui| {
            slider(
                ctx,
                ui,
                Key::FRAME_CAP,
                nooksill_core::i18n::t("Frame cap (0 = unlimited)"),
                " FPS",
            );
            toggle(
                ctx,
                ui,
                Key::VSYNC,
                nooksill_core::i18n::t("VSync"),
                nooksill_core::i18n::t(
                    "Align frames to the DWM refresh rate; overrides the frame cap",
                ),
            );
            combo(
                ctx,
                ui,
                Key::MSAA,
                nooksill_core::i18n::t("Anti-aliasing (applies on next start)"),
                MSAA_OPTS,
            );
            ui.label(
                RichText::new(nooksill_core::i18n::t(
                    "Lowering MSAA noticeably cuts per-window resident VRAM.",
                ))
                .small()
                .weak(),
            );
        },
    );

    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Current usage"),
        "",
        |ctx, ui| {
            ui.monospace(&ctx.state.stats);
        },
    );

    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Costly features"),
        nooksill_core::i18n::t("Each item lists its cost and can be switched off alone."),
        |ctx, ui| {
            // Frost is derived from `BgMode`, not a separate boolean key.
            toggle_derived(
                ctx,
                ui,
                nooksill_core::i18n::t("Frosted glass background"),
                nooksill_core::i18n::t("Medium: desktop capture + GPU blur every 2.5 s"),
                ctx.cfg.bg_mode == BgMode::Frost,
                |cfg, on| cfg.apply_i32(Key::BgMode, if on { 2 } else { 1 }),
            );

            // One switch drives two keys: running dots and the running list.
            toggle_derived(
                ctx,
                ui,
                nooksill_core::i18n::t("Scan running apps (dots / running list)"),
                nooksill_core::i18n::t(
                    "Medium: enumerates windows every 2.5 s; off means almost no background scan",
                ),
                ctx.cfg.scan_enabled(),
                |cfg, on| {
                    cfg.apply_i32(Key::Dots, i32::from(on));
                    cfg.apply_i32(Key::ShowRunning, i32::from(on));
                },
            );

            toggle(
                ctx,
                ui,
                Key::Clock,
                nooksill_core::i18n::t("Time & date"),
                nooksill_core::i18n::t("Low: refreshes every 30 s"),
            );
            combo(
                ctx,
                ui,
                Key::CLOCK_SHOW,
                nooksill_core::i18n::t("Clock content"),
                CLOCK_SHOW_OPTS,
            );
            combo(
                ctx,
                ui,
                Key::DATE_FMT,
                nooksill_core::i18n::t("Date format"),
                DATE_FMT_OPTS,
            );
            toggle(
                ctx,
                ui,
                Key::CLOCK_WEEK,
                nooksill_core::i18n::t("Show weekday"),
                "",
            );
            toggle(
                ctx,
                ui,
                Key::CLOCK_SEC,
                nooksill_core::i18n::t("Show seconds"),
                nooksill_core::i18n::t("Refreshes every second with a small cost"),
            );
            toggle(
                ctx,
                ui,
                Key::Reflect,
                nooksill_core::i18n::t("Icon reflection"),
                nooksill_core::i18n::t("Low: one extra translucent layer"),
            );
        },
    );

    card(
        ctx,
        ui,
        nooksill_core::i18n::t("System & compatibility"),
        "",
        |ctx, ui| {
            toggle(
                ctx,
                ui,
                Key::CLICK_TOGGLE,
                nooksill_core::i18n::t("Click a running app = minimise / restore"),
                nooksill_core::i18n::t("Clicking the front window again sends it back to the Dock"),
            );
            toggle(
                ctx,
                ui,
                Key::MN_AUTO,
                nooksill_core::i18n::t("Show the autostart switch in the context menu"),
                "",
            );
            toggle(
                ctx,
                ui,
                Key::MN_NOTES,
                nooksill_core::i18n::t("Show the Notes switch in the context menu"),
                "",
            );
            toggle(
                ctx,
                ui,
                Key::MN_ADD,
                nooksill_core::i18n::t("Show new-item entries on empty-space right click"),
                "",
            );
        },
    );

    card(ctx, ui, nooksill_core::i18n::t("Presets"), "", |ctx, ui| {
        ui.horizontal(|ui| {
            button(
                ctx,
                ui,
                nooksill_core::i18n::t("Low-usage mode"),
                Command::LowPower,
            );
            button(
                ctx,
                ui,
                nooksill_core::i18n::t("Restore recommended settings"),
                Command::Recommended,
            );
        });
        ui.label(
            RichText::new(nooksill_core::i18n::t("No resident animation timer while idle; GPU frames are submitted only on interaction."))
                .small()
                .weak(),
        );
    });
}

/// GPU selection. Unlike `combo` the value is a string and not a `Key`: the
/// `[Graphics]` section is Rust-only.
fn adapter_combo(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    let mut cur = ctx.cfg.adapter.clone();
    let adapters = ctx.state.adapters.clone();
    row(
        ui,
        ctx.state,
        nooksill_core::i18n::t("GPU"),
        "",
        |ui, id| {
            let text = if cur.is_empty() {
                nooksill_core::i18n::t("Automatic (recommended)")
            } else {
                cur.as_str()
            };
            egui::ComboBox::from_id_salt("gpu_adapter")
                .selected_text(text)
                .width(220.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut cur,
                        String::new(),
                        nooksill_core::i18n::t("Automatic (recommended)"),
                    );
                    for name in &adapters {
                        ui.selectable_value(&mut cur, name.clone(), name);
                    }
                })
                .response
                .labelled_by(id);
        },
    );

    if cur != ctx.cfg.adapter {
        ctx.cfg.adapter = Config::sanitize_adapter(&cur);
        ctx.out.changed = true;
    }

    // Show the adapter actually in use: a name mismatch falls back silently,
    // so the configured value alone would be misleading.
    if ctx.state.active_adapter.is_empty() {
        ui.small(nooksill_core::i18n::t("Active GPU: not initialised yet"));
    } else {
        ui.small(nooksill_core::i18n::fmt(
            "Active GPU: {0}",
            &[ctx.state.active_adapter.to_string().as_str()],
        ));
    }
    if !ctx.state.adapter_note.is_empty() {
        ui.colored_label(ui.visuals().warn_fg_color, &ctx.state.adapter_note);
    }
    if ctx.state.adapters.is_empty() {
        ui.label(
            RichText::new(nooksill_core::i18n::t(
                "No usable GPU enumerated; automatic selection is the only option.",
            ))
            .small()
            .weak(),
        );
    }
}
