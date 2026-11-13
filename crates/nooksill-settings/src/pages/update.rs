//! Update pages: release channel, prompt and auto-install policy, network
//! switches, plus the status card shared with the About page.

use egui::{RichText, Ui};

use nooksill_core::config::Key;

use super::widgets::{button, card, combo, toggle, Ctx, Modal, ModalAction};
use super::{Command, SettingsState};

/// Both update pages use the same option table; combo translates labels.
const CHANNEL_OPTS: &[(i32, &str)] = &[(0, "Stable"), (1, "Beta")];

/// Channel policy and the manual update actions.
pub(super) fn policy_page(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    status_card(ctx, ui);
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Release channel"),
        nooksill_core::i18n::t("Choose which GitHub releases this install follows."),
        |ctx, ui| {
            combo(
                ctx,
                ui,
                Key::UPD_CHANNEL,
                nooksill_core::i18n::t("Update channel"),
                CHANNEL_OPTS,
            );
            toggle(
                ctx,
                ui,
                Key::UPD_CHK,
                nooksill_core::i18n::t("Check for updates daily"),
                nooksill_core::i18n::t(
                    "Runs once a day in the background; manual checks are always available",
                ),
            );
            toggle(
                ctx,
                ui,
                Key::UPD_PROMPT,
                nooksill_core::i18n::t("Show an update popup in Settings"),
                nooksill_core::i18n::t("A centred card appears when a new version is found"),
            );
            toggle(
                ctx,
                ui,
                Key::UPD_AUTO,
                nooksill_core::i18n::t("Update automatically without a popup"),
                nooksill_core::i18n::t("Download and verify silently; the new build replaces this one on the next start"),
            );
            if ctx.cfg.get_i32(Key::UPD_AUTO) != 0 {
                ui.label(
                    RichText::new(nooksill_core::i18n::t(
                        "Automatic updates take priority over the popup.",
                    ))
                    .small()
                    .weak(),
                );
            }
        },
    );
}

/// Network switches that used to hide behind the About page's hidden entry.
pub(super) fn network_page(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Network"),
        nooksill_core::i18n::t(
            "Release metadata comes from GitHub; mirrors only carry the download.",
        ),
        |ctx, ui| {
            toggle(
                ctx,
                ui,
                Key::NET_SECURE_DNS,
                nooksill_core::i18n::t("Use secure DNS"),
                nooksill_core::i18n::t("Check system DNS with DoH; skip direct connections when poisoned and use mirrors"),
            );
            toggle(
                ctx,
                ui,
                Key::NET_MIRROR_ALL,
                nooksill_core::i18n::t("Benchmark all mirrors before choosing"),
                nooksill_core::i18n::t("Off: download as soon as one mirror is fast enough"),
            );
        },
    );
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Experimental"),
        nooksill_core::i18n::t("Changes apply immediately."),
        |ctx, ui| {
            toggle(
                ctx,
                ui,
                Key::FROST_ILLUSION,
                nooksill_core::i18n::t("Experimental: lightweight live frost"),
                nooksill_core::i18n::t("A colour-reprojection blur: one real Gaussian keeps the structure, then a low-frequency colour field tracks the desktop; cuts capture and convolution cost. Off returns to the original Gaussian."),
            );
        },
    );
}

/// Status line plus the buttons allowed by the current update stage.
///
/// Shared with About so both entry points show the same state and emit the
/// same commands.
pub(super) fn status_card(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Update"),
        nooksill_core::i18n::t("Check GitHub for a newer release."),
        |ctx, ui| {
            let view = ctx.state.update.clone();
            let text = if view.status.is_empty() {
                nooksill_core::i18n::t("Not checked yet").to_owned()
            } else {
                view.status.clone()
            };
            ui.label(RichText::new(text).small().weak());
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                if view.busy {
                    ui.label(RichText::new(nooksill_core::i18n::t("Please wait…")).small());
                    return;
                }
                button(
                    ctx,
                    ui,
                    nooksill_core::i18n::t("Check for updates"),
                    Command::CheckUpdate,
                );
                if view.available && !view.ready {
                    button(
                        ctx,
                        ui,
                        nooksill_core::i18n::t("Download and verify"),
                        Command::DownloadUpdate,
                    );
                }
                if view.ready {
                    button(
                        ctx,
                        ui,
                        nooksill_core::i18n::t("Restart and update"),
                        Command::ApplyUpdate,
                    );
                }
            });
        },
    );
}

/// A verified download is a second event: offer an immediate restart, or let
/// the exit-time swap handle it when the user just closes the window.
pub(super) fn maybe_ready_prompt(state: &mut SettingsState, cfg: &nooksill_core::config::Config) {
    if state.update_ready_prompted || state.modal.is_some() {
        return;
    }
    if !state.update.ready || state.update.busy {
        return;
    }
    // Automatic updates explicitly ignore popups.
    if cfg.get_i32(Key::UPD_AUTO) != 0 {
        return;
    }
    let mut modal = Modal::new(
        nooksill_core::i18n::t("Update ready"),
        nooksill_core::i18n::t("The new build has been verified. Restart now to apply it, or let it replace this build on the next launch."),
    );
    modal.badge = nooksill_core::i18n::t("Ready").to_owned();
    modal.actions = vec![
        ModalAction::primary(
            nooksill_core::i18n::t("Restart and update"),
            Command::ApplyUpdate,
        ),
        ModalAction::close(nooksill_core::i18n::t("Later")),
    ];
    state.modal = Some(modal);
    state.update_ready_prompted = true;
}

/// Open the update popup once per window session, never during automatic
/// updates and never twice for the same release.
pub(super) fn maybe_prompt(state: &mut SettingsState, cfg: &nooksill_core::config::Config) {
    if state.update_prompted || state.modal.is_some() {
        return;
    }
    let view = &state.update;
    if !view.available || view.ready || view.busy {
        return;
    }
    if cfg.get_i32(Key::UPD_AUTO) != 0 || cfg.get_i32(Key::UPD_PROMPT) == 0 {
        return;
    }
    let channel = if view.prerelease {
        nooksill_core::i18n::t("Beta")
    } else {
        nooksill_core::i18n::t("Stable")
    };
    let version = if view.version.is_empty() {
        nooksill_core::i18n::t("a newer version").to_owned()
    } else {
        view.version.clone()
    };
    // One or two lines at most: a long release body would push the buttons
    // out of the fixed-height card.
    let detail: String = view
        .notes
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(90)
        .collect();
    // Show the upgrade path explicitly: a beta build can carry a lower-looking
    // version triple than the stable install it was cut from.
    let mut modal = Modal::new(
        nooksill_core::i18n::t("Update available"),
        &nooksill_core::i18n::fmt(
            "{0} → {1} is available to download.",
            &[nooksill_core::VERSION, version.as_str()],
        ),
    )
    .with_detail(&detail);
    modal.badge = channel.to_owned();
    modal.actions = vec![
        ModalAction::primary(
            nooksill_core::i18n::t("Download and verify"),
            Command::DownloadUpdate,
        ),
        ModalAction::close(nooksill_core::i18n::t("Later")),
    ];
    modal.suppress_key = Some(Key::UPD_PROMPT);
    state.modal = Some(modal);
    state.update_prompted = true;
}
