use super::*;

/// Every table-driven key must appear in the read and write lists.
///
/// A missing key is not a compile error: the slider moves, the session
/// applies it, and the value resets on the next start.
#[test]
fn every_numeric_key_is_persisted() {
    let live: std::collections::HashSet<u16> = Key::all_keys()
        .filter_map(|k| match k {
            Key::Numeric(i) => Some(i),
            _ => None,
        })
        .collect();
    let missing: Vec<&str> = NUMERIC_KEYS
        .iter()
        .enumerate()
        .filter(|(i, _)| !live.contains(&(*i as u16)))
        .map(|(_, row)| row.0)
        .collect();
    assert!(missing.is_empty(), "这些键不会被写盘：{missing:?}");
}

/// The length check must cover the group that stores logical pixels and
/// must not include percentages.
#[test]
fn length_keys_are_exactly_the_pixel_sized_ones() {
    for key in [
        Key::IconSize,
        Key::ZoomSize,
        Key::Spacing,
        Key::Padding,
        Key::Margin,
        Key::Radius,
        Key::HPAD,
        Key::SEP_GAP,
        Key::SEP_WIDTH,
        Key::REFL_GAP,
        Key::WIDGET_RADIUS,
        Key::WIDGET_GAP,
    ] {
        assert!(key.is_length(), "{key:?} 应当算长度");
    }
    for key in [
        Key::Opacity,
        Key::REFL_SIZE,
        Key::WIDGET_OPACITY,
        Key::PlateRadius,
        Key::BORDER_WIDTH,
        Key::GRAD_ANGLE,
        Key::FAN_ARC,
        Key::HIDE_DELAY,
    ] {
        assert!(!key.is_length(), "{key:?} 不是长度，缩放它会把语义弄错");
    }
}

/// The named constants must map one-to-one onto the table rows.
///
/// The constants hard-code their index, so a reordered table breaks them
/// all; this test is the only guard.
#[test]
fn numeric_constants_match_table_rows() {
    // A slice rather than a fixed-size array: a hard-coded length would have
    // to change with every new key, which is exactly the manual sync this
    // test guards against.
    let pairs: &[(Key, &str)] = &[
        (Key::PLATE_MODE, "PlateMode"),
        (Key::PLATE_THEME, "PlateTheme"),
        (Key::PLATE_OPACITY, "PlateOpacity"),
        (Key::BG_COLOR3, "BgColor3"),
        (Key::MENU_THEME, "MenuTheme"),
        (Key::BORDER_COLOR, "BorderColor"),
        (Key::BORDER_WIDTH, "BorderWidth"),
        (Key::SEP_COLOR, "SepColor"),
        (Key::DOT_COLOR, "DotColor"),
        (Key::GRAD_ANGLE, "GradAngle"),
        (Key::GRAD_A1, "GradA1"),
        (Key::GRAD_A2, "GradA2"),
        (Key::GRAD_A3, "GradA3"),
        (Key::CLOCK_SHOW, "ClockShow"),
        (Key::CLOCK_COL_LIGHT, "ClockColLight"),
        (Key::CLOCK_COL_DARK, "ClockColDark"),
        (Key::REFL_SIZE, "ReflSize"),
        (Key::REFL_GAP, "ReflGap"),
        (Key::REFL_OPACITY, "ReflOpacity"),
        (Key::FAN_ARC, "FanArc"),
        (Key::FAN_GAP, "FanGap"),
        (Key::FAN_CUT, "FanCut"),
        (Key::GROUP_FROST, "GroupFrost"),
        (Key::GROUP_POS, "GroupPos"),
        (Key::GUARDIAN, "Guardian"),
        (Key::CLICK_TOGGLE, "ClickToggle"),
        (Key::SEP_GAP, "SepGap"),
        (Key::SEP_WIDTH, "SepWidth"),
        (Key::CLOCK_SEC, "ClockSec"),
        (Key::CLOCK_WEEK, "ClockWeek"),
        (Key::DATE_FMT, "DateFmt"),
        (Key::CLOCK_ORDER, "ClockOrder"),
        (Key::CLOCK_BOLD_T, "ClockBoldT"),
        (Key::CLOCK_BOLD_D, "ClockBoldD"),
        (Key::BROWSER, "Browser"),
        (Key::GRP_OPACITY, "GrpOpacity"),
        (Key::GRP_BG, "GrpBg"),
        (Key::MIN_STYLE, "MinStyle"),
        (Key::MN_AUTO, "MnAuto"),
        (Key::MN_NOTES, "MnNotes"),
        (Key::MN_ADD, "MnAdd"),
        (Key::LAUNCHPAD, "Launchpad"),
        (Key::W_SIDE, "WSide"),
        (Key::W_ORDER, "WOrder"),
        (Key::NOTIFY_W, "NotifyW"),
        (Key::FAN_BG_L, "FanBgL"),
        (Key::FAN_BG_D, "FanBgD"),
        (Key::FAN_FG_L, "FanFgL"),
        (Key::FAN_FG_D, "FanFgD"),
        (Key::PIN_MENU, "PinMenu"),
        (Key::MULTI_MODE, "MultiMode"),
        (Key::UPD_CHK, "UpdChk"),
        (Key::UPD_LAST, "UpdLast"),
        (Key::LP_COLS, "LpCols"),
        (Key::LP_ROWS, "LpRows"),
        (Key::FAN_DRAG, "FanDrag"),
        (Key::TRAY_HIDE, "TrayHide"),
        (Key::CTRL_CENTER, "CtrlCenter"),
        (Key::CC_ENERGY, "CcEnergy"),
        (Key::CC_HOTSPOT, "CcHotspot"),
        (Key::RES_MON, "ResMon"),
        (Key::TOOL_W, "ToolW"),
        (Key::WIDE_MASK, "WideMask"),
        (Key::MODE_W, "ModeW"),
        // The third batch; this table is indexed by constant, so the order
        // does not matter but no row may be missing.
        (Key::HPAD, "HPad"),
        (Key::UI_FROST, "UiFrost"),
        (Key::LP_FROST, "LpFrost"),
        (Key::TRAY_W, "TrayW"),
        (Key::W_MERGE, "WMerge"),
        (Key::STATUS_BAR, "StatusBar"),
        (Key::SB_FROST, "SbFrost"),
        (Key::SB_OPACITY, "SbOpacity"),
        (Key::SB_COLOR, "SbColor"),
        (Key::SB_RADIUS, "SbRadius"),
        (Key::SB_HEIGHT, "SbHeight"),
        (Key::SB_FOLLOW_SYS, "SbFollowSys"),
        (Key::SB_TOP_GAP, "SbTopGap"),
        (Key::SB_MAX_AVOID, "SbMaxAvoid"),
        (Key::SB_FLOAT, "SbFloat"),
        (Key::SB_SIDE_GAP, "SbSideGap"),
        (Key::SB_AUTO_HIDE, "SbAutoHide"),
        (Key::SEP_SCALE, "SepScale"),
        (Key::MENU_FROST, "MenuFrost"),
        // The [effect.smoke] section.
        (Key::SMOKE, "Smoke"),
        (Key::SMOKE_COLOR, "SmokeColor"),
        (Key::SMOKE_COLOR2, "SmokeColor2"),
        (Key::SMOKE_OPACITY, "SmokeOpacity"),
        (Key::SMOKE_DENSITY, "SmokeDensity"),
        (Key::SMOKE_SPEED, "SmokeSpeed"),
        (Key::SMOKE_TONE, "SmokeTone"),
        (Key::SMOKE_THICK, "SmokeThick"),
        (Key::SMOKE_MOUSE, "SmokeMouse"),
        (Key::SMOKE_REACH, "SmokeReach"),
        (Key::SMOKE_CLOUDS, "SmokeClouds"),
        (Key::PANEL_DISMISS, "PanelDismiss"),
        (Key::UI_SCALE, "UiScale"),
        (Key::UI_ACCENT, "UiAccent"),
        (Key::UI_ACCENT_DOCK, "UiAccentDock"),
        (Key::FRAME_CAP, "FrameCap"),
        (Key::VSYNC, "VSync"),
        (Key::MSAA, "Msaa"),
        (Key::UI_ACCENT_DOCK_ICONS, "UiAccentDockIcons"),
        (Key::UI_ACCENT_CC_ICONS, "UiAccentCcIcons"),
        (Key::NET_SECURE_DNS, "NetSecureDns"),
        (Key::NET_MIRROR_ALL, "NetMirrorAll"),
        (Key::HIDE_DELAY, "HideDelay"),
        (Key::SHOW_DELAY, "ShowDelay"),
        (Key::HIDE_DELAY_MAX, "HideDelayMax"),
        (Key::SHOW_DELAY_MAX, "ShowDelayMax"),
        (Key::WIDGET_OPACITY, "WidgetOpacity"),
        (Key::WIDGET_RADIUS, "WidgetRadius"),
        (Key::WIDGET_GAP, "WidgetGap"),
        (Key::WIDGET_BLUR, "WidgetBlur"),
        (Key::PANEL_OPACITY, "PanelOpacity"),
        (Key::PANEL_OVERLAY, "PanelOverlay"),
        (Key::FROST_ILLUSION, "FrostIllusion"),
        (Key::UPD_CHANNEL, "UpdChannel"),
        (Key::UPD_PROMPT, "UpdPrompt"),
        (Key::UPD_AUTO, "UpdAuto"),
    ];
    for &(key, name) in pairs {
        assert_eq!(key.name(), name, "常量与表行错位");
    }
    assert_eq!(NUMERIC_KEYS.len(), pairs.len());
}

/// The key count and order must match the ini row order.
///
/// The order decides the written row order: a mismatch produces a
/// meaningless diff when two implementations round-trip.
#[test]
fn key_list_matches_upstream_order() {
    // Compared by old name so the order stays stable across a round trip.
    let names: Vec<&str> = Key::ALL.iter().map(|k| k.legacy_name()).collect();
    assert_eq!(names.len(), 118, "行序表是 118 个键");
    // Spot checks: a wrong order fails here.
    assert_eq!(names[0], "IconSize");
    // `HPad` sits after `Margin`, shifting every later key by one.
    assert_eq!(names[4], "HPad");
    assert_eq!(names[26], "PlateMode");
    assert_eq!(names[29], "PlateRadius");
    assert_eq!(names[32], "BgColor3");
    assert_eq!(names[39], "UiTheme");
    assert_eq!(names[40], "MenuTheme");
    assert_eq!(names[76], "MnAdd");
    // The 23 second-batch keys follow `MnAdd`.
    assert_eq!(names[77], "Launchpad");
    assert_eq!(names[99], "ModeW");
    // The 18 third-batch keys are appended at the end.
    assert_eq!(names[100], "UiFrost");
    assert_eq!(names[117], "MenuFrost");
    // No duplicate key names.
    let mut seen = std::collections::HashSet::new();
    for n in &names {
        assert!(seen.insert(*n), "键名重复: {n}");
    }
}

#[test]
fn bool_keys_normalize_to_0_or_1() {
    let mut c = Config::defaults();
    c.apply_i32(Key::Dots, 7);
    assert!(c.dots);
    assert_eq!(c.get_i32(Key::Dots), 1);
    c.apply_i32(Key::Dots, -7);
    assert!(c.dots);
    c.apply_i32(Key::Dots, 0);
    assert!(!c.dots);
}

#[test]
fn key_name_lookup_is_case_insensitive() {
    assert_eq!(Key::from_name("iconsize"), Some(Key::IconSize));
    assert_eq!(Key::from_name("  Opacity "), Some(Key::Opacity));
    assert_eq!(Key::from_name("nope"), None);
}

#[test]
fn key_table_matches_c_g_tab() {
    // The key names are listed one by one, so a single wrong character
    // fails the test.
    const UPSTREAM: [&str; 118] = [
        "IconSize",
        "ZoomSize",
        "Spacing",
        "Margin",
        "HPad",
        "Padding",
        "Radius",
        "Opacity",
        "Blur",
        "Zoom",
        "Theme",
        "Dots",
        "Labels",
        "Bounce",
        "WebAppMode",
        "Border",
        "BgMode",
        "Mode",
        "Reserve",
        "Edge",
        "Monitor",
        "Preview",
        "ShowRunning",
        "Lock",
        "Hotkey",
        "DotStyle",
        "PlateMode",
        "PlateTheme",
        "PlateOpacity",
        "PlateRadius",
        "BgColor1",
        "BgColor2",
        "BgColor3",
        "AccentTone",
        "Reflect",
        "Clock",
        "FolderFan",
        "MinAnim",
        "Notes",
        "UiTheme",
        "MenuTheme",
        "BorderColor",
        "BorderWidth",
        "SepColor",
        "DotColor",
        "GradAngle",
        "GradA1",
        "GradA2",
        "GradA3",
        "ClockShow",
        "ClockColLight",
        "ClockColDark",
        "ReflSize",
        "ReflGap",
        "ReflOpacity",
        "FanArc",
        "FanGap",
        "FanCut",
        "GroupFrost",
        "GroupPos",
        "Guardian",
        "ClickToggle",
        "SepGap",
        "SepWidth",
        "ClockSec",
        "ClockWeek",
        "DateFmt",
        "ClockOrder",
        "ClockBoldT",
        "ClockBoldD",
        "Browser",
        "GrpOpacity",
        "GrpBg",
        "MinStyle",
        "MnAuto",
        "MnNotes",
        "MnAdd",
        // The 23 second-batch keys.
        "Launchpad",
        "WSide",
        "WOrder",
        "NotifyW",
        "FanBgL",
        "FanBgD",
        "FanFgL",
        "FanFgD",
        "PinMenu",
        "MultiMode",
        "UpdChk",
        "UpdLast",
        "LpCols",
        "LpRows",
        "FanDrag",
        "TrayHide",
        "CtrlCenter",
        "CcEnergy",
        "CcHotspot",
        "ResMon",
        "ToolW",
        "WideMask",
        "ModeW",
        // The 18 third-batch keys (`HPad` is listed above).
        "UiFrost",
        "LpFrost",
        "TrayW",
        "WMerge",
        "StatusBar",
        "SbFrost",
        "SbOpacity",
        "SbColor",
        "SbRadius",
        "SbHeight",
        "SbFollowSys",
        "SbTopGap",
        "SbMaxAvoid",
        "SbFloat",
        "SbSideGap",
        "SbAutoHide",
        "SepScale",
        "MenuFrost",
    ];
    assert_eq!(Key::ALL.len(), UPSTREAM.len());
    for (i, name) in UPSTREAM.iter().enumerate() {
        assert_eq!(Key::ALL[i].legacy_name(), *name, "第 {i} 个键对不上");
    }
    // Names must be unique in both sets, or `from_name` silently takes the
    // first match.
    for (i, k) in Key::ALL.iter().enumerate() {
        for other in &Key::ALL[i + 1..] {
            assert_ne!(k.name(), other.name(), "新名重复");
            assert_ne!(k.legacy_name(), other.legacy_name(), "旧名重复");
        }
    }
}

/// Every key needs a known section, otherwise the file scatters into
/// misspelled sections that no one can map back to a key.
#[test]
fn every_key_has_a_known_section() {
    const KNOWN: &[&str] = &[
        "appearance",
        "appearance.background",
        "appearance.plate",
        "appearance.reflect",
        "layout",
        "behavior",
        "behavior.group",
        "theme",
        "widgets",
        "widgets.clock",
        "widgets.launchpad",
        "widgets.statusbar",
        "menu",
        "advanced",
    ];
    for key in Key::ALL.iter().copied() {
        let sec = key.section();
        assert!(
            KNOWN.contains(&sec),
            "键 {} 的段 {sec} 不在已知列表里",
            key.name()
        );
    }
}

/// Numeric keys are grouped by prefix; spot-check the ones most likely to
/// land in the wrong section.
#[test]
fn numeric_keys_land_in_sensible_sections() {
    let by_name = |want: &str| {
        Key::ALL
            .iter()
            .copied()
            .find(|k| k.legacy_name() == want)
            .unwrap_or_else(|| panic!("找不到键 {want}"))
    };
    assert_eq!(by_name("PlateMode").section(), "appearance.plate");
    assert_eq!(by_name("GradAngle").section(), "appearance.background");
    assert_eq!(by_name("BgColor3").section(), "appearance.background");
    assert_eq!(by_name("ReflSize").section(), "appearance.reflect");
    assert_eq!(by_name("ClockShow").section(), "widgets.clock");
    assert_eq!(by_name("DateFmt").section(), "widgets.clock");
    assert_eq!(by_name("FanArc").section(), "behavior.group");
    assert_eq!(by_name("GroupFrost").section(), "behavior.group");
    assert_eq!(by_name("MnAuto").section(), "menu");
    assert_eq!(by_name("MenuTheme").section(), "menu");
    assert_eq!(by_name("Browser").section(), "advanced");
    assert_eq!(by_name("BorderColor").section(), "appearance");
}
