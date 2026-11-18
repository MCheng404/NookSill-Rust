//! Applying enabled contributions to live config.

use super::parse::key_from_name;
use super::*;
use std::path::Path;

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
pub(crate) fn apply_plugins(cfg: &mut Config, plugins: &[Plugin], enabled: &str) {
    let mut contributions = Vec::new();
    let mut desired = HashSet::new();
    let mut accent = None;
    let mut palette = Vec::new();
    let mut rules = Vec::new();
    let mut dock_items = Vec::new();
    let mut icon_paths = Vec::new();
    let mut hotkeys = None;
    let mut text_wanted: HashMap<TextField, String> = HashMap::new();
    for plugin in plugins {
        if !is_enabled(enabled, &plugin.id) {
            continue;
        }
        for entry in &plugin.palette {
            if !palette
                .iter()
                .any(|(n, _): &(String, [u8; 3])| n == &entry.0)
            {
                palette.push(entry.clone());
            }
        }
        for rule in &plugin.color_rules {
            rules.push(rule.clone());
        }
        for item in &plugin.dock_items {
            dock_items.push(item.clone());
        }
        let dir = plugin
            .source
            .parent()
            .unwrap_or(Path::new("."))
            .join("icons");
        for (key, file) in &plugin.icon_map {
            let full = dir.join(file);
            if full.is_file() {
                icon_paths.push((key.clone(), full));
            }
        }
        if hotkeys.is_none() {
            hotkeys = plugin.hotkeys.clone();
        }
        for (field, value) in &plugin.text {
            text_wanted.insert(*field, value.clone());
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

    // Snapshot once per field, restore when no enabled pack wants it, then
    // apply the current winner. Overrides stay in memory only.
    if let Ok(mut base) = TEXT_BASE.lock() {
        for field in text_wanted.keys() {
            base.entry(*field).or_insert_with(|| field.get(cfg));
        }
        let restore: Vec<(TextField, String)> = base
            .iter()
            .filter(|(field, _)| !text_wanted.contains_key(field))
            .map(|(field, value)| (*field, value.clone()))
            .collect();
        for (field, value) in restore {
            field.apply(cfg, &value);
            base.remove(&field);
        }
    }
    for (field, value) in text_wanted {
        field.apply(cfg, &value);
    }
    if let Ok(mut base) = HOTKEY_BASE.lock() {
        if hotkeys.is_some() {
            if base.is_none() {
                *base = Some((cfg.hotkey_combo, cfg.hotkey_launchpad));
            }
        } else if let Some((toggle, launchpad)) = base.take() {
            cfg.hotkey_combo = toggle;
            cfg.hotkey_launchpad = launchpad;
        }
    }
    if let Some((toggle, launchpad)) = &hotkeys {
        if let Some(hk) = nooksill_core::hotkey::Hotkey::parse(toggle) {
            cfg.hotkey_combo = hk;
        }
        if let Some(hk) = nooksill_core::hotkey::Hotkey::parse(launchpad) {
            cfg.hotkey_launchpad = hk;
        }
    }

    if let Ok(mut g) = LABEL_PALETTE.lock() {
        *g = palette.clone();
    }
    if let Ok(mut g) = LABEL_RULES.lock() {
        *g = rules.clone();
    }
    if let Ok(mut g) = ACTIVE.lock() {
        *g = Contributions {
            palette,
            rules,
            dock_items,
            icon_map: icon_paths,
            hotkeys,
        };
    }
}
