//! Extensions page: Host Link v1 apps discovered by the host program.
//!
//! This layer is pure UI: it renders a snapshot filled by the dock and emits
//! commands. Probing and process spawning stay in `nooksill-platform`.

use egui::{RichText, Ui};

/// One discovered Host Link app, filled by the host program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionRow {
    pub id: String,
    pub name: String,
    pub exe: String,
    pub ready: bool,
    pub detail: String,
}

/// Display-only catalogue. Discovery never uses this list: any app publishing
/// `mc.host-link/v1` is accepted by the provider.
const SUPPORTED: &[(&str, &str, &str)] = &[(
    "ModularClipboard",
    "https://github.com/MCheng404/ModularClipboard",
    "Clipboard tool with Host Link v1 support.",
)];

/// One discovered plugin pack, filled by the host program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginRow {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub homepage: String,
    pub description: String,
    pub conflicts: Vec<String>,
    pub enabled: bool,
}

use super::widgets::{button, card, switch_row, toggle, Ctx};
use super::Command;

/// Human-readable host-link health codes; the raw code stays in the tooltip.
fn error_label(code: &str) -> &'static str {
    let t = nooksill_core::i18n::t;
    match code {
        "ready" => t("Ready"),
        "not checked" => t("Not checked"),
        "not ready" => t("Not ready"),
        "LINK_NOT_FOUND" => t("Manifest not published yet"),
        "LINK_UNREADABLE" => t("Manifest unreadable"),
        "LINK_SCHEMA_UNSUPPORTED" => t("Unsupported manifest version"),
        "LINK_STALE" => t("Manifest out of date"),
        "EXE_MISSING" => t("Application executable missing"),
        "EXE_CHANGED" => t("Application updated, rescan needed"),
        "DAEMON_NOT_RUNNING" => t("Background service not running"),
        "PUBLISH_FAILED" => t("Could not republish manifest"),
        "ACTIVATE_FAILED" => t("Could not open the application"),
        "" => t("No problem detected"),
        _ => t("Unknown problem"),
    }
}

pub(super) fn page(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    let t = nooksill_core::i18n::t;

    card(ctx, ui, t("Extensions"), "", |ctx, ui| {
        toggle(
            ctx,
            ui,
            nooksill_core::config::Key::HOST_LINK_ENABLED,
            t("Enable extensions"),
            t("Inject discovered apps into the launchpad."),
        );
        toggle(
            ctx,
            ui,
            nooksill_core::config::Key::HOST_LINK_AUTOSCAN,
            t("Rescan on launchpad open"),
            t("Re-read every published manifest each time."),
        );
        ui.add_space(6.0);
        button(ctx, ui, t("Rescan now"), Command::HostLinkRescan);
    });

    card(ctx, ui, t("Discovered extensions"), "", |ctx, ui| {
        if ctx.state.extensions.is_empty() {
            ui.label(
                RichText::new(t(
                    "No extensions found. Start an app that publishes a manifest, then rescan.",
                ))
                .small()
                .weak(),
            );
            return;
        }
        let rows = ctx.state.extensions.clone();
        for row in rows {
            ui.add_space(4.0);
            ui.label(RichText::new(&row.name).strong());
            ui.label(RichText::new(&row.id).small().weak());
            ui.label(RichText::new(&row.exe).small().weak());
            let status = if row.ready {
                t("Ready")
            } else {
                t("Not ready")
            };
            ui.label(RichText::new(format!("{status} · {}", error_label(&row.detail))).small())
                .on_hover_text(&row.detail);
            ui.horizontal(|ui| {
                if ui.button(t("Health check")).clicked() {
                    ctx.out.command = Some(Command::HostLinkCheck(row.id.clone()));
                }
                if ui.button(t("Open")).clicked() {
                    ctx.out.command = Some(Command::HostLinkActivate(row.id.clone()));
                }
                if ui.button(t("Add to Dock")).clicked() {
                    ctx.out.command = Some(Command::HostLinkAddToDock {
                        id: row.id.clone(),
                        name: row.name.clone(),
                    });
                }
            });
        }
    });

    card(
        ctx,
        ui,
        t("Supported extensions"),
        t("Apps known to publish a Host Link v1 manifest."),
        |ctx, ui| {
            for &(name, url, note) in SUPPORTED {
                ui.horizontal(|ui| {
                    if ui.button(name).clicked() {
                        ctx.out.command = Some(Command::OpenUrl(url));
                    }
                    ui.label(RichText::new(t(note)).small().weak());
                });
            }
        },
    );

    card(ctx, ui, t("Plugins"), "", |ctx, ui| {
        button(ctx, ui, t("Rescan plugins"), Command::PluginRescan);
        if ctx.state.plugins.is_empty() {
            ui.label(
                RichText::new(t(
                    "No plugins found. Add a folder with plugin.json, then rescan.",
                ))
                .small()
                .weak(),
            );
            return;
        }
        let rows = ctx.state.plugins.clone();
        for row in rows {
            ui.add_space(4.0);
            let label = format!("{} {} {}", row.name, t("Version"), row.version);
            if !row.description.is_empty() {
                ui.label(RichText::new(&row.description).small().weak());
            }
            if !row.conflicts.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "{}: {}",
                        t("Overlapping settings"),
                        row.conflicts.join(", ")
                    ))
                    .small()
                    .weak(),
                );
            }
            if let Some(on) = switch_row(ui, ctx.state, &label, &row.author, row.enabled) {
                ctx.out.command = Some(Command::PluginToggle(row.id.clone(), on));
            }
            if !row.homepage.is_empty() && ui.button(t("Open plugin page")).clicked() {
                ctx.out.command = Some(Command::OpenDynamicUrl(row.homepage.clone()));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::error_label;

    #[test]
    fn health_codes_have_readable_labels() {
        assert_ne!(error_label("DAEMON_NOT_RUNNING"), "DAEMON_NOT_RUNNING");
        assert_ne!(error_label("EXE_CHANGED"), "EXE_CHANGED");
        assert_ne!(error_label("ACTIVATE_FAILED"), "ACTIVATE_FAILED");
        assert_ne!(error_label("unknown-code"), "unknown-code");
    }
}
