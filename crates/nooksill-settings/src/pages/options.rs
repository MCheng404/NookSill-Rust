//! Option tables for widgets; the integers are the codes stored in the INI.
//!
//! Shared here rather than per page so a combo box that appears twice (the
//! icon reflection shows up on two pages) cannot drift in meaning.

pub(crate) const THEME_OPTS: &[(i32, &str)] = &[(0, "深色"), (1, "浅色"), (2, "跟随系统")];

/// Note: `UiTheme` uses the opposite 0/1 order from `DockTheme`.
pub(crate) const UI_THEME_OPTS: &[(i32, &str)] = &[(0, "浅色"), (1, "深色"), (2, "跟随系统")];

pub(crate) const BG_OPTS: &[(i32, &str)] = &[(0, "纯色"), (1, "渐变"), (2, "磨砂玻璃")];

pub(crate) const EDGE_OPTS: &[(i32, &str)] = &[
    (0, "屏幕底部"),
    (1, "屏幕顶部"),
    (2, "屏幕左侧"),
    (3, "屏幕右侧"),
];

pub(crate) const MODE_OPTS: &[(i32, &str)] = &[(0, "常驻显示"), (1, "自动隐藏"), (2, "智能隐藏")];

pub(crate) const PLATE_OPTS: &[(i32, &str)] = &[(0, "无底盘"), (1, "纯色底盘"), (2, "半透明底盘")];

pub(crate) const PLATE_THEME_OPTS: &[(i32, &str)] =
    &[(0, "浅色"), (1, "深色"), (2, "跟随 Dock 主题")];

pub(crate) const DOT_OPTS: &[(i32, &str)] = &[(0, "横条"), (1, "圆点")];

pub(crate) const MENU_THEME_OPTS: &[(i32, &str)] = &[(0, "浅色"), (1, "深色"), (2, "跟随系统")];

pub(crate) const MIN_STYLE_OPTS: &[(i32, &str)] = &[
    (0, "压缩飞入（类似 macOS）"),
    (1, "弹性飞入（类似手机，非线性）"),
];

pub(crate) const CLOCK_SHOW_OPTS: &[(i32, &str)] = &[
    (0, "时间 + 日期（上下分栏）"),
    (1, "仅显示时间"),
    (2, "仅显示日期"),
];

pub(crate) const DATE_FMT_OPTS: &[(i32, &str)] =
    &[(0, "2026年10月22日"), (1, "2026/10/22"), (2, "2026-10-22")];

pub(crate) const BROWSER_OPTS: &[(i32, &str)] = &[
    (0, "自动（Edge 优先）"),
    (1, "Microsoft Edge"),
    (2, "Google Chrome"),
    (3, "Mozilla Firefox"),
    (4, "系统默认浏览器"),
];

/// Indices must match `core::smoke::PALETTE`, whose order is the upstream
/// `kRauchRgb` order and not the order of this list's last two entries.
pub(crate) const SMOKE_COLOR_OPTS: &[(i32, &str)] = &[
    (0, "紫"),
    (1, "橙"),
    (2, "红"),
    (3, "粉"),
    (4, "浅蓝"),
    (5, "冰蓝"),
    (6, "绿"),
    (7, "金"),
    (8, "跟随系统强调色"),
    (9, "彩虹（90 秒一圈）"),
];

/// Second colour, blended left to right along the bar; `-1` disables it.
pub(crate) const SMOKE_COLOR2_OPTS: &[(i32, &str)] = &[
    (-1, "无"),
    (0, "紫"),
    (1, "橙"),
    (2, "红"),
    (3, "粉"),
    (4, "浅蓝"),
    (5, "冰蓝"),
    (6, "绿"),
    (7, "金"),
];

/// The two steps above 100% are alpha gain, not a different colour ramp.
pub(crate) const SMOKE_OPACITY_OPTS: &[(i32, &str)] = &[
    (0, "17%"),
    (1, "30%"),
    (2, "50%"),
    (3, "70%"),
    (4, "100%"),
    (5, "125%"),
    (6, "150%"),
];

pub(crate) const SMOKE_DENSITY_OPTS: &[(i32, &str)] = &[(0, "稀疏"), (1, "正常"), (2, "浓密")];

pub(crate) const SMOKE_SPEED_OPTS: &[(i32, &str)] = &[(0, "平缓"), (1, "正常"), (2, "活泼")];

/// Glow thickness, which also sets the simulated outer margin.
pub(crate) const SMOKE_THICK_OPTS: &[(i32, &str)] =
    &[(0, "细"), (1, "正常"), (2, "厚"), (3, "极厚")];

pub(crate) const SMOKE_REACH_OPTS: &[(i32, &str)] = &[(0, "小"), (1, "正常"), (2, "大")];

/// 0 picks the level from window area, using 2x for large windows to save VRAM.
pub(crate) const MSAA_OPTS: &[(i32, &str)] = &[
    (0, "自动（大窗口 2x）"),
    (1, "关闭"),
    (2, "2x"),
    (4, "4x（最清晰）"),
];
