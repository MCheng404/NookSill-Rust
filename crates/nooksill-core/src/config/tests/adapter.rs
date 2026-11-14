use super::*;

#[test]
fn adapter_defaults_to_auto() {
    // An empty string means automatic, and it must be the default.
    assert!(Config::defaults().adapter.is_empty());
}

#[test]
fn adapter_roundtrips_through_graphics_section() {
    let mut c = Config::defaults();
    c.adapter = "NVIDIA GeForce RTX 5070 Laptop GPU".to_owned();
    let mut ini = Ini::new();
    c.write_ini(&mut ini);

    // It must land in its own section, not pollute the row-ordered
    // [Settings].
    assert_eq!(
        ini.get("Graphics", "Adapter"),
        Some("NVIDIA GeForce RTX 5070 Laptop GPU")
    );
    assert_eq!(ini.get("Settings", "Adapter"), None);

    let back = Config::from_ini(&Ini::parse(&ini.to_text()));
    assert_eq!(back.adapter, c.adapter);
}

/// The tile order is written only after a drag.
///
/// An empty `[widgets] Order=` line would suggest the user reordered the
/// tiles when the value is really the default.
#[test]
fn cc_order_only_written_when_set() {
    let c = Config::defaults();
    assert!(c.cc_order.is_empty());
    let mut ini = Ini::default();
    c.write_ini(&mut ini);
    assert!(
        ini.get("widgets", "Order").is_none(),
        "没拖过就不该有这一行"
    );

    let mut c2 = Config::defaults();
    c2.cc_order = "5,0,1,2,3,4,6,7".to_owned();
    let mut ini2 = Ini::default();
    c2.write_ini(&mut ini2);
    assert_eq!(
        Config::from_ini(&ini2).cc_order,
        "5,0,1,2,3,4,6,7",
        "拖过之后要原样读回来"
    );

    // Resetting to defaults must erase the saved order, not leave it behind:
    // otherwise the next start restores the very order the user reset.
    let c3 = Config::defaults();
    let mut ini3 = ini2;
    c3.write_ini(&mut ini3);
    assert!(
        ini3.get("widgets", "Order").is_none(),
        "恢复默认后旧的 Order 行要删掉"
    );
    assert!(Config::from_ini(&ini3).cc_order.is_empty());
}

#[test]
fn adapter_auto_roundtrips_as_empty_string() {
    let c = Config::defaults();
    let mut ini = Ini::new();
    c.write_ini(&mut ini);
    assert_eq!(ini.get("Graphics", "Adapter"), Some(""));
    assert!(Config::from_ini(&ini).adapter.is_empty());
}

#[test]
fn adapter_sanitize_trims_strips_controls_and_caps_length() {
    assert_eq!(
        Config::sanitize_adapter("  AMD Radeon 610M  "),
        "AMD Radeon 610M"
    );
    // A newline would break the ini value in the multi-line format.
    assert_eq!(Config::sanitize_adapter("Intel\r\nUHD\t620"), "IntelUHD620");
    let long = "x".repeat(500);
    assert_eq!(Config::sanitize_adapter(&long).chars().count(), 128);
    assert_eq!(Config::sanitize_adapter("   "), "");
}

#[test]
fn adapter_from_ini_is_sanitized() {
    let mut ini = Ini::new();
    ini.set("Graphics", "Adapter", "  NVIDIA RTX 5070\r\n");
    assert_eq!(Config::from_ini(&ini).adapter, "NVIDIA RTX 5070");
}
