//! Declarative plugin packs: themes, settings presets and style overrides.
//!
//! A plugin is a folder containing `plugin.json`. It contributes config values,
//! icons, Dock entries and style rules. No code, DLL or script is ever loaded.

use nooksill_core::config::{Config, Key};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

mod apply;
mod parse;

pub use apply::apply_all;
pub use parse::parse_plugin;

/// String config fields a plugin may set. Whitelist only: arbitrary keys would
/// let a malformed pack corrupt the ini.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextField {
    Adapter,
    Language,
    ResOrder,
    CcOrder,
    Proxy,
}

impl TextField {
    fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "adapter" => Some(Self::Adapter),
            "language" => Some(Self::Language),
            "resorder" => Some(Self::ResOrder),
            "ccorder" => Some(Self::CcOrder),
            "proxy" => Some(Self::Proxy),
            _ => None,
        }
    }

    /// Apply with the same sanitizer the ini reader uses.
    pub fn apply(self, cfg: &mut Config, value: &str) {
        match self {
            Self::Adapter => cfg.adapter = Config::sanitize_adapter(value),
            Self::Language => cfg.language = Config::sanitize_language(value),
            Self::ResOrder => cfg.res_order = value.chars().take(256).collect(),
            Self::CcOrder => cfg.cc_order = value.chars().take(256).collect(),
            Self::Proxy => cfg.proxy = Config::sanitize_proxy(value),
        }
    }

    /// Current value, used to snapshot the pre-plugin base.
    #[must_use]
    pub fn get(self, cfg: &Config) -> String {
        match self {
            Self::Adapter => cfg.adapter.clone(),
            Self::Language => cfg.language.clone(),
            Self::ResOrder => cfg.res_order.clone(),
            Self::CcOrder => cfg.cc_order.clone(),
            Self::Proxy => cfg.proxy.clone(),
        }
    }
}

/// One Dock entry declared by a plugin, appended while the pack is enabled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockItemSpec {
    pub name: String,
    pub path: String,
    pub kind: i32,
}

/// Palette entry or name rule: `(display name / prefix, RGB)`.
pub type ColorEntry = (String, [u8; 3]);

/// Only this manifest schema is understood.
pub const SCHEMA: &str = "mc.dock-plugin/v1";
const MANIFEST: &str = "plugin.json";

/// One validated plugin pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub homepage: String,
    pub description: String,
    pub source: PathBuf,
    /// Numeric config keys, applied with `Config::apply_i32` (clamped).
    pub settings: Vec<(Key, i32)>,
    pub accent: Option<[u8; 3]>,
    pub palette: Vec<ColorEntry>,
    pub color_rules: Vec<ColorEntry>,
    /// String config fields, sanitised by [`TextField::apply`].
    pub text: Vec<(TextField, String)>,
    /// Dock entries appended while the pack is enabled.
    pub dock_items: Vec<DockItemSpec>,
    /// Icon overrides: `(app name or path, file name under icons/)`.
    pub icon_map: Vec<(String, String)>,
    /// Hotkey strings: `(Toggle, Launchpad)`.
    pub hotkeys: Option<(String, String)>,
}

/// One row on the Extensions settings page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub homepage: String,
    pub description: String,
    pub conflicts: Vec<String>,
    pub enabled: bool,
}

/// Name rules from enabled plugins, used to auto-colour matching labels.
static LABEL_RULES: Mutex<Vec<(String, [u8; 3])>> = Mutex::new(Vec::new());

/// Rules contributed by enabled official plugins.
#[must_use]
pub fn label_rules() -> Vec<(String, [u8; 3])> {
    LABEL_RULES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Pre-plugin string values, restored when a pack is disabled. Runtime only:
/// plugin text overrides never touch the ini, so a restart with the pack off
/// already sees the user's own value.
static TEXT_BASE: std::sync::LazyLock<Mutex<HashMap<TextField, String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
static HOTKEY_BASE: Mutex<Option<(nooksill_core::hotkey::Hotkey, nooksill_core::hotkey::Hotkey)>> =
    Mutex::new(None);

static ACTIVE: Mutex<Contributions> = Mutex::new(Contributions {
    palette: Vec::new(),
    rules: Vec::new(),
    dock_items: Vec::new(),
    icon_map: Vec::new(),
    hotkeys: None,
});

/// Snapshot of every enabled pack's non-config contributions.
#[must_use]
pub fn active() -> Contributions {
    ACTIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Exposes plugin contributions to other layers without code loading.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Contributions {
    pub palette: Vec<ColorEntry>,
    pub rules: Vec<ColorEntry>,
    pub dock_items: Vec<DockItemSpec>,
    /// Resolved icon files, passed to the icon loader as `icon_src`.
    pub icon_map: Vec<(String, PathBuf)>,
    pub hotkeys: Option<(String, String)>,
}

/// Palette shared with the Launchpad label-colour menu.
static LABEL_PALETTE: Mutex<Vec<(String, [u8; 3])>> = Mutex::new(Vec::new());

/// Palette entries contributed by enabled official plugins.
#[must_use]
pub fn label_palette() -> Vec<(String, [u8; 3])> {
    LABEL_PALETTE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

fn cache() -> &'static Mutex<(Instant, Vec<Plugin>)> {
    static CACHE: OnceLock<Mutex<(Instant, Vec<Plugin>)>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new((Instant::now(), scan())))
}

/// Returns the cached scan, refreshing it at most once every five seconds so
/// newly dropped-in plugin folders appear without a manual rescan.
fn fresh() -> Vec<Plugin> {
    let mut cache = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if cache.0.elapsed() > Duration::from_secs(5) {
        cache.1 = scan();
        cache.0 = Instant::now();
    }
    cache.1.clone()
}

fn scan() -> Vec<Plugin> {
    let mut roots = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        roots.push(PathBuf::from(appdata).join("nooksill").join("plugins"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.join("plugins"));
        }
    }
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let manifest = dir.join(MANIFEST);
            if std::fs::metadata(&manifest).is_ok_and(|m| m.len() > 64 * 1024) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&manifest) else {
                continue;
            };
            let Ok(plugin) = parse_plugin(&text, &manifest) else {
                continue;
            };
            if seen.insert(plugin.id.clone()) {
                found.push(plugin);
            }
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// Re-reads every plugin folder; returns the number of valid packs found.
pub fn rescan() -> usize {
    let plugins = scan();
    let count = plugins.len();
    let mut cached = cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *cached = (Instant::now(), plugins);
    count
}

#[must_use]
pub fn is_enabled(list: &str, id: &str) -> bool {
    list.split(',').any(|item| item == id)
}

/// Adds or removes one id from the comma-separated enabled list.
#[must_use]
pub fn toggle(list: &str, id: &str, on: bool) -> String {
    let mut ids: Vec<&str> = list.split(',').filter(|item| !item.is_empty()).collect();
    ids.retain(|item| *item != id);
    if on {
        ids.push(id);
    }
    ids.join(",")
}

/// Rows for the settings page; never applies any plugin.
#[must_use]
pub fn snapshot(enabled: &str) -> Vec<PluginInfo> {
    let plugins = fresh();
    let mut counts: HashMap<String, usize> = HashMap::new();
    for plugin in &plugins {
        if is_enabled(enabled, &plugin.id) {
            for (key, _) in &plugin.settings {
                *counts.entry(key.name().to_owned()).or_default() += 1;
            }
        }
    }
    plugins
        .into_iter()
        .map(|plugin| PluginInfo {
            conflicts: plugin
                .settings
                .iter()
                .filter(|(key, _)| counts.get(key.name()).is_some_and(|count| *count > 1))
                .map(|(key, _)| key.name().to_owned())
                .collect(),
            enabled: is_enabled(enabled, &plugin.id),
            id: plugin.id,
            name: plugin.name,
            version: plugin.version,
            author: plugin.author,
            homepage: plugin.homepage,
            description: plugin.description,
        })
        .collect()
}

#[cfg(test)]
mod tests;
