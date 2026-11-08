use super::*;

/// A v3 ini stores physical pixels and v4 stores logical ones, so the
/// migration touches only the length keys.
#[test]
fn migrate_units_converts_only_the_length_keys() {
    let mut ini = Ini::parse("");
    let put = |ini: &mut Ini, key: Key, v: i32| ini.set_i32(key.section(), key.name(), v);
    put(&mut ini, Key::IconSize, 72);
    put(&mut ini, Key::HPAD, 18);
    put(&mut ini, Key::WIDGET_RADIUS, 18);
    put(&mut ini, Key::Opacity, 60);
    put(&mut ini, Key::REFL_SIZE, 38);
    ini.set_i32(META_SECTION, "version", 3);

    assert!(Config::migrate_units(&mut ini, 144), "v3 应当被迁移");
    let get = |ini: &Ini, key: Key| ini.get_i32(key.section(), key.name());
    assert_eq!(
        get(&ini, Key::IconSize),
        Some(48),
        "72 物理 / 150% = 48 逻辑"
    );
    assert_eq!(get(&ini, Key::HPAD), Some(12));
    assert_eq!(get(&ini, Key::WIDGET_RADIUS), Some(12));
    assert_eq!(get(&ini, Key::Opacity), Some(60), "百分比不换算");
    assert_eq!(get(&ini, Key::REFL_SIZE), Some(38), "比例不换算");
    assert_eq!(
        ini.get_i32(META_SECTION, "version"),
        Some(CONFIG_VERSION),
        "迁移后要写上新版本号"
    );
    // Idempotent: a second pass must do nothing, or every start shrinks
    // the icons by another step.
    assert!(!Config::migrate_units(&mut ini, 144));
    assert_eq!(get(&ini, Key::IconSize), Some(48));
}

/// The old format (single `[Settings]` plus CamelCase names) must still
/// load.
#[test]
fn legacy_settings_section_still_loads() {
    let mut ini = Ini::new();
    ini.set_i32(LEGACY_SECTION, "IconSize", 64);
    ini.set_i32(LEGACY_SECTION, "Mode", 2);
    ini.set_i32(LEGACY_SECTION, "BgMode", 0);
    let c = Config::from_ini(&ini);
    assert_eq!(c.icon_size, 64);
    assert_eq!(c.mode, DockMode::SmartHide);
    assert_eq!(c.bg_mode, BgMode::Solid);
}

/// The upgrade happens on read: load an old file, save, and the file on
/// disk is already in the new format.
#[test]
fn saving_after_legacy_load_produces_new_format() {
    let mut legacy = Ini::new();
    legacy.set_i32(LEGACY_SECTION, "IconSize", 64);
    let c = Config::from_ini(&legacy);

    let mut out = Ini::new();
    c.write_ini(&mut out);
    assert_eq!(out.get("appearance", "icon_size"), Some("64"));
    let version = CONFIG_VERSION.to_string();
    assert_eq!(out.get(META_SECTION, "version"), Some(version.as_str()));
}

/// The upgrade turns the old format into the new one in place and keeps
/// every value.
///
/// This is the only defence against "settings scrambled after the upgrade";
/// every key is compared and any one that reverted to its default fails.
#[test]
fn upgrade_legacy_keeps_every_value() {
    let dpi = 144;
    let mut want = Config::defaults();
    for key in Key::ALL.iter().copied() {
        let (lo, hi) = key.range();
        let v = if want.get_i32(key) == lo { hi } else { lo };
        want.apply_i32(key, v);
    }
    want.clamp();

    // Build an old-format file.
    let mut ini = Ini::new();
    for key in Key::ALL.iter().copied() {
        ini.set_i32(LEGACY_SECTION, key.legacy_name(), want.get_i32(key));
    }
    ini.set("Items", "Count", "1");
    ini.set("Items", "I0", "0|0|记事本|C:/Windows/notepad.exe||0|");

    assert!(Config::upgrade_legacy(&mut ini, dpi), "应当报告发生了升级");

    // The old section is gone, the new ones are in place, the version is
    // written.
    assert_eq!(ini.len_of(LEGACY_SECTION), 0, "旧段没删干净");
    let version = CONFIG_VERSION.to_string();
    assert_eq!(ini.get(META_SECTION, "version"), Some(version.as_str()));
    // The items section must survive: the upgrade only touches the config.
    assert_eq!(ini.get("Items", "Count"), Some("1"));

    let after = Config::from_ini(&ini);
    for key in Key::ALL.iter().copied() {
        assert_eq!(
            after.get_i32(key),
            want.get_i32(key),
            "升级后键 {} 的值变了",
            key.name()
        );
    }

    // A second run must not act again.
    assert!(!Config::upgrade_legacy(&mut ini, dpi), "已经是新格式了");
}

/// The 23 second-batch keys must survive the upgrade.
///
/// `upgrade_legacy` reads, drops the whole `[Settings]` section and rewrites
/// it, so any key missing from `Key::ALL` is dropped with it.
#[test]
fn upgrade_legacy_keeps_upstream_v160_keys() {
    let mut ini = Ini::new();
    // A whole section in the old format.
    ini.set_i32(LEGACY_SECTION, "IconSize", 64);
    ini.set_i32(LEGACY_SECTION, "MnAdd", 1);
    ini.set_i32(LEGACY_SECTION, "Launchpad", 1);
    ini.set_i32(LEGACY_SECTION, "WSide", 1);
    ini.set_i32(LEGACY_SECTION, "WOrder", 1);
    ini.set_i32(LEGACY_SECTION, "NotifyW", 1);
    ini.set_i32(LEGACY_SECTION, "FanBgL", 0x00FF_0000);
    ini.set_i32(LEGACY_SECTION, "FanBgD", 0x0012_3456);
    ini.set_i32(LEGACY_SECTION, "FanFgL", 0x00AB_CDEF);
    ini.set_i32(LEGACY_SECTION, "FanFgD", 0x0000_00FF);
    ini.set_i32(LEGACY_SECTION, "PinMenu", 1);
    ini.set_i32(LEGACY_SECTION, "MultiMode", 1);
    ini.set_i32(LEGACY_SECTION, "UpdChk", 0);
    ini.set_i32(LEGACY_SECTION, "UpdLast", 12345);
    ini.set_i32(LEGACY_SECTION, "LpCols", 9);
    ini.set_i32(LEGACY_SECTION, "LpRows", 6);
    ini.set_i32(LEGACY_SECTION, "FanDrag", 0);
    ini.set_i32(LEGACY_SECTION, "TrayHide", 1);
    ini.set_i32(LEGACY_SECTION, "CtrlCenter", 0);
    ini.set_i32(LEGACY_SECTION, "CcEnergy", 0);
    ini.set_i32(LEGACY_SECTION, "CcHotspot", 0);
    ini.set_i32(LEGACY_SECTION, "ResMon", 0);
    ini.set_i32(LEGACY_SECTION, "ToolW", 0);
    ini.set_i32(LEGACY_SECTION, "WideMask", 0b101010);
    ini.set_i32(LEGACY_SECTION, "ModeW", 0);

    assert!(Config::upgrade_legacy(&mut ini, 96), "应当报告发生了升级");

    let after = Config::from_ini(&ini);
    assert_eq!(after.get_i32(Key::LAUNCHPAD), 1);
    assert_eq!(after.get_i32(Key::W_SIDE), 1);
    assert_eq!(after.get_i32(Key::W_ORDER), 1);
    assert_eq!(after.get_i32(Key::NOTIFY_W), 1);
    assert_eq!(after.get_i32(Key::FAN_BG_L), 0x00FF_0000);
    assert_eq!(after.get_i32(Key::FAN_BG_D), 0x0012_3456);
    assert_eq!(after.get_i32(Key::FAN_FG_L), 0x00AB_CDEF);
    assert_eq!(after.get_i32(Key::FAN_FG_D), 0x0000_00FF);
    assert_eq!(after.get_i32(Key::PIN_MENU), 1);
    assert_eq!(after.get_i32(Key::MULTI_MODE), 1);
    assert_eq!(after.get_i32(Key::UPD_CHK), 0);
    assert_eq!(after.get_i32(Key::UPD_LAST), 12345);
    assert_eq!(after.get_i32(Key::LP_COLS), 9);
    assert_eq!(after.get_i32(Key::LP_ROWS), 6);
    assert_eq!(after.get_i32(Key::FAN_DRAG), 0);
    assert_eq!(after.get_i32(Key::TRAY_HIDE), 1);
    assert_eq!(after.get_i32(Key::CTRL_CENTER), 0);
    assert_eq!(after.get_i32(Key::CC_ENERGY), 0);
    assert_eq!(after.get_i32(Key::CC_HOTSPOT), 0);
    assert_eq!(after.get_i32(Key::RES_MON), 0);
    assert_eq!(after.get_i32(Key::TOOL_W), 0);
    assert_eq!(after.get_i32(Key::WIDE_MASK), 0b101010);
    assert_eq!(after.get_i32(Key::MODE_W), 0);
}

/// The 19 third-batch keys must survive the upgrade as well.
///
/// The drop-the-section rewrite is unchanged, so a key missing from the
/// table is dropped with it.
#[test]
fn upgrade_legacy_keeps_upstream_v165_keys() {
    let mut ini = Ini::new();
    ini.set_i32(LEGACY_SECTION, "IconSize", 64);
    ini.set_i32(LEGACY_SECTION, "HPad", 20);
    ini.set_i32(LEGACY_SECTION, "UiFrost", 0);
    ini.set_i32(LEGACY_SECTION, "LpFrost", 0);
    ini.set_i32(LEGACY_SECTION, "TrayW", 0);
    ini.set_i32(LEGACY_SECTION, "WMerge", 0);
    ini.set_i32(LEGACY_SECTION, "StatusBar", 1);
    ini.set_i32(LEGACY_SECTION, "SbFrost", 0);
    ini.set_i32(LEGACY_SECTION, "SbOpacity", 72);
    ini.set_i32(LEGACY_SECTION, "SbColor", 0x0012_3456);
    ini.set_i32(LEGACY_SECTION, "SbRadius", 18);
    ini.set_i32(LEGACY_SECTION, "SbHeight", 40);
    ini.set_i32(LEGACY_SECTION, "SbFollowSys", 0);
    ini.set_i32(LEGACY_SECTION, "SbTopGap", 8);
    ini.set_i32(LEGACY_SECTION, "SbMaxAvoid", 1);
    ini.set_i32(LEGACY_SECTION, "SbFloat", 1);
    ini.set_i32(LEGACY_SECTION, "SbSideGap", 24);
    ini.set_i32(LEGACY_SECTION, "SbAutoHide", 1);
    ini.set_i32(LEGACY_SECTION, "SepScale", 1);
    ini.set_i32(LEGACY_SECTION, "MenuFrost", 1);

    assert!(Config::upgrade_legacy(&mut ini, 96), "应当报告发生了升级");

    let after = Config::from_ini(&ini);
    assert_eq!(after.get_i32(Key::HPAD), 20);
    assert_eq!(after.get_i32(Key::UI_FROST), 0);
    assert_eq!(after.get_i32(Key::LP_FROST), 0);
    assert_eq!(after.get_i32(Key::TRAY_W), 0);
    assert_eq!(after.get_i32(Key::W_MERGE), 0);
    assert_eq!(after.get_i32(Key::STATUS_BAR), 1);
    assert_eq!(after.get_i32(Key::SB_FROST), 0);
    assert_eq!(after.get_i32(Key::SB_OPACITY), 72);
    assert_eq!(after.get_i32(Key::SB_COLOR), 0x0012_3456);
    assert_eq!(after.get_i32(Key::SB_RADIUS), 18);
    assert_eq!(after.get_i32(Key::SB_HEIGHT), 40);
    assert_eq!(after.get_i32(Key::SB_FOLLOW_SYS), 0);
    assert_eq!(after.get_i32(Key::SB_TOP_GAP), 8);
    assert_eq!(after.get_i32(Key::SB_MAX_AVOID), 1);
    assert_eq!(after.get_i32(Key::SB_FLOAT), 1);
    assert_eq!(after.get_i32(Key::SB_SIDE_GAP), 24);
    assert_eq!(after.get_i32(Key::SB_AUTO_HIDE), 1);
    assert_eq!(after.get_i32(Key::SEP_SCALE), 1);
    assert_eq!(after.get_i32(Key::MENU_FROST), 1);
    // The older keys must still be there.
    assert_eq!(after.get_i32(Key::IconSize), 64);
}

/// A file already in the new format must not be upgraded.
#[test]
fn upgrade_legacy_ignores_new_format() {
    let mut ini = Ini::new();
    Config::defaults().write_ini(&mut ini);
    ini.set("Items", "Count", "1");
    assert!(!Config::upgrade_legacy(&mut ini, 96));
}

#[test]
fn legacy_frost_key_migrates() {
    let mut ini = Ini::new();
    ini.set(LEGACY_SECTION, "Frost", "1");
    assert_eq!(Config::from_ini(&ini).bg_mode, BgMode::Frost);

    let mut ini = Ini::new();
    ini.set(LEGACY_SECTION, "Frost", "0");
    assert_eq!(Config::from_ini(&ini).bg_mode, BgMode::Solid);

    // The new key wins when both exist.
    let mut ini = Ini::new();
    ini.set(LEGACY_SECTION, "Frost", "1");
    ini.set("Settings", "BgMode", "0");
    assert_eq!(Config::from_ini(&ini).bg_mode, BgMode::Solid);
}
