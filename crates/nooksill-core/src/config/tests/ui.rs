use super::*;

/// The two icon accent switches are independent.
#[test]
fn icon_accent_switches_are_independent() {
    let mut c = Config::defaults();
    assert_eq!(c.get_i32(Key::UI_ACCENT_DOCK_ICONS), 1, "默认开");
    assert_eq!(c.get_i32(Key::UI_ACCENT_CC_ICONS), 1, "默认开");
    c.apply_i32(Key::UI_ACCENT_DOCK_ICONS, 0);
    assert_eq!(c.get_i32(Key::UI_ACCENT_CC_ICONS), 1);
    c.apply_i32(Key::UI_ACCENT_CC_ICONS, 0);
    assert_eq!(c.get_i32(Key::UI_ACCENT_DOCK_ICONS), 0);
}

#[test]
fn ui_theme_codes_are_inverted_relative_to_dock_theme() {
    // The two enums invert 0/1, so a mistake silently flips the theme.
    assert_eq!(DockTheme::from_code(0), DockTheme::Dark);
    assert_eq!(UiTheme::from_code(0), UiTheme::Light);
    let c = Config {
        theme: DockTheme::Dark,
        ui_theme: UiTheme::Light,
        ..Config::defaults()
    };
    assert!(c.dock_is_dark(false));
    assert!(!c.ui_is_dark(true));
}

/// `PanelDismiss` defaults on, lives in `[widgets]` and round-trips.
///
/// It is an own key, so it lives in `EXTRA`: `Key::ALL` must map item-by-item
/// onto the upstream keys, and mixing one in breaks that regression test.
#[test]
fn panel_dismiss_defaults_on_and_roundtrips() {
    let c = Config::defaults();
    assert_eq!(c.get_i32(Key::PANEL_DISMISS), 1);
    assert_eq!(Key::PANEL_DISMISS.section(), "widgets");
    assert!(Key::EXTRA.contains(&Key::PANEL_DISMISS));
    assert!(!Key::ALL.contains(&Key::PANEL_DISMISS));

    let mut ini = Ini::new();
    c.write_ini(&mut ini);
    assert_eq!(ini.get("widgets", "PanelDismiss"), Some("1"));
    ini.set("widgets", "PanelDismiss", "0");
    assert_eq!(Config::from_ini(&ini).get_i32(Key::PANEL_DISMISS), 0);
    // An out-of-range value clamps back to 0/1.
    ini.set("widgets", "PanelDismiss", "9");
    assert_eq!(Config::from_ini(&ini).get_i32(Key::PANEL_DISMISS), 1);
}

/// The experimental blur switch defaults off and lives in `[appearance]`.
#[test]
fn frost_illusion_defaults_off_and_roundtrips() {
    let c = Config::defaults();
    assert_eq!(c.get_i32(Key::FROST_ILLUSION), 0);
    assert_eq!(Key::FROST_ILLUSION.section(), "appearance");
    // It must be in EXTRA, or `from_ini` / `write_ini` skip it and the
    // switch is lost on restart.
    assert!(Key::EXTRA.contains(&Key::FROST_ILLUSION));
    assert!(!Key::ALL.contains(&Key::FROST_ILLUSION));

    let mut ini = Ini::new();
    c.write_ini(&mut ini);
    assert_eq!(ini.get("appearance", "FrostIllusion"), Some("0"));
    ini.set("appearance", "FrostIllusion", "1");
    assert_eq!(Config::from_ini(&ini).get_i32(Key::FROST_ILLUSION), 1);
    ini.set("appearance", "FrostIllusion", "7");
    assert_eq!(
        Config::from_ini(&ini).get_i32(Key::FROST_ILLUSION),
        1,
        "布尔键越界要夹紧"
    );
}

/// The updater switches keep their defaults, section and round-trip.
#[test]
fn updater_switches_default_and_roundtrip() {
    let c = Config::defaults();
    assert_eq!(c.get_i32(Key::UPD_CHANNEL), 0, "稳定版通道是默认");
    assert_eq!(c.get_i32(Key::UPD_PROMPT), 1, "更新弹窗默认开");
    assert_eq!(c.get_i32(Key::UPD_AUTO), 0, "自动更新默认关");
    assert_eq!(Key::UPD_CHANNEL.section(), "advanced");
    assert_eq!(Key::UPD_PROMPT.section(), "advanced");
    assert_eq!(Key::UPD_AUTO.section(), "advanced");
    for key in [Key::UPD_CHANNEL, Key::UPD_PROMPT, Key::UPD_AUTO] {
        assert!(Key::EXTRA.contains(&key));
        assert!(!Key::ALL.contains(&key));
    }

    let mut ini = Ini::new();
    c.write_ini(&mut ini);
    assert_eq!(ini.get("advanced", "UpdChannel"), Some("0"));
    assert_eq!(ini.get("advanced", "UpdPrompt"), Some("1"));
    assert_eq!(ini.get("advanced", "UpdAuto"), Some("0"));

    ini.set("advanced", "UpdChannel", "1");
    ini.set("advanced", "UpdPrompt", "0");
    ini.set("advanced", "UpdAuto", "1");
    let back = Config::from_ini(&ini);
    assert_eq!(back.get_i32(Key::UPD_CHANNEL), 1);
    assert_eq!(back.get_i32(Key::UPD_PROMPT), 0);
    assert_eq!(back.get_i32(Key::UPD_AUTO), 1);

    // Out-of-range values clamp into 0/1.
    ini.set("advanced", "UpdChannel", "9");
    ini.set("advanced", "UpdAuto", "-3");
    let clamped = Config::from_ini(&ini);
    assert_eq!(clamped.get_i32(Key::UPD_CHANNEL), 1, "通道越界夹紧到 1");
    assert_eq!(clamped.get_i32(Key::UPD_AUTO), 1, "布尔键非零即 1");
}
