use super::apply::apply_plugins;
use super::parse::parse_plugin;
use super::*;

static APPLY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
use std::path::Path;

mod parsing {
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
            palette: Vec::new(),
            color_rules: Vec::new(),
            text: Vec::new(),
            dock_items: Vec::new(),
            icon_map: Vec::new(),
            hotkeys: None,
        }
    }

    #[test]
    fn disabling_a_plugin_restores_the_saved_value() {
        let _guard = APPLY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
        let _guard = APPLY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut cfg = Config::defaults();
        cfg.apply_i32(Key::GLOW_STRENGTH, 50);
        let pack = plugin(Key::GLOW_STRENGTH, 20, None);
        apply_plugins(&mut cfg, std::slice::from_ref(&pack), "com.example.theme");
        apply_plugins(&mut cfg, std::slice::from_ref(&pack), "com.example.theme");
        assert_eq!(cfg.plugin_base, "GlowStrength=50");
    }
}

#[cfg(test)]
mod label_color_tests {
    use super::*;

    const COLOR_PLUGIN: &str = r#"{
      "schema": "mc.dock-plugin/v1",
      "plugin": {
        "id": "com.example.colors",
        "name": "Colours",
        "version": "1.0.0"
      },
      "contributes": {
        "labelColors": {
          "palette": [
            { "name": " coral ", "rgb": [255, 80, 80] },
            { "name": "sky", "rgb": [80, 160, 255] }
          ],
          "rules": [
            { "match": "Game", "rgb": [255, 80, 80] }
          ]
        }
      }
    }"#;

    #[test]
    fn label_palette_and_rules_are_parsed() {
        let p = parse_plugin(COLOR_PLUGIN, Path::new("plugin.json")).expect("valid plugin");
        assert_eq!(p.palette.len(), 2);
        assert_eq!(p.palette[0].0, " coral ");
        assert_eq!(p.palette[0].1, [255, 80, 80]);
        assert_eq!(p.color_rules.len(), 1);
        assert_eq!(p.color_rules[0], ("Game".to_owned(), [255, 80, 80]));
    }

    #[test]
    fn invalid_color_channels_are_rejected() {
        let bad = COLOR_PLUGIN.replace("[255, 80, 80]", "[256, 80, 80]");
        assert!(parse_plugin(&bad, Path::new("plugin.json")).is_err());
    }
}

#[cfg(test)]
mod extension_tests {
    use super::*;

    const FULL: &str = r#"{
      "schema": "mc.dock-plugin/v1",
      "plugin": { "id": "com.example.full", "name": "Full", "version": "1.0.0" },
      "contributes": {
        "text": { "Language": "zh-CN", "Proxy": "http://127.0.0.1:7890", "Bogus": "x" },
        "dockItems": [
          { "name": "Terminal", "path": "C:/Windows/System32/cmd.exe", "kind": 0 }
        ],
        "iconPack": { "Terminal": "terminal.png" },
        "hotkeys": { "toggle": "Ctrl+Alt+T", "launchpad": "Ctrl+Alt+L" }
      }
    }"#;

    #[test]
    fn unknown_text_field_is_rejected() {
        assert_eq!(
            parse_plugin(FULL, Path::new("plugin.json")).err(),
            Some("unknown text field")
        );
    }

    #[test]
    fn all_new_contributions_are_parsed() {
        let ok = FULL.replace(", \"Bogus\": \"x\"", "");
        let p = parse_plugin(&ok, Path::new("plugin.json")).expect("valid");
        assert_eq!(p.text.len(), 2);
        assert_eq!(p.dock_items.len(), 1);
        assert_eq!(p.dock_items[0].name, "Terminal");
        assert_eq!(
            p.icon_map,
            vec![("Terminal".to_owned(), "terminal.png".to_owned())]
        );
        assert_eq!(p.hotkeys.as_ref().map(|h| h.0.as_str()), Some("Ctrl+Alt+T"));
    }

    #[test]
    fn text_and_hotkeys_apply_and_restore() {
        let _guard = APPLY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let ok = FULL.replace(", \"Bogus\": \"x\"", "");
        let p = parse_plugin(&ok, Path::new("full/plugin.json")).expect("valid");
        let mut cfg = Config::defaults();
        cfg.language = "en-US".to_owned();
        let lang = Config::sanitize_language(&cfg.language);
        let toggle = cfg.hotkey_combo;

        apply_plugins(&mut cfg, std::slice::from_ref(&p), "com.example.full");
        assert_eq!(cfg.language, "zh-CN");
        assert_eq!(cfg.proxy, "http://127.0.0.1:7890");
        assert_eq!(cfg.hotkey_combo.display(), "Ctrl+Alt+T");
        assert_eq!(cfg.hotkey_launchpad.display(), "Ctrl+Alt+L");

        apply_plugins(&mut cfg, std::slice::from_ref(&p), "");
        assert_eq!(cfg.language, lang);
        assert_eq!(cfg.hotkey_combo, toggle);
    }
}
