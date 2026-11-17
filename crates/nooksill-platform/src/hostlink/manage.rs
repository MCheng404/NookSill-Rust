//! Management API for the Extensions settings page.
//!
//! Commands run with a detached provider so the global cache lock is never
//! held across a process spawn; the UI can keep painting while an app starts.

use super::{shared, HostLink, HostLinkProvider, ProbeError, ProbeReport, ProcessRunner};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub(super) fn autoscan() -> bool {
    AUTOSCAN.load(Ordering::Relaxed)
}

/// One row on the Extensions settings page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionInfo {
    pub id: String,
    pub name: String,
    pub exe: String,
    /// `true` only after a successful probe.
    pub ready: bool,
    /// Error code from the last probe, or `ready` / `not checked`.
    pub detail: String,
}

/// Whether manifest apps are injected into the launchpad.
static ENABLED: AtomicBool = AtomicBool::new(true);
/// Whether every launchpad open re-runs discovery.
static AUTOSCAN: AtomicBool = AtomicBool::new(true);

/// Set by the dock from `Key::HOST_LINK_ENABLED`.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Set by the dock from `Key::HOST_LINK_AUTOSCAN`.
pub fn set_autoscan(on: bool) {
    AUTOSCAN.store(on, Ordering::Relaxed);
}

/// Copies one manifest out of the shared cache. Callers run commands with a
/// detached provider so the global lock is never held across a process spawn.
#[must_use]
pub fn link_by_id(id: &str) -> Option<HostLink> {
    let provider = shared()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    provider.links.get(id).cloned()
}

/// Runs a published command set on a private provider (no global lock).
pub fn run_detached(link: HostLink, activate: bool) -> bool {
    let mut provider = HostLinkProvider::with_runner(Box::new(ProcessRunner));
    let id = link.id.clone();
    let exe = link.exe.clone();
    provider.insert(link);
    if activate {
        provider.activate(&exe)
    } else {
        let report = provider.probe(&id);
        let ok = report.ok;
        provider.cache.insert(id, report);
        ok
    }
}

/// Records the worker's outcome so the settings page shows it next frame.
pub fn store_ready(id: &str, ok: bool) {
    let report = if ok {
        ProbeReport {
            ok: true,
            source: "worker".to_owned(),
            ..ProbeReport::default()
        }
    } else {
        ProbeReport {
            error: Some(ProbeError {
                code: "ACTIVATE_FAILED".to_owned(),
                message: String::new(),
            }),
            source: "worker".to_owned(),
            ..ProbeReport::default()
        }
    };
    let mut provider = shared()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    provider.cache.insert(id.to_owned(), report);
}

/// Returns `true` when the path belongs to a published app and its `activate`
/// command ran; the caller falls back to a normal shell open otherwise.
#[must_use]
pub fn activate_path(path: &str) -> bool {
    let mut provider = shared()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    provider.activate(Path::new(path))
}

/// Re-read all manifests now, regardless of the autoscan switch.
pub fn rescan() -> usize {
    let mut provider = shared()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    provider.discover();
    provider.order.len()
}

/// Snapshot for the settings page; never spawns a command.
#[must_use]
pub fn snapshot() -> Vec<ExtensionInfo> {
    let provider = shared()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    provider
        .order
        .iter()
        .filter_map(|id| {
            let link = provider.links.get(id)?;
            let (ready, detail) = match provider.cache.get(id) {
                Some(report) if report.ok => (true, "ready".to_owned()),
                Some(report) => (
                    false,
                    report
                        .error
                        .as_ref()
                        .map_or_else(|| "not ready".to_owned(), |e| e.code.clone()),
                ),
                None => (false, "not checked".to_owned()),
            };
            Some(ExtensionInfo {
                id: link.id.clone(),
                name: link.name.clone(),
                exe: link.exe.to_string_lossy().into_owned(),
                ready,
                detail,
            })
        })
        .collect()
}

/// Runs `probe`, follows its `repair` steps, then probes again.
pub fn check(id: &str) -> bool {
    let mut provider = shared()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(link) = provider.links.get(id).cloned() else {
        return false;
    };
    let mut report = provider.probe(id);
    if !report.ok {
        for step in &report.repair {
            if let Some(command) = link.commands.get(&step.command) {
                let _ = provider.runner.run(&command.exe, &command.args);
            }
        }
        if report.needs_reread() {
            provider.discover();
        }
        report = provider.probe(id);
    }
    provider.cache.insert(id.to_owned(), report.clone());
    report.ok
}

/// Runs `commands.activate` for one published id.
pub fn activate_id(id: &str) -> bool {
    let mut provider = shared()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(exe) = provider.links.get(id).map(|link| link.exe.clone()) else {
        return false;
    };
    provider.activate(&exe)
}
