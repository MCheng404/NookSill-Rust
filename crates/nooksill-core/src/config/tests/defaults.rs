use super::*;

#[test]
fn defaults_match_c_version() {
    let c = Config::defaults();
    assert_eq!(c.icon_size, 48);
    assert_eq!(c.zoom_size, 84);
    assert_eq!(c.spacing, 6);
    assert_eq!(c.margin, 8);
    assert_eq!(c.pad, 8);
    assert_eq!(c.radius, 18);
    assert_eq!(c.opacity, 60);
    assert_eq!(c.blur, 6);
    assert_eq!(c.bg_mode, BgMode::Frost);
    assert_eq!(c.plate_radius, 22);
    assert_eq!(c.bg_color1, -1);
    assert_eq!(c.ui_theme, UiTheme::System);
    assert!(c.zoom && c.dots && c.labels && c.bounce && c.web_app && c.border);
    assert!(!c.reserve && !c.lock && !c.clock && !c.reflect && !c.notes);
}

/// Defaults are 96 DPI logical pixels, independent of the DPI.
///
/// The conversion to physical pixels happens only at render time, so the
/// same ini looks the same at 100% and 150%.
#[test]
fn defaults_are_logical_pixels() {
    let c = Config::defaults();
    assert_eq!(c.icon_size, 48);
    assert_eq!(c.radius, 18);
    assert_eq!(c.get_i32(Key::HPAD), 12);
    assert_eq!(c.get_i32(Key::SEP_GAP), 4);
    // Non-geometry values must not change.
    assert_eq!(c.opacity, 60);
    assert_eq!(c.plate_radius, 22);

    // Physical pixels appear at render time: 48 logical x 150% = 72.
    assert_eq!(crate::units::scale(c.icon_size, 144), 72);
    assert_eq!(crate::units::scale(c.get_i32(Key::HPAD), 144), 18);
}

/// Defaults of the numeric keys.
#[test]
fn numeric_defaults_match_upstream() {
    let c = Config::defaults();
    assert_eq!(c.get_i32(Key::PLATE_MODE), 0);
    assert_eq!(c.get_i32(Key::PLATE_THEME), 2);
    assert_eq!(c.get_i32(Key::PLATE_OPACITY), 100);
    assert_eq!(c.get_i32(Key::MENU_THEME), 2);
    assert_eq!(c.get_i32(Key::BORDER_WIDTH), 1);
    assert_eq!(c.get_i32(Key::GRAD_ANGLE), 90);
    assert_eq!(c.get_i32(Key::REFL_SIZE), 38);
    assert_eq!(c.get_i32(Key::REFL_GAP), 3);
    assert_eq!(c.get_i32(Key::REFL_OPACITY), 30);
    assert_eq!(c.get_i32(Key::FAN_ARC), 40);
    assert_eq!(c.get_i32(Key::FAN_GAP), 9);
    assert_eq!(c.get_i32(Key::GRP_OPACITY), 94);
    assert_eq!(c.get_i32(Key::CLOCK_WEEK), 1);
    assert_eq!(c.get_i32(Key::CLOCK_BOLD_T), 1);
    assert_eq!(c.get_i32(Key::MN_AUTO), 1);
    assert_eq!(c.get_i32(Key::MN_NOTES), 1);
    assert_eq!(c.get_i32(Key::MN_ADD), 1);
    // Second batch.
    assert_eq!(c.get_i32(Key::LAUNCHPAD), 0);
    assert_eq!(c.get_i32(Key::W_SIDE), 0);
    assert_eq!(c.get_i32(Key::W_ORDER), 0);
    assert_eq!(c.get_i32(Key::NOTIFY_W), 0);
    assert_eq!(c.get_i32(Key::FAN_BG_L), -1);
    assert_eq!(c.get_i32(Key::FAN_FG_D), -1);
    assert_eq!(c.get_i32(Key::PIN_MENU), 0);
    assert_eq!(c.get_i32(Key::MULTI_MODE), 0);
    assert_eq!(c.get_i32(Key::UPD_CHK), 1);
    assert_eq!(c.get_i32(Key::UPD_LAST), 0);
    assert_eq!(c.get_i32(Key::LP_COLS), 7);
    assert_eq!(c.get_i32(Key::LP_ROWS), 4);
    assert_eq!(c.get_i32(Key::FAN_DRAG), 1);
    assert_eq!(c.get_i32(Key::TRAY_HIDE), 0);
    assert_eq!(c.get_i32(Key::CTRL_CENTER), 1);
    assert_eq!(c.get_i32(Key::CC_ENERGY), 1);
    assert_eq!(c.get_i32(Key::CC_HOTSPOT), 1);
    assert_eq!(c.get_i32(Key::RES_MON), 1);
    assert_eq!(c.get_i32(Key::TOOL_W), 1);
    assert_eq!(c.get_i32(Key::WIDE_MASK), 0);
    assert_eq!(c.get_i32(Key::MODE_W), 1);
    // Third batch. `SepScale` and `MenuFrost` are not assigned in the
    // default config, so zero-initialization means off: 0, not 1.
    assert_eq!(c.get_i32(Key::HPAD), 12);
    assert_eq!(c.get_i32(Key::UI_FROST), 1);
    assert_eq!(c.get_i32(Key::LP_FROST), 1);
    assert_eq!(c.get_i32(Key::TRAY_W), 1);
    assert_eq!(c.get_i32(Key::W_MERGE), 1);
    assert_eq!(c.get_i32(Key::STATUS_BAR), 0);
    assert_eq!(c.get_i32(Key::SB_FROST), 1);
    assert_eq!(c.get_i32(Key::SB_OPACITY), 88);
    assert_eq!(c.get_i32(Key::SB_COLOR), -1);
    assert_eq!(c.get_i32(Key::SB_RADIUS), 10);
    assert_eq!(c.get_i32(Key::SB_HEIGHT), 32);
    assert_eq!(c.get_i32(Key::SB_FOLLOW_SYS), 1);
    assert_eq!(c.get_i32(Key::SB_TOP_GAP), 0);
    assert_eq!(c.get_i32(Key::SB_MAX_AVOID), 0);
    assert_eq!(c.get_i32(Key::SB_FLOAT), 0);
    assert_eq!(c.get_i32(Key::SB_SIDE_GAP), 12);
    assert_eq!(c.get_i32(Key::SB_AUTO_HIDE), 0);
    assert_eq!(c.get_i32(Key::SEP_SCALE), 0);
    assert_eq!(c.get_i32(Key::MENU_FROST), 0);
    // HPad and SepGap store logical pixels, so the default ignores the DPI.
    assert_eq!(Config::defaults().get_i32(Key::HPAD), 12);
    assert_eq!(c.get_i32(Key::SEP_GAP), 4);
    assert_eq!(Config::defaults().get_i32(Key::SEP_GAP), 4);
}
