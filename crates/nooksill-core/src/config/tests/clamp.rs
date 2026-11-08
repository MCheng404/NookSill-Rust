use super::*;

#[test]
fn hostile_values_are_clamped() {
    // These feed geometry and alpha directly, so out-of-range values must
    // be blocked.
    let mut c = Config::defaults();
    for (key, evil) in [
        (Key::Opacity, 1000),
        (Key::Opacity, -5),
        (Key::IconSize, 999_999),
        (Key::IconSize, i32::MIN),
        (Key::ZoomSize, 1),
        (Key::Blur, 0),
        (Key::Blur, 999),
        (Key::Spacing, -1),
        (Key::Margin, -1),
        (Key::Padding, -1),
        (Key::Radius, -1),
        (Key::PlateRadius, 500),
        (Key::Theme, 99),
        (Key::Theme, -3),
        (Key::UiTheme, i32::MAX),
        (Key::BgMode, 7),
        (Key::Edge, 4),
        (Key::DotStyle, -1),
        (Key::Monitor, -1),
        // Table-driven keys are clamped too: an unclamped int goes straight
        // into the drawing code.
        (Key::PLATE_OPACITY, 999),
        (Key::SEP_GAP, -5),
        (Key::GRAD_ANGLE, i32::MAX),
        (Key::BROWSER, 77),
    ] {
        c.apply_i32(key, evil);
        let got = c.get_i32(key);
        let (lo, hi) = key.range();
        assert!(
            (lo..=hi).contains(&got),
            "{} = {evil} -> {got} 越界 [{lo}, {hi}]",
            key.name()
        );
    }
    // The last Opacity round is -5, so it clamps to zero; the upper bound is
    // checked separately.
    assert_eq!(c.opacity, 0);
    c.apply_i32(Key::Opacity, 250);
    assert_eq!(c.opacity, 100);
    // The last Blur round is 999 -> clamped to the upper bound 32.
    assert_eq!(c.blur, 32);
    c.apply_i32(Key::Blur, 0);
    assert_eq!(c.blur, 1);
    assert_eq!(c.theme, DockTheme::Dark);
    assert_eq!(c.edge, Edge::Bottom);
    // The three base-plate keys.
    assert_eq!(c.get_i32(Key::PLATE_MODE), 0);
    assert_eq!(c.get_i32(Key::PLATE_THEME), 2);
    assert_eq!(c.get_i32(Key::PLATE_OPACITY), 100);
}

#[test]
fn missing_and_garbage_values_fall_back() {
    let mut ini = Ini::new();
    ini.set("Settings", "Opacity", "abc");
    ini.set("Settings", "IconSize", "");
    let c = Config::from_ini(&ini);
    assert_eq!(c.opacity, 0); // 与 GetPrivateProfileInt 一致：非数字 -> 0
    assert_eq!(c.icon_size, 16); // 0 被夹到下限
}
