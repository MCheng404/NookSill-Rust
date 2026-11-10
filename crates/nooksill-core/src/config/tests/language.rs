use super::*;

#[test]
fn language_defaults_to_system() {
    assert!(Config::defaults().language.is_empty());
}

#[test]
fn language_roundtrips_through_general_section() {
    let mut c = Config::defaults();
    c.language = "zh-CN".to_owned();
    let mut ini = Ini::new();
    c.write_ini(&mut ini);
    assert_eq!(ini.get("general", "Language"), Some("zh-CN"));
    let back = Config::from_ini(&Ini::parse(&ini.to_text()));
    assert_eq!(back.language, "zh-CN");
}

#[test]
fn auto_is_written_as_an_empty_line() {
    let mut ini = Ini::new();
    Config::defaults().write_ini(&mut ini);
    assert_eq!(ini.get("general", "Language"), Some(""));
    assert_eq!(Config::from_ini(&Ini::parse(&ini.to_text())).language, "");
}

#[test]
fn tags_are_normalized_and_unknown_values_fall_back_to_auto() {
    assert_eq!(Config::sanitize_language(" zh-tw "), "zh-TW");
    assert_eq!(Config::sanitize_language("EN-us"), "en");
    assert_eq!(Config::sanitize_language("auto"), "");
    assert_eq!(Config::sanitize_language("fr-FR"), "");
    // A corrupted ini line must not reach the language lookup.
    let mut ini = Ini::new();
    ini.set("general", "Language", "not-a-language");
    assert_eq!(Config::from_ini(&ini).language, "");
}
