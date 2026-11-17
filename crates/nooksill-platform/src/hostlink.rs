//! Generic Host Link v1 provider.
//!
//! Discovers any app publishing `mc.host-link/v1`, validates its manifest and
//! runs only the commands the manifest publishes. No application name, version,
//! window title or private pipe appears in the logic: the bootstrap paths below
//! are the protocol's fixed entry points, everything else comes from the JSON.

mod manage;
mod model;

pub use manage::*;
pub use model::{
    parse_manifest, CommandSpec, FileEndpoints, HostLink, ParseFailure, ProbeError, ProbeReport,
    RepairStep, SCHEMA,
};

use nooksill_core::launchpad::AppEntry;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};

/// Path scheme for a Host Link app pinned as a Dock item.
pub const PATH_PREFIX: &str = "mc.host-link:";

/// Builds the Dock item path for one published app id.
#[must_use]
pub fn widget_path(id: &str) -> String {
    format!("{PATH_PREFIX}{id}")
}

/// Reads the app id back from a Dock item path.
#[must_use]
pub fn id_from_path(path: &str) -> Option<&str> {
    path.strip_prefix(PATH_PREFIX).filter(|id| !id.is_empty())
}

/// Protocol bootstrap constants (section 2). These locate manifests, never
/// identify an application: identity is `app.id` from the parsed document.
const MANIFEST_FILE: &str = "host-link.json";
const PROBE_FILE: &str = "host-probe.json";
const LAST_FILE: &str = "host-last.json";
const ENV_PATH: &str = "MODULAR_CLIPBOARD_HOST_LINK";
const REG_SUBKEY: &str = r"Software\ModularClipboard";
const REG_VALUE: &str = "HostLinkPath";
const START_MENU: &str = r"Microsoft\Windows\Start Menu\Programs";

/// Result of one discovery pass.
#[derive(Debug, Default)]
pub struct Discovery {
    pub link: Option<HostLink>,
    /// Channel 6 (`.lnk`) fallback when no manifest yielded a usable link.
    pub shortcut: Option<PathBuf>,
    /// Paths actually tried, in protocol order; for diagnostics.
    pub visited: Vec<PathBuf>,
}

/// Captured stdout+exit code of a spawned command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandOutput {
    pub code: i32,
    pub stdout: String,
}

/// Injectable command runner: production uses `Command::output`, tests fake it.
pub trait Runner: std::fmt::Debug + Send + Sync {
    fn run(&self, exe: &Path, args: &[String]) -> Option<CommandOutput>;
}

/// GUI-subsystem binaries still write JSON to `Command::output()`'s pipe.
#[derive(Debug, Default)]
pub struct ProcessRunner;

impl Runner for ProcessRunner {
    fn run(&self, exe: &Path, args: &[String]) -> Option<CommandOutput> {
        // A GUI app can stall before writing its JSON (first-run prompt, busy
        // daemon). Bound the wait so neither the worker nor the UI can hang.
        const TIMEOUT: Duration = Duration::from_secs(2);
        let mut child = std::process::Command::new(exe)
            .args(args)
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = child.try_wait().ok().flatten() {
                let mut stdout = String::new();
                if let Some(mut pipe) = child.stdout.take() {
                    let _ = pipe.read_to_string(&mut stdout);
                }
                return Some(CommandOutput {
                    code: status.code().unwrap_or(-1),
                    stdout,
                });
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Cache of discovered links, keyed by `app.id`.
#[derive(Debug)]
pub struct HostLinkProvider {
    runner: Box<dyn Runner>,
    links: HashMap<String, HostLink>,
    order: Vec<String>,
    last_shortcut: Option<PathBuf>,
    cache: HashMap<String, ProbeReport>,
}

impl Default for HostLinkProvider {
    fn default() -> Self {
        Self::with_runner(Box::new(ProcessRunner))
    }
}

impl HostLinkProvider {
    #[must_use]
    pub fn with_runner(runner: Box<dyn Runner>) -> Self {
        Self {
            runner,
            links: HashMap::new(),
            order: Vec::new(),
            last_shortcut: None,
            cache: HashMap::new(),
        }
    }

    /// Try the protocol's six channels in order; a missing or invalid path
    /// only skips that channel.
    pub fn discover(&mut self) -> Discovery {
        let (paths, shortcut) = bootstrap();
        let mut result = self.discover_from(&paths);
        result.shortcut = shortcut.clone();
        self.last_shortcut = shortcut;
        result
    }

    /// Discovery with injected paths, used by the fault-injection acceptance
    /// tests. Order is preserved and every candidate is reported in `visited`.
    pub fn discover_from(&mut self, paths: &[PathBuf]) -> Discovery {
        let mut result = Discovery::default();
        for path in paths {
            result.visited.push(path.clone());
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            let Ok(link) = parse_manifest(&text, path) else {
                continue;
            };
            result.link = Some(link);
            break;
        }
        if let Some(link) = result.link.clone() {
            self.insert(link);
        }
        result
    }

    fn insert(&mut self, link: HostLink) {
        if !self.links.contains_key(&link.id) {
            self.order.push(link.id.clone());
        }
        // Re-reading refreshes the exe and its fingerprint on LINK_STALE /
        // EXE_CHANGED / EXE_MISSING.
        self.links.insert(link.id.clone(), link);
    }

    /// Launchpad candidates: display name plus the manifest's real exe path,
    /// so the existing icon pipeline works without an app-specific branch.
    #[must_use]
    pub fn entries(&self) -> Vec<AppEntry> {
        self.order
            .iter()
            .filter_map(|id| self.links.get(id))
            .map(|link| AppEntry {
                name: link.name.clone(),
                path: link.exe.to_string_lossy().into_owned(),
            })
            .collect()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }

    #[must_use]
    pub fn shortcut(&self) -> Option<&Path> {
        self.last_shortcut.as_deref()
    }

    #[must_use]
    pub fn link_for_exe(&self, exe: &Path) -> Option<&HostLink> {
        self.links.values().find(|link| same_path(&link.exe, exe))
    }

    /// Full click path: probe, follow `repair`, re-read on fingerprint errors,
    /// then run `commands.activate` verbatim.
    pub fn activate(&mut self, exe: &Path) -> bool {
        let Some(link) = self.link_for_exe(exe).cloned() else {
            return false;
        };
        let report = self.probe(&link.id);
        if !report.ok {
            // `repair` frequently contains `activate` itself (DAEMON_NOT_RUNNING).
            // Running it again would flash two windows or restart the daemon
            // twice, so remember whether the repair step already did it.
            let mut activated_in_repair = false;
            for step in &report.repair {
                if let Some(command) = link.commands.get(&step.command) {
                    let result = self.runner.run(&command.exe, &command.args);
                    if step.command == "activate" {
                        activated_in_repair = result.is_some();
                    }
                }
            }
            if activated_in_repair {
                return true;
            }
            if report.needs_reread() {
                self.discover();
            }
        }
        match link.commands.get("activate") {
            Some(command) => self
                .runner
                .run(&command.exe, &command.args)
                .is_some_and(|out| out.code == 0 || out.stdout.contains("\"ok\"")),
            None => false,
        }
    }

    /// Probe via stdout, falling back to the mirrored files when no pipe is
    /// available (GUI-only hosts, broken console, blocked spawn).
    #[must_use]
    pub fn probe(&self, id: &str) -> ProbeReport {
        let Some(link) = self.links.get(id) else {
            return ProbeReport {
                error: Some(ProbeError {
                    code: "LINK_NOT_FOUND".to_owned(),
                    message: String::new(),
                }),
                ..ProbeReport::default()
            };
        };
        if let Some(command) = link.commands.get("probe") {
            if let Some(out) = self.runner.run(&command.exe, &command.args) {
                if let Some(report) = ProbeReport::parse(&out.stdout, "stdout") {
                    return report;
                }
            }
        }
        self.file_probe(link)
    }

    fn file_probe(&self, link: &HostLink) -> ProbeReport {
        let dir = link
            .files
            .link
            .as_deref()
            .and_then(Path::parent)
            .or_else(|| link.discovered_at.parent())
            .unwrap_or(Path::new(""));
        let probe_path = link
            .files
            .probe
            .clone()
            .unwrap_or_else(|| dir.join(PROBE_FILE));
        let last_path = link
            .files
            .last_result
            .clone()
            .unwrap_or_else(|| dir.join(LAST_FILE));
        // Mirrored files survive process crashes; a stale report would keep a
        // recovered app marked unhealthy long after it came back.
        const MAX_AGE_SECS: u64 = 300;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        for (path, label) in [(probe_path, "file:probe"), (last_path, "file:last")] {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Some(report) = ProbeReport::parse(&text, label) {
                    let fresh = report
                        .checked_unix
                        .is_none_or(|checked| now.saturating_sub(checked) <= MAX_AGE_SECS);
                    if fresh {
                        return report;
                    }
                }
            }
        }
        ProbeReport {
            error: Some(ProbeError {
                code: "LINK_UNREADABLE".to_owned(),
                message: "no probe output or mirrored file".to_owned(),
            }),
            source: "unavailable".to_owned(),
            ..ProbeReport::default()
        }
    }

    /// Run a named command from the manifest, if that capability is published.
    pub fn run_named(&self, id: &str, name: &str) -> bool {
        let Some(link) = self.links.get(id) else {
            return false;
        };
        match link.commands.get(name) {
            Some(command) => self
                .runner
                .run(&command.exe, &command.args)
                .is_some_and(|out| out.code == 0 || out.stdout.contains("\"ok\"")),
            None => false,
        }
    }
}

/// Process-wide provider: the launchpad scanner and its click handler live in
/// different call sites, both on the UI thread.
#[must_use]
pub fn shared() -> &'static std::sync::Mutex<HostLinkProvider> {
    use std::sync::OnceLock;
    static PROVIDER: OnceLock<std::sync::Mutex<HostLinkProvider>> = OnceLock::new();
    PROVIDER.get_or_init(|| std::sync::Mutex::new(HostLinkProvider::default()))
}

/// Re-run discovery and return launchpad candidates from all published apps.
#[must_use]
pub fn refresh() -> Vec<AppEntry> {
    if !manage::enabled() {
        return Vec::new();
    }
    let mut provider = shared()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if manage::autoscan() || provider.is_empty() {
        provider.discover();
    }
    provider.entries()
}

/// Finds the published app id for an executable path (launchpad entries use
/// real exe paths so the shell icon pipeline can read them).
#[must_use]
pub fn id_for_exe(exe: &Path) -> Option<String> {
    let provider = shared()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    provider
        .links
        .values()
        .find(|link| same_path(&link.exe, exe))
        .map(|link| link.id.clone())
}

/// Protocol channels in order. Channel 6 is a shortcut, not a manifest, so it
/// is returned separately for `.lnk`-only hosts.
fn bootstrap() -> (Vec<PathBuf>, Option<PathBuf>) {
    let start_menu = std::env::var("APPDATA")
        .ok()
        .map(|root| PathBuf::from(root).join(START_MENU));
    let shortcut = start_menu
        .as_ref()
        .map(|dir| dir.join("ModularClipboard.lnk"))
        .filter(|p| p.is_file());
    // Exe dirs are needed for channel 3; derive them from the shortcut target
    // and the registry pointer without consuming those channels as manifests.
    let mut exe_dirs = Vec::new();
    if let Some(path) = shortcut.as_deref().and_then(resolve_target) {
        if let Some(parent) = path.parent() {
            exe_dirs.push(parent.to_path_buf());
        }
    }
    if let Some(path) = registry_path() {
        if let Some(parent) = path.parent() {
            exe_dirs.push(parent.to_path_buf());
        }
    }

    let mut out = Vec::new();
    // 1: environment override, file or directory.
    if let Ok(raw) = std::env::var(ENV_PATH) {
        if !raw.trim().is_empty() {
            let path = PathBuf::from(raw);
            if path.is_dir() {
                out.push(path.join(MANIFEST_FILE));
            } else {
                out.push(path);
            }
        }
    }
    // 2: primary per-user manifest.
    if let Ok(root) = std::env::var("APPDATA") {
        out.push(
            PathBuf::from(root)
                .join("modular-clipboard")
                .join(MANIFEST_FILE),
        );
    }
    // 3: portable / moved installation, next to the executable.
    for dir in exe_dirs {
        out.push(dir.join(MANIFEST_FILE));
    }
    // 4: Start Menu copy.
    if let Some(dir) = start_menu.as_ref() {
        out.push(dir.join(MANIFEST_FILE));
    }
    // 5: registry pointer to the primary manifest.
    if let Some(path) = registry_path() {
        out.push(path);
    }
    (out, shortcut)
}

fn resolve_target(lnk: &Path) -> Option<PathBuf> {
    let text = lnk.to_string_lossy();
    let shortcut = crate::shortcut::resolve(&text)?;
    Some(PathBuf::from(shortcut.target))
}

fn registry_path() -> Option<PathBuf> {
    let sub = wide(REG_SUBKEY);
    let value = wide(REG_VALUE);
    let mut buffer = [0_u16; 1024];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: the buffer and byte-size agree; both strings are NUL-terminated.
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if rc != WIN32_ERROR(0) {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1).min(buffer.len());
    let text = String::from_utf16_lossy(&buffer[..len]);
    (!text.is_empty()).then(|| PathBuf::from(text))
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests;
