use super::*;

/// The new format is written into semantic sections and carries a version.
#[test]
fn write_ini_uses_semantic_sections() {
    let mut c = Config::defaults();
    c.icon_size = 64;
    c.mode = DockMode::AutoHide;
    let mut ini = Ini::new();
    c.write_ini(&mut ini);

    assert_eq!(
        ini.get(META_SECTION, "version"),
        Some(CONFIG_VERSION.to_string().as_str())
    );
    assert_eq!(ini.get("appearance", "icon_size"), Some("64"));
    assert_eq!(ini.get("layout", "hide_mode"), Some("1"));
    // Nothing may be written to the old [Settings] section any more.
    assert!(ini.get(LEGACY_SECTION, "IconSize").is_none());
}

/// The core migration guarantee: the same config read from the old format
/// and from the new one is equivalent.
///
/// Asserting only that it loads is not enough: a key-name mapping off by
/// one or a wrong section shows up as a setting silently reverting to its
/// default after the upgrade, so every key is compared.
#[test]
fn legacy_and_new_formats_agree_on_every_key() {
    // Set every key to a non-default value first, so a value that happens
    // to equal the default cannot hide a mapping error.
    let mut want = Config::defaults();
    for key in Key::ALL.iter().copied() {
        let (lo, hi) = key.range();
        let cur = want.get_i32(key);
        // Pick a legal value different from the current one.
        let v = if cur == lo { hi } else { lo };
        want.apply_i32(key, v);
    }
    want.clamp();

    // Write one file in the old format.
    let mut legacy = Ini::new();
    for key in Key::ALL.iter().copied() {
        legacy.set_i32(LEGACY_SECTION, key.legacy_name(), want.get_i32(key));
    }
    let from_legacy = Config::from_ini(&legacy);

    // Write one file in the new format and read it back.
    let mut fresh = Ini::new();
    want.write_ini(&mut fresh);
    let from_new = Config::from_ini(&fresh);

    for key in Key::ALL.iter().copied() {
        assert_eq!(
            from_legacy.get_i32(key),
            from_new.get_i32(key),
            "键 {} 在新旧格式下取值不同",
            key.name()
        );
    }
}

#[test]
fn ini_roundtrip_preserves_every_key() {
    let mut c = Config::defaults();
    c.apply_i32(Key::IconSize, 64);
    c.apply_i32(Key::Edge, 2);
    c.apply_i32(Key::BgColor1, 0x0011_2233);
    c.apply_i32(Key::Clock, 1);

    let mut ini = Ini::new();
    c.write_ini(&mut ini);
    let back = Config::from_ini(&ini);
    assert_eq!(back, c);
}

/// Every key must survive a write to ini and a read back.
///
/// With the table-driven key list, this is the only guard that every row is
/// wired up.
#[test]
fn every_key_roundtrips_through_ini() {
    let mut c = Config::defaults();
    // Give every key a legal value: 1 for enums and bools, the range
    // midpoint for numeric keys.
    for key in Key::ALL.iter().copied() {
        let (lo, hi) = key.range();
        // Bools and enums use 1, numeric keys the range midpoint.
        let v = if key.is_bool() || key.is_enum() {
            1
        } else if lo < 0 {
            0
        } else {
            lo + (hi - lo) / 2
        };
        c.apply_i32(key, v);
    }
    let mut ini = Ini::new();
    c.write_ini(&mut ini);
    let back = Config::from_ini(&ini);
    for key in Key::ALL.iter().copied() {
        assert_eq!(
            back.get_i32(key),
            c.get_i32(key),
            "{} 往返后变了",
            key.name()
        );
    }
}
