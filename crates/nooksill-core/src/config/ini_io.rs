//! Whole-file read and write between `Config` and ini.

use super::*;

impl Config {
    /// Read from the ini. Missing keys keep their default value.
    ///
    /// The 1.x `Frost` key is accepted: `BgMode` missing but `Frost` present
    /// means `Frost=1` -> frosted, `Frost=0` -> solid.
    #[must_use]
    pub fn from_ini(ini: &Ini) -> Self {
        let mut cfg = Self::defaults();
        // 1.x `Frost` is the predecessor of `BgMode`. Deciding whether to look
        // at `Frost` needs both formats: checking the old section only would
        // misjudge a new-format file (which has `fill`, not `BgMode`) as 1.x
        // and let `Frost` override `fill`.
        let has_fill = ini.get("appearance.background", "fill").is_some()
            || ini.get(LEGACY_SECTION, "BgMode").is_some();
        if !has_fill {
            if let Some(frost) = ini.get_i32(LEGACY_SECTION, "Frost") {
                cfg.bg_mode = if frost != 0 {
                    BgMode::Frost
                } else {
                    BgMode::Solid
                };
            }
        }
        // Both formats are read: the new one first, then the old
        // `[Settings]` section with its CamelCase names. An old config
        // therefore loads as-is and becomes the new format on the next save,
        // with no separate migration step and no "migration failed but the
        // old file was overwritten" state.
        for key in Key::all_keys() {
            let found = ini
                .get(key.section(), key.name())
                .or_else(|| ini.get(LEGACY_SECTION, key.legacy_name()));
            if let Some(raw) = found {
                // The Windows profile API returns 0 for a non-numeric value;
                // the same behaviour is kept here, but a parse failure is not
                // silently used as 0 -- the clamp catches it.
                let v = raw.trim().parse::<i32>().unwrap_or(0);
                cfg.apply_i32(key, v);
            }
        }
        cfg.adapter = Self::sanitize_adapter(ini.get("Graphics", "Adapter").unwrap_or_default());
        // An unparseable combination falls back to the historical default, so
        // a typo can never leave the process with no registered hotkey.
        cfg.hotkey_combo = ini
            .get("Hotkeys", "Toggle")
            .and_then(Hotkey::parse)
            .unwrap_or(Hotkey::DEFAULT);
        cfg.cc_order = ini.get("widgets", "Order").unwrap_or_default().to_owned();
        cfg.language = ini
            .get("general", "Language")
            .unwrap_or_default()
            .to_owned();
        cfg.clamp();
        cfg
    }

    /// Write back to the ini, keeping other sections and unknown keys.
    pub fn write_ini(&self, ini: &mut Ini) {
        // The version makes the format generation visible at a glance. Reading
        // does not depend on it (that relies on the key-name fallback), but it
        // helps when diagnosing a config.
        ini.set_i32(META_SECTION, "version", CONFIG_VERSION);
        for key in Key::all_keys() {
            ini.set_i32(key.section(), key.name(), self.get_i32(key));
        }
        // "Automatic" is written as an empty string rather than by removing
        // the key: removing it makes "manually set to automatic" invisible in
        // the file, which reads as "never configured" while diagnosing.
        ini.set("Graphics", "Adapter", &self.adapter);
        ini.set("Hotkeys", "Toggle", self.hotkey_combo.display());
        // The tile order is only written once dragged; an empty line would
        // just add noise to every user's ini. When the value is cleared again
        // (Reset defaults), the old line has to be removed or the next start
        // reads the stale order back.
        if self.cc_order.is_empty() {
            ini.remove_key("widgets", "Order");
        } else {
            ini.set("widgets", "Order", &self.cc_order);
        }
        // An explicit empty line keeps "follow the system" visible in the file.
        ini.set("general", "Language", &self.language);
    }
}
