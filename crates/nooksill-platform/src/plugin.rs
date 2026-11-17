//! Declarative plugin packs: themes, settings presets and style overrides.
//!
//! A plugin is a folder containing `plugin.json`. It contributes numeric
//! settings and an accent colour. No code, DLL or script is ever loaded, so a
//! broken or hostile plugin can only change values the Dock already validates.

use crate::json::{self, Json};
use nooksill_core::config::{Config, Key};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

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
    /// Setting keys also changed by another enabled plugin.
    pub conflicts: Vec<String>,
    pub enabled: bool,
}

/// Looks up both upstream and Rust-only keys; `Key::from_name` covers only
/// the upstream table.
fn key_from_name(name: &str) -> Option<Key> {
    Key::from_name(name).or_else(|| {
        Key::EXTRA
            .iter()
            .copied()
            .find(|key| key.name().eq_ignore_ascii_case(name.trim()))
    })
}

/// Parses one `plugin.json`, rejecting unknown keys and malformed values.
pub fn parse_plugin(text: &str, source: &Path) -> Result<Plugin, &'static str> {
    let root = json::parse(text).map_err(|_| "invalid JSON")?;
    if root.get("schema").and_then(Json::as_str) != Some(SCHEMA) {
        return Err("unsupported schema");
    }
    let meta = root.get("plugin").ok_or("missing plugin object")?;
    let text_field = |key: &str| -> Result<String, &'static str> {
        meta.get(key)
            .and_then(Json::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or("missing plugin field")
    };
    let id = text_field("id")?;
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err("invalid plugin id");
    }
    let name = text_field("name")?;
    let version = text_field("version")?;
    let author = meta
        .get("author")
        .and_then(Json::as_str)
        .unwrap_or("")
        .to_owned();
    let homepage = meta
        .get("homepage")
        .and_then(Json::as_str)
        .unwrap_or("")
        .to_owned();
    let description = meta
        .get("description")
        .and_then(Json::as_str)
        .unwrap_or("")
        .to_owned();

    let mut settings = Vec::new();
    if let Some(table) = root.get("contributes").and_then(|c| c.get("settings")) {
        if let Json::Obj(pairs) = table {
            for (key_name, value) in pairs {
                let Some(key) = key_from_name(key_name) else {
                    return Err("unknown setting key");
                };
                let Some(number) = value.as_f64() else {
                    return Err("setting value must be a number");
                };
                if !number.is_finite() || number < i32::MIN as f64 || number > i32::MAX as f64 {
                    return Err("setting value out of range");
                }
                settings.push((key, number as i32));
            }
        } else {
            return Err("settings must be an object");
        }
    }

    let accent = match root.get("contributes").and_then(|c| c.get("accent")) {
        Some(Json::Arr(items)) if items.len() == 3 => {
            let mut rgb = [0_u8; 3];
            for (slot, item) in rgb.iter_mut().zip(items.iter()) {
                let Some(value) = item.as_f64() else {
                    return Err("accent channels must be numbers");
                };
                if !value.is_finite() || !(0.0..=255.0).contains(&value) {
                    return Err("accent channel out of range");
                }
                *slot = value as u8;
            }
            Some(rgb)
        }
        Some(_) => return Err("accent must be three channels"),
        None => None,
    };

    Ok(Plugin {
        id,
        name,
        version,
        author,
        homepage,
        description,
        source: source.to_path_buf(),
        settings,
        accent,
    })
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

/// Applies every enabled plugin's contributions to the live config.
pub fn apply_all(cfg: &mut Config, enabled: &str) {
    apply_plugins(cfg, &fresh(), enabled);
}

fn parse_base(text: &str) -> HashMap<String, i32> {
    let mut out = HashMap::new();
    for pair in text.split(',').filter(|item| !item.is_empty()) {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        if let Ok(value) = value.trim().parse::<i32>() {
            out.insert(name.trim().to_owned(), value);
        }
    }
    out
}

fn write_base(base: &HashMap<String, i32>) -> String {
    let mut pairs: Vec<(&String, &i32)> = base.iter().collect();
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    pairs
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Applies enabled contributions and restores the saved values of keys that
/// are no longer overridden. Kept separate from [`apply_all`] so tests can
/// inject plugins without touching the global scan cache.
fn apply_plugins(cfg: &mut Config, plugins: &[Plugin], enabled: &str) {
    let mut contributions = Vec::new();
    let mut desired = HashSet::new();
    let mut accent = None;
    for plugin in plugins {
        if !is_enabled(enabled, &plugin.id) {
            continue;
        }
        for (key, value) in &plugin.settings {
            desired.insert(key.name().to_owned());
            contributions.push((*key, *value));
        }
        if let Some(rgb) = plugin.accent {
            accent = Some(rgb);
        }
    }

    let mut base = parse_base(&cfg.plugin_base);
    for (key, _) in &contributions {
        let name = key.name().to_owned();
        base.entry(name).or_insert_with(|| cfg.get_i32(*key));
    }
    let restore: Vec<(Key, i32)> = base
        .iter()
        .filter(|(name, _)| !desired.contains(name.as_str()))
        .filter_map(|(name, value)| key_from_name(name).map(|key| (key, *value)))
        .collect();
    for (key, value) in restore {
        cfg.apply_i32(key, value);
        base.remove(key.name());
    }
    for (key, value) in contributions {
        cfg.apply_i32(key, value);
    }
    if let Some(rgb) = accent {
        cfg.runtime_accent = Some(rgb);
    }
    cfg.plugin_base = write_base(&base);
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "schema": "mc.dock-plugin/v1",
      "plugin": {
        "id": "com.example.midnight",
        "name": "Midnight",
        "version": "1.0.0",
        "author": "Someone",
        "homepage": "https://example.com/plugin"
      },
      "contributes": {
        "settings": { "GlowStrength": 20, "NotAKey": 1 },
        "accent": [10, 20, 30]
      }
    }"#;

    #[test]
    fn unknown_setting_key_is_rejected() {
        assert_eq!(
            parse_plugin(SAMPLE, Path::new("plugin.json")).err(),
            Some("unknown setting key")
        );
    }

    #[test]
    fn valid_manifest_keeps_only_known_contributions() {
        let text = r#"{
          "schema": "mc.dock-plugin/v1",
          "plugin": {
            "id": "com.example.midnight",
            "name": "Midnight",
            "version": "1.0",
            "description": "Cool blue accent."
          },
          "contributes": {
            "settings": { "GlowStrength": 20 },
            "accent": [10, 20, 30]
          }
        }"#;
        let plugin = parse_plugin(text, Path::new("plugin.json")).unwrap();
        assert_eq!(plugin.id, "com.example.midnight");
        assert_eq!(plugin.settings, vec![(Key::GLOW_STRENGTH, 20)]);
        assert_eq!(plugin.accent, Some([10, 20, 30]));
        assert_eq!(plugin.description, "Cool blue accent.");
    }

    #[test]
    fn accent_is_bounded() {
        let text = r#"{
          "schema": "mc.dock-plugin/v1",
          "plugin": { "id": "com.example.midnight", "name": "Midnight", "version": "1.0" },
          "contributes": { "accent": [10, 300, 30] }
        }"#;
        assert_eq!(
            parse_plugin(text, Path::new("plugin.json")).err(),
            Some("accent channel out of range")
        );
    }

    #[test]
    fn toggle_keeps_ids_unique_and_ordered() {
        let list = toggle("", "a", true);
        let list = toggle(&list, "b", true);
        assert_eq!(list, "a,b");
        assert_eq!(toggle(&list, "a", false), "b");
        assert_eq!(toggle(&list, "a", true), "b,a");
        assert!(is_enabled(&list, "b"));
        assert!(!is_enabled(&list, "c"));
    }
}

#[cfg(test)]
mod rollback_tests {
    use super::*;

    fn plugin(key: Key, value: i32, accent: Option<[u8; 3]>) -> Plugin {
        Plugin {
            id: "com.example.theme".to_owned(),
            name: "Theme".to_owned(),
            version: "1".to_owned(),
            author: String::new(),
            homepage: String::new(),
            description: String::new(),
            source: PathBuf::new(),
            settings: vec![(key, value)],
            accent,
        }
    }

    #[test]
    fn disabling_a_plugin_restores_the_saved_value() {
        let mut cfg = Config::defaults();
        cfg.apply_i32(Key::GLOW_STRENGTH, 50);
        let pack = plugin(Key::GLOW_STRENGTH, 20, Some([10, 20, 30]));

        apply_plugins(&mut cfg, std::slice::from_ref(&pack), "com.example.theme");
        assert_eq!(cfg.get_i32(Key::GLOW_STRENGTH), 20);
        assert_eq!(cfg.plugin_base, "GlowStrength=50");
        assert_eq!(cfg.runtime_accent, Some([10, 20, 30]));

        apply_plugins(&mut cfg, &[pack], "");
        assert_eq!(cfg.get_i32(Key::GLOW_STRENGTH), 50);
        assert!(cfg.plugin_base.is_empty());
    }

    #[test]
    fn baseline_keeps_the_first_value_across_multiple_applies() {
        let mut cfg = Config::defaults();
        cfg.apply_i32(Key::GLOW_STRENGTH, 50);
        let pack = plugin(Key::GLOW_STRENGTH, 20, None);
        apply_plugins(&mut cfg, std::slice::from_ref(&pack), "com.example.theme");
        apply_plugins(&mut cfg, std::slice::from_ref(&pack), "com.example.theme");
        assert_eq!(cfg.plugin_base, "GlowStrength=50");
    }
}
