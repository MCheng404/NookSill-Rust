//! Manifest parsing and validation.

use super::*;
use std::path::Path;

use crate::json::{self, Json};

/// Looks up both upstream and Rust-only keys; `Key::from_name` covers only
/// the upstream table.
pub(crate) fn key_from_name(name: &str) -> Option<Key> {
    Key::from_name(name).or_else(|| {
        Key::EXTRA
            .iter()
            .copied()
            .find(|key| key.name().eq_ignore_ascii_case(name.trim()))
    })
}

/// A palette entry or name rule: `(display name / prefix, RGB)`.
pub type ColorEntry = (String, [u8; 3]);

/// Parse an `RRGGBB` triple from a JSON array of three 0..255 numbers.
fn rgb_of(value: Option<&Json>) -> Result<[u8; 3], &'static str> {
    let Json::Arr(items) = value.ok_or("color must be three channels")? else {
        return Err("color must be three channels");
    };
    if items.len() != 3 {
        return Err("color must be three channels");
    }
    let mut rgb = [0u8; 3];
    for (slot, item) in rgb.iter_mut().zip(items.iter()) {
        let Some(v) = item.as_f64() else {
            return Err("color channels must be numbers");
        };
        if !v.is_finite() || !(0.0..=255.0).contains(&v) {
            return Err("color channel out of range");
        }
        *slot = v as u8;
    }
    Ok(rgb)
}

/// Parse `contributes.labelColors` into `(palette, rules)`.
fn label_colors(root: &Json) -> Result<(Vec<ColorEntry>, Vec<ColorEntry>), &'static str> {
    let Some(colors) = root.get("contributes").and_then(|c| c.get("labelColors")) else {
        return Ok((Vec::new(), Vec::new()));
    };
    let mut palette = Vec::new();
    if let Some(Json::Arr(items)) = colors.get("palette") {
        for item in items {
            let name = item
                .get("name")
                .and_then(Json::as_str)
                .ok_or("palette entry needs a name")?;
            if name.is_empty() || name.len() > 32 || palette.len() >= 16 {
                return Err("invalid palette entry");
            }
            palette.push((name.to_owned(), rgb_of(item.get("rgb"))?));
        }
    }
    let mut rules = Vec::new();
    if let Some(Json::Arr(items)) = colors.get("rules") {
        for item in items {
            let prefix = item
                .get("match")
                .and_then(Json::as_str)
                .ok_or("color rule needs match")?;
            if prefix.is_empty() || prefix.len() > 128 || rules.len() >= 128 {
                return Err("invalid color rule");
            }
            rules.push((prefix.to_owned(), rgb_of(item.get("rgb"))?));
        }
    }
    Ok((palette, rules))
}

/// Parsed non-numeric contributions.
type ExtraContribs = (
    Vec<(TextField, String)>,
    Vec<DockItemSpec>,
    Vec<(String, String)>,
    Option<(String, String)>,
);

/// Parse the remaining `contributes` sections into one bundle.
fn extra_contributes(root: &Json) -> Result<ExtraContribs, &'static str> {
    let Some(c) = root.get("contributes") else {
        return Ok((Vec::new(), Vec::new(), Vec::new(), None));
    };
    let mut text = Vec::new();
    if let Some(Json::Obj(pairs)) = c.get("text") {
        for (name, value) in pairs {
            let field = TextField::from_name(name).ok_or("unknown text field")?;
            let value = value.as_str().ok_or("text value must be a string")?;
            if value.len() > 512 {
                return Err("text value too long");
            }
            text.push((field, value.to_owned()));
        }
    }
    let mut dock_items = Vec::new();
    if let Some(Json::Arr(items)) = c.get("dockItems") {
        for item in items {
            let name = item
                .get("name")
                .and_then(Json::as_str)
                .ok_or("dock item needs a name")?;
            let path = item
                .get("path")
                .and_then(Json::as_str)
                .ok_or("dock item needs a path")?;
            let kind = item
                .get("kind")
                .and_then(Json::as_f64)
                .unwrap_or(0.0)
                .clamp(0.0, 6.0) as i32;
            if name.is_empty() || name.len() > 128 || path.is_empty() || path.len() > 1024 {
                return Err("invalid dock item");
            }
            dock_items.push(DockItemSpec {
                name: name.to_owned(),
                path: path.to_owned(),
                kind,
            });
        }
        if dock_items.len() > 32 {
            return Err("too many dock items");
        }
    }
    let mut icon_map = Vec::new();
    if let Some(Json::Obj(pairs)) = c.get("iconPack") {
        for (key, value) in pairs {
            let file = value.as_str().ok_or("icon value must be a file name")?;
            if key.is_empty()
                || key.len() > 512
                || file.is_empty()
                || file.len() > 256
                || file.contains('/')
                || file.contains('\\')
                || file == "."
                || file == ".."
            {
                return Err("invalid icon override");
            }
            icon_map.push((key.clone(), file.to_owned()));
        }
        if icon_map.len() > 256 {
            return Err("too many icon overrides");
        }
    }
    let hotkeys = match (c.get("hotkeys"), c.get("hotkey")) {
        (Some(h), _) => {
            let toggle = h
                .get("toggle")
                .and_then(Json::as_str)
                .ok_or("hotkeys need toggle")?;
            let lp = h
                .get("launchpad")
                .and_then(Json::as_str)
                .ok_or("hotkeys need launchpad")?;
            if toggle.len() > 64 || lp.len() > 64 {
                return Err("hotkey too long");
            }
            Some((toggle.to_owned(), lp.to_owned()))
        }
        _ => None,
    };
    Ok((text, dock_items, icon_map, hotkeys))
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
                let Some(key) = key_from_name(key_name.as_str()) else {
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

    let (palette, color_rules) = label_colors(&root)?;
    let (text, dock_items, icon_map, hotkeys) = extra_contributes(&root)?;

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
        palette,
        color_rules,
        text,
        dock_items,
        icon_map,
        hotkeys,
    })
}
