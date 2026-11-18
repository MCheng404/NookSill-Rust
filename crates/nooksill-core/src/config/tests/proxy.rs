//! Proxy mode and address round-trip through the ini.

use super::*;

#[test]
fn proxy_address_roundtrips_and_is_sanitized() {
    let mut cfg = Config::defaults();
    assert_eq!(cfg.get_i32(Key::NET_PROXY), 0);
    assert!(cfg.proxy.is_empty());

    cfg.apply_i32(Key::NET_PROXY, 1);
    cfg.proxy = Config::sanitize_proxy(" http://127.0.0.1:7890 \r\n");
    assert_eq!(cfg.proxy, "http://127.0.0.1:7890");

    let mut ini = Ini::default();
    cfg.write_ini(&mut ini);
    let back = Config::from_ini(&ini);
    assert_eq!(back.get_i32(Key::NET_PROXY), 1);
    assert_eq!(back.proxy, "http://127.0.0.1:7890");
}

#[test]
fn clearing_proxy_address_removes_the_ini_key() {
    let mut ini = Ini::default();
    ini.set("update", "Proxy", "http://127.0.0.1:7890");
    let cfg = Config::from_ini(&ini);
    assert_eq!(cfg.proxy, "http://127.0.0.1:7890");

    let mut ini2 = Ini::default();
    cfg.write_ini(&mut ini2);
    assert!(ini2.get("update", "Proxy").is_some());

    let cleared = Config {
        proxy: String::new(),
        ..cfg
    };
    let mut ini3 = Ini::default();
    cleared.write_ini(&mut ini3);
    assert!(ini3.get("update", "Proxy").is_none());
}
