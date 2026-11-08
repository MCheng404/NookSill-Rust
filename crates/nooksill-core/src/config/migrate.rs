//! Config version and in-place DPI unit migration.
//!
//! `upgrade_legacy` runs first (1.x -> current format), then `migrate_units`
//! (v3 -> v4 logical pixels).

use super::*;

impl Config {
    /// Migrate length keys from physical pixels to 96 DPI logical pixels
    /// (`CONFIG_VERSION` 3 -> 4).
    ///
    /// The old format treated `IconSize=72` as physical pixels, so the same
    /// config gave a 150%-scaled desktop two thirds of the intended icon size
    /// and did not follow a DPI change. The new format stores logical pixels
    /// and multiplies by `dpi / 96` at render time, so an old file has to be
    /// converted once on first read.
    ///
    /// Only keys recognized by [`Key::is_length`] are touched; percentages,
    /// angles and colors stay as they are. Returns whether anything was
    /// written, so the caller can decide whether to persist.
    ///
    /// 1.x files (no `[meta] version`) are not handled here:
    /// [`Self::upgrade_legacy`] rewrites them as a whole, and that pass
    /// converts the units.
    pub fn migrate_units(ini: &mut Ini, dpi: i32) -> bool {
        let ver = ini.get_i32(META_SECTION, "version").unwrap_or(0);
        if ver >= CONFIG_VERSION {
            return false;
        }
        if ver == 0 {
            return false;
        }
        if dpi > 0 && dpi != 96 {
            for key in Key::all_keys() {
                if !key.is_length() {
                    continue;
                }
                if let Some(v) = ini.get_i32(key.section(), key.name()) {
                    ini.set_i32(key.section(), key.name(), units::unscale(v, dpi));
                }
            }
        }
        ini.set_i32(META_SECTION, "version", CONFIG_VERSION);
        true
    }

    /// Upgrade an old-format file in place, returning whether it happened.
    ///
    /// The method is "read -> drop the old section -> rewrite in the new
    /// format". No separate migration tool: the user never has to know, and
    /// opening the app once finishes it.
    ///
    /// The condition is "no `[meta] version` and `[Settings]` is non-empty".
    /// Checking `[Settings]` alone would misjudge the `[Items]` section of a
    /// new-format file: that section has always had this name and both
    /// formats share it.
    pub fn upgrade_legacy(ini: &mut Ini, dpi: i32) -> bool {
        if ini.get(META_SECTION, "version").is_some() {
            return false;
        }
        if ini.len_of(LEGACY_SECTION) == 0 {
            return false;
        }
        let mut cfg = Self::from_ini(ini);
        // The 1.x length keys are physical pixels too: convert to logical
        // pixels before rewriting in the new format, otherwise the icons
        // would jump by a factor of `dpi/96` after the upgrade to version 4.
        if dpi > 0 && dpi != 96 {
            for key in Key::all_keys() {
                if key.is_length() {
                    cfg.apply_i32(key, units::unscale(cfg.get_i32(key), dpi));
                }
            }
        }
        // Drop the old section before writing: keeping it would leave two
        // equivalent sets of keys in the file. The reader prefers the new
        // format, but a human would think the config is duplicated.
        ini.remove_section(LEGACY_SECTION);
        cfg.write_ini(ini);
        true
    }
}
