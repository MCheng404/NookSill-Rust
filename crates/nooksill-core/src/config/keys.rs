//! `[Settings]` key table: enum variants, section ownership, ranges, defaults.
//!
//! The table is the only key list; lookups live in `key_query`.

/// Keys of the `[Settings]` section. Enum order is the save order.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    IconSize,
    ZoomSize,
    Spacing,
    Margin,
    Padding,
    Radius,
    Opacity,
    Blur,
    Zoom,
    Theme,
    Dots,
    Labels,
    Bounce,
    WebAppMode,
    Border,
    BgMode,
    Mode,
    Reserve,
    Edge,
    Monitor,
    Preview,
    ShowRunning,
    Lock,
    Hotkey,
    DotStyle,
    PlateRadius,
    BgColor1,
    BgColor2,
    AccentTone,
    Reflect,
    Clock,
    FolderFan,
    MinAnim,
    Notes,
    UiTheme,
    /// Bare numeric key; the index points into [`NUMERIC_KEYS`].
    ///
    /// Most of these are plain `int` values with no enum semantics, so the
    /// table-driven form keeps "add a key" down to one row.
    Numeric(u16),
}

/// Section of a numeric key, decided by name.
///
/// Exact name matches win over prefix rules because later keys span several
/// semantic sections.
pub(super) fn section_of_numeric(name: &str) -> &'static str {
    match name {
        // Widget strip: position, order, toggles, double-width mask.
        "WSide" | "WOrder" | "WideMask" | "NotifyW" | "CtrlCenter" | "CcEnergy" | "CcHotspot"
        | "ResMon" | "ToolW" | "ModeW" | "PanelDismiss" => return "widgets",
        // Launchpad.
        "Launchpad" | "LpCols" | "LpRows" => return "widgets.launchpad",
        // System integration: Explorer context menu, tray icon, auto update.
        "PinMenu" | "TrayHide" | "UpdChk" | "UpdLast" => return "advanced",
        // Updater switches.
        "UpdChannel" | "UpdPrompt" | "UpdAuto" => return "advanced",
        // Multi-window app mode.
        "MultiMode" => return "behavior",
        // Fan label colors, same group as `FanArc` / `FanGap`.
        "FanBgL" | "FanBgD" | "FanFgL" | "FanFgD" | "FanDrag" => {
            return "behavior.group";
        }
        // Top status bar: its switch and appearance params form their own section.
        "StatusBar" | "SbFrost" | "SbOpacity" | "SbColor" | "SbRadius" | "SbHeight"
        | "SbFollowSys" | "SbTopGap" | "SbMaxAvoid" | "SbFloat" | "SbSideGap" | "SbAutoHide" => {
            return "widgets.statusbar"
        }
        // Tray widget and "merge all widgets into one capsule".
        "TrayW" | "WMerge" => return "widgets",
        // Launchpad frost.
        "LpFrost" => return "widgets.launchpad",
        // Settings frost, bar side padding, UI scale.
        "UiFrost" | "HPad" | "UiScale" | "UiAccent" | "UiAccentDock" | "UiAccentDockIcons"
        | "UiAccentCcIcons" | "FrostIllusion" => return "appearance",
        // Frame pacing and sync.
        "FrameCap" | "VSync" | "Msaa" => return "performance",
        // Updater.
        "NetSecureDns" | "NetMirrorAll" => return "update",
        // Context menu frost.
        "MenuFrost" => return "menu",
        // Separator drag scaling.
        "SepScale" => return "behavior",
        _ => {}
    }
    if name.starts_with("Plate") {
        "appearance.plate"
    } else if name.starts_with("Smoke") {
        // Ambient smoke has 11 keys; its own section keeps it out of `appearance`.
        "effect.smoke"
    } else if name.starts_with("Grad") || name.starts_with("BgColor") {
        "appearance.background"
    } else if name.starts_with("Refl") {
        "appearance.reflect"
    } else if name.starts_with("Clock") || name.starts_with("DateFmt") {
        "widgets.clock"
    } else if name.starts_with("Fan")
        || name.starts_with("Group")
        || name.starts_with("Grp")
        || name == "MinStyle"
    {
        "behavior.group"
    } else if name.starts_with("Mn") || name == "MenuTheme" {
        "menu"
    } else if name == "Browser" {
        "advanced"
    } else {
        // Borders, separators, dots, click toggle, guardian.
        "appearance"
    }
}

/// Bare numeric keys: `(ini name, min, max, default, is bool, is length)`.
///
/// The last column marks 96 DPI logical-pixel lengths, scaled by `dpi / 96`
/// at render time. [`Key::is_length`] is the single source of that decision,
/// so a new length key only needs the flag here. Percentages, angles, enums
/// and colors are never lengths.
///
/// Raw ints are not clamped elsewhere, so every key carries its own safe
/// range; the bounds follow the settings row type (`RSl` slider, `RCmb`
/// dropdown, `RTog` toggle, `RCol` color).
///
/// Row order is the ini write order and must not change.
pub const NUMERIC_KEYS: &[(&str, i32, i32, i32, bool, bool)] = &[
    ("PlateMode", 0, 2, 0, false, false),
    ("PlateTheme", 0, 2, 2, false, false),
    ("PlateOpacity", 0, 100, 100, false, false),
    ("BgColor3", -1, 16777215, -1, false, false),
    ("MenuTheme", 0, 2, 2, false, false),
    ("BorderColor", -1, 16777215, -1, false, false),
    ("BorderWidth", 1, 6, 1, false, false),
    ("SepColor", -1, 16777215, -1, false, false),
    ("DotColor", -1, 16777215, -1, false, false),
    ("GradAngle", 0, 360, 90, false, false),
    ("GradA1", 0, 100, 100, false, false),
    ("GradA2", 0, 100, 100, false, false),
    ("GradA3", 0, 100, 100, false, false),
    ("ClockShow", 0, 2, 0, false, false),
    ("ClockColLight", -1, 16777215, -1, false, false),
    ("ClockColDark", -1, 16777215, -1, false, false),
    ("ReflSize", 10, 80, 38, false, false),
    ("ReflGap", 0, 30, 3, false, true),
    ("ReflOpacity", 0, 100, 30, false, false),
    ("FanArc", 0, 100, 40, false, false),
    ("FanGap", 0, 40, 9, false, false),
    ("FanCut", 0, 1, 0, false, false),
    ("GroupFrost", 0, 1, 1, true, false),
    ("GroupPos", 0, 1, 0, false, false),
    ("Guardian", 0, 1, 1, true, false),
    ("ClickToggle", 0, 1, 1, true, false),
    ("SepGap", 0, 64, 4, false, true),
    ("SepWidth", 0, 16, 2, false, true),
    ("ClockSec", 0, 1, 0, true, false),
    ("ClockWeek", 0, 1, 1, true, false),
    ("DateFmt", 0, 2, 0, false, false),
    ("ClockOrder", 0, 1, 0, false, false),
    ("ClockBoldT", 0, 1, 1, true, false),
    ("ClockBoldD", 0, 1, 0, true, false),
    ("Browser", 0, 4, 0, false, false),
    ("GrpOpacity", 20, 100, 94, false, false),
    ("GrpBg", -1, 16777215, -1, false, false),
    ("MinStyle", 0, 1, 0, false, false),
    ("MnAuto", 0, 1, 1, true, false),
    ("MnNotes", 0, 1, 1, true, false),
    ("MnAdd", 0, 1, 1, true, false),
    // Second batch, all appended after `MnAdd`. Color keys use -1 = follow theme.
    ("Launchpad", 0, 1, 0, true, false),
    // WSide: widget strip on the right (0) or left (1) of the bar.
    ("WSide", 0, 1, 0, true, false),
    // WOrder: clock first (0) or notifications first (1).
    ("WOrder", 0, 1, 0, true, false),
    // NotifyW: notification widget.
    ("NotifyW", 0, 1, 0, true, false),
    // Fan{Bg,Fg}{L,D}: fan label background and text color, light and dark theme.
    ("FanBgL", -1, 16777215, -1, false, false),
    ("FanBgD", -1, 16777215, -1, false, false),
    ("FanFgL", -1, 16777215, -1, false, false),
    ("FanFgD", -1, 16777215, -1, false, false),
    // PinMenu: add "Pin to NookSill" to the Explorer context menu.
    ("PinMenu", 0, 1, 0, true, false),
    // MultiMode: merge a multi-window app into one icon (0) or one icon per
    // document window (1).
    ("MultiMode", 0, 1, 0, false, false),
    // UpdChk: check for updates daily; UpdLast: day stamp of the last check.
    ("UpdChk", 0, 1, 1, true, false),
    ("UpdLast", 0, 100000, 0, false, false),
    // LpCols / LpRows: launchpad columns and rows per page. Clamped here
    // rather than at use.
    ("LpCols", 3, 12, 7, false, false),
    ("LpRows", 2, 8, 4, false, false),
    // FanDrag: allow dragging files out of the fan.
    ("FanDrag", 0, 1, 1, true, false),
    // TrayHide: hide the notification area icons.
    ("TrayHide", 0, 1, 0, true, false),
    // CtrlCenter: control center widget; CcEnergy / CcHotspot: its tiles.
    ("CtrlCenter", 0, 1, 1, true, false),
    ("CcEnergy", 0, 1, 1, true, false),
    ("CcHotspot", 0, 1, 1, true, false),
    // ResMon: resource monitor widget.
    ("ResMon", 0, 1, 1, true, false),
    // ToolW: timer and todo widget.
    ("ToolW", 0, 1, 1, true, false),
    // WideMask: bit k marks widget k as double width; there are 6 widgets.
    ("WideMask", 0, 63, 0, false, false),
    // ModeW: focus mode widget.
    ("ModeW", 0, 1, 1, true, false),
    // Third batch. `HPad` sits after `Margin` in the ini row order but is a
    // constant index here, so new rows are appended to keep indices stable.
    ("HPad", 0, 40, 12, false, true),
    ("UiFrost", 0, 1, 1, true, false),
    ("LpFrost", 0, 1, 1, true, false),
    ("TrayW", 0, 1, 1, true, false),
    ("WMerge", 0, 1, 1, true, false),
    ("StatusBar", 0, 1, 0, true, false),
    ("SbFrost", 0, 1, 1, true, false),
    ("SbOpacity", 0, 100, 88, false, false),
    ("SbColor", -1, 16777215, -1, false, false),
    ("SbRadius", 0, 24, 10, false, false),
    ("SbHeight", 22, 48, 32, false, false),
    ("SbFollowSys", 0, 1, 1, true, false),
    ("SbTopGap", 0, 80, 0, false, false),
    ("SbMaxAvoid", 0, 1, 0, true, false),
    ("SbFloat", 0, 1, 0, true, false),
    ("SbSideGap", 0, 80, 12, false, false),
    ("SbAutoHide", 0, 1, 0, true, false),
    ("SepScale", 0, 1, 0, true, false),
    ("MenuFrost", 0, 1, 0, true, false),
    // Own keys, see `Key::EXTRA`. Smoke ranges hold option indices, not
    // ratios: the runtime indexes an array with them and the ini stores i32.
    ("Smoke", 0, 1, 1, true, false),
    ("SmokeColor", 0, 9, 0, false, false),
    ("SmokeColor2", -1, 7, -1, false, false),
    ("SmokeOpacity", 0, 6, 3, false, false),
    ("SmokeDensity", 0, 2, 1, false, false),
    ("SmokeSpeed", 0, 2, 1, false, false),
    ("SmokeTone", 0, 1, 0, true, false),
    ("SmokeThick", 0, 3, 1, false, false),
    ("SmokeMouse", 0, 2, 1, false, false),
    ("SmokeReach", 0, 2, 1, false, false),
    ("SmokeClouds", 0, 1, 1, true, false),
    // PanelDismiss: dismiss the widget panel by clicking outside it.
    ("PanelDismiss", 0, 1, 1, true, false),
    // UiScale: settings window UI scale percent, 0 = follow system. The dock
    // scales through its own size keys; 80 is the floor for a usable window.
    ("UiScale", 0, 300, 0, false, false),
    // UiAccent: -1 = follow the system accent, 0..0xFFFFFF = COLORREF.
    // UiAccentDock mirrors it onto the dock bar.
    ("UiAccent", -1, 16777215, -1, false, false),
    // UiAccentDock: apply the accent to the dock bar and running dots.
    ("UiAccentDock", 0, 1, 0, true, false),
    // Frame pacing. FrameCap 0 = unlimited; VSync aligns to DWM refresh.
    ("FrameCap", 0, 240, 0, false, false),
    ("VSync", 0, 1, 0, true, false),
    // MSAA samples: 0 = auto by window area, 1/2/4 = forced (next launch).
    ("Msaa", 0, 4, 0, false, false),
    // UiAccentDockIcons / UiAccentCcIcons are independent of UiAccentDock and
    // on by default; turning one off falls back to that window's neutral color.
    ("UiAccentDockIcons", 0, 1, 1, true, false),
    ("UiAccentCcIcons", 0, 1, 1, true, false),
    // Updater, hidden behind the About page.
    ("NetSecureDns", 0, 1, 1, true, false),
    ("NetMirrorAll", 0, 1, 0, true, false),
    // Auto-hide delays in ms, one pair per scenario: Hide* for the cursor
    // leaving the bar, Show* for the cursor entering the hot zone.
    ("HideDelay", 0, 5000, 450, false, false),
    ("ShowDelay", 0, 5000, 180, false, false),
    ("HideDelayMax", 0, 5000, 900, false, false),
    ("ShowDelayMax", 0, 5000, 400, false, false),
    // Widget appearance, 0 = follow the bar or global frost.
    ("WidgetOpacity", 0, 100, 0, false, false),
    ("WidgetRadius", 0, 40, 0, false, true),
    ("WidgetGap", 0, 24, 6, false, true),
    ("WidgetBlur", 0, 14, 0, false, false),
    // Pop-up panel, 0 = follow the frost default.
    ("PanelOpacity", 0, 100, 0, false, false),
    ("PanelOverlay", 0, 100, 28, false, false),
    // Experimental temporal reprojection blur, off by default.
    ("FrostIllusion", 0, 1, 0, true, false),
    // Updater: channel selection, update prompt and silent auto-download.
    ("UpdChannel", 0, 1, 0, false, false),
    ("UpdPrompt", 0, 1, 1, true, false),
    ("UpdAuto", 0, 1, 0, true, false),
];

impl Key {
    // Named constants for the table rows; the index is the row index in
    // `NUMERIC_KEYS` and a test keeps the two in sync.

    /// `PlateMode`
    pub const PLATE_MODE: Key = Key::Numeric(0);
    /// `PlateTheme`
    pub const PLATE_THEME: Key = Key::Numeric(1);
    /// `PlateOpacity`
    pub const PLATE_OPACITY: Key = Key::Numeric(2);
    /// `BgColor3`
    pub const BG_COLOR3: Key = Key::Numeric(3);
    /// `MenuTheme`
    pub const MENU_THEME: Key = Key::Numeric(4);
    /// `BorderColor`
    pub const BORDER_COLOR: Key = Key::Numeric(5);
    /// `BorderWidth`
    pub const BORDER_WIDTH: Key = Key::Numeric(6);
    /// `SepColor`
    pub const SEP_COLOR: Key = Key::Numeric(7);
    /// `DotColor`
    pub const DOT_COLOR: Key = Key::Numeric(8);
    /// `GradAngle`
    pub const GRAD_ANGLE: Key = Key::Numeric(9);
    /// `GradA1`
    pub const GRAD_A1: Key = Key::Numeric(10);
    /// `GradA2`
    pub const GRAD_A2: Key = Key::Numeric(11);
    /// `GradA3`
    pub const GRAD_A3: Key = Key::Numeric(12);
    /// `ClockShow`
    pub const CLOCK_SHOW: Key = Key::Numeric(13);
    /// `ClockColLight`
    pub const CLOCK_COL_LIGHT: Key = Key::Numeric(14);
    /// `ClockColDark`
    pub const CLOCK_COL_DARK: Key = Key::Numeric(15);
    /// `ReflSize`
    pub const REFL_SIZE: Key = Key::Numeric(16);
    /// `ReflGap`
    pub const REFL_GAP: Key = Key::Numeric(17);
    /// `ReflOpacity`
    pub const REFL_OPACITY: Key = Key::Numeric(18);
    /// `FanArc`
    pub const FAN_ARC: Key = Key::Numeric(19);
    /// `FanGap`
    pub const FAN_GAP: Key = Key::Numeric(20);
    /// `FanCut`
    pub const FAN_CUT: Key = Key::Numeric(21);
    /// `GroupFrost`
    pub const GROUP_FROST: Key = Key::Numeric(22);
    /// `GroupPos`
    pub const GROUP_POS: Key = Key::Numeric(23);
    /// `Guardian`
    pub const GUARDIAN: Key = Key::Numeric(24);
    /// `ClickToggle`
    pub const CLICK_TOGGLE: Key = Key::Numeric(25);
    /// `SepGap`
    pub const SEP_GAP: Key = Key::Numeric(26);
    /// `SepWidth`
    pub const SEP_WIDTH: Key = Key::Numeric(27);
    /// `ClockSec`
    pub const CLOCK_SEC: Key = Key::Numeric(28);
    /// `ClockWeek`
    pub const CLOCK_WEEK: Key = Key::Numeric(29);
    /// `DateFmt`
    pub const DATE_FMT: Key = Key::Numeric(30);
    /// `ClockOrder`
    pub const CLOCK_ORDER: Key = Key::Numeric(31);
    /// `ClockBoldT`
    pub const CLOCK_BOLD_T: Key = Key::Numeric(32);
    /// `ClockBoldD`
    pub const CLOCK_BOLD_D: Key = Key::Numeric(33);
    /// `Browser`
    pub const BROWSER: Key = Key::Numeric(34);
    /// `GrpOpacity`
    pub const GRP_OPACITY: Key = Key::Numeric(35);
    /// `GrpBg`
    pub const GRP_BG: Key = Key::Numeric(36);
    /// `MinStyle`
    pub const MIN_STYLE: Key = Key::Numeric(37);
    /// `MnAuto`
    pub const MN_AUTO: Key = Key::Numeric(38);
    /// `MnNotes`
    pub const MN_NOTES: Key = Key::Numeric(39);
    /// `MnAdd`
    pub const MN_ADD: Key = Key::Numeric(40);
    /// `Launchpad`
    pub const LAUNCHPAD: Key = Key::Numeric(41);
    /// `WSide`
    pub const W_SIDE: Key = Key::Numeric(42);
    /// `WOrder`
    pub const W_ORDER: Key = Key::Numeric(43);
    /// `NotifyW`
    pub const NOTIFY_W: Key = Key::Numeric(44);
    /// `FanBgL`
    pub const FAN_BG_L: Key = Key::Numeric(45);
    /// `FanBgD`
    pub const FAN_BG_D: Key = Key::Numeric(46);
    /// `FanFgL`
    pub const FAN_FG_L: Key = Key::Numeric(47);
    /// `FanFgD`
    pub const FAN_FG_D: Key = Key::Numeric(48);
    /// `PinMenu`
    pub const PIN_MENU: Key = Key::Numeric(49);
    /// `MultiMode`
    pub const MULTI_MODE: Key = Key::Numeric(50);
    /// `UpdChk`
    pub const UPD_CHK: Key = Key::Numeric(51);
    /// `UpdLast`
    pub const UPD_LAST: Key = Key::Numeric(52);
    /// `LpCols`
    pub const LP_COLS: Key = Key::Numeric(53);
    /// `LpRows`
    pub const LP_ROWS: Key = Key::Numeric(54);
    /// `FanDrag`
    pub const FAN_DRAG: Key = Key::Numeric(55);
    /// `TrayHide`
    pub const TRAY_HIDE: Key = Key::Numeric(56);
    /// `CtrlCenter`
    pub const CTRL_CENTER: Key = Key::Numeric(57);
    /// `CcEnergy`
    pub const CC_ENERGY: Key = Key::Numeric(58);
    /// `CcHotspot`
    pub const CC_HOTSPOT: Key = Key::Numeric(59);
    /// `ResMon`
    pub const RES_MON: Key = Key::Numeric(60);
    /// `ToolW`
    pub const TOOL_W: Key = Key::Numeric(61);
    /// `WideMask`
    pub const WIDE_MASK: Key = Key::Numeric(62);
    /// `ModeW`
    pub const MODE_W: Key = Key::Numeric(63);
    /// `HPad`
    pub const HPAD: Key = Key::Numeric(64);
    /// `UiFrost`
    pub const UI_FROST: Key = Key::Numeric(65);
    /// `LpFrost`
    pub const LP_FROST: Key = Key::Numeric(66);
    /// `TrayW`
    pub const TRAY_W: Key = Key::Numeric(67);
    /// `WMerge`
    pub const W_MERGE: Key = Key::Numeric(68);
    /// `StatusBar`
    pub const STATUS_BAR: Key = Key::Numeric(69);
    /// `SbFrost`
    pub const SB_FROST: Key = Key::Numeric(70);
    /// `SbOpacity`
    pub const SB_OPACITY: Key = Key::Numeric(71);
    /// `SbColor`
    pub const SB_COLOR: Key = Key::Numeric(72);
    /// `SbRadius`
    pub const SB_RADIUS: Key = Key::Numeric(73);
    /// `SbHeight`
    pub const SB_HEIGHT: Key = Key::Numeric(74);
    /// `SbFollowSys`
    pub const SB_FOLLOW_SYS: Key = Key::Numeric(75);
    /// `SbTopGap`
    pub const SB_TOP_GAP: Key = Key::Numeric(76);
    /// `SbMaxAvoid`
    pub const SB_MAX_AVOID: Key = Key::Numeric(77);
    /// `SbFloat`
    pub const SB_FLOAT: Key = Key::Numeric(78);
    /// `SbSideGap`
    pub const SB_SIDE_GAP: Key = Key::Numeric(79);
    /// `SbAutoHide`
    pub const SB_AUTO_HIDE: Key = Key::Numeric(80);
    /// `SepScale`
    pub const SEP_SCALE: Key = Key::Numeric(81);
    /// `MenuFrost`
    pub const MENU_FROST: Key = Key::Numeric(82);

    // Own keys, kept out of the sequence above: inserting one would shift
    // every later constant silently. New keys are appended at the end.
    /// `Smoke`
    pub const SMOKE: Key = Key::Numeric(83);
    /// `SmokeColor`
    pub const SMOKE_COLOR: Key = Key::Numeric(84);
    /// `SmokeColor2`
    pub const SMOKE_COLOR2: Key = Key::Numeric(85);
    /// `SmokeOpacity`
    pub const SMOKE_OPACITY: Key = Key::Numeric(86);
    /// `SmokeDensity`
    pub const SMOKE_DENSITY: Key = Key::Numeric(87);
    /// `SmokeSpeed`
    pub const SMOKE_SPEED: Key = Key::Numeric(88);
    /// `SmokeTone`
    pub const SMOKE_TONE: Key = Key::Numeric(89);
    /// `SmokeThick`
    pub const SMOKE_THICK: Key = Key::Numeric(90);
    /// `SmokeMouse`
    pub const SMOKE_MOUSE: Key = Key::Numeric(91);
    /// `SmokeReach`
    pub const SMOKE_REACH: Key = Key::Numeric(92);
    /// `SmokeClouds`
    pub const SMOKE_CLOUDS: Key = Key::Numeric(93);
    /// `PanelDismiss`
    pub const PANEL_DISMISS: Key = Key::Numeric(94);
    /// `UiScale`: settings window UI scale percent; 0 = follow system.
    pub const UI_SCALE: Key = Key::Numeric(95);
    /// `UiAccent`: UI accent color; -1 = follow the system accent.
    pub const UI_ACCENT: Key = Key::Numeric(96);
    /// `UiAccentDock`: sync the accent color to the dock.
    pub const UI_ACCENT_DOCK: Key = Key::Numeric(97);
    /// `FrameCap`: frame rate cap; 0 = unlimited.
    pub const FRAME_CAP: Key = Key::Numeric(98);
    /// `VSync`: align to the DWM refresh rate.
    pub const VSYNC: Key = Key::Numeric(99);
    /// `Msaa`: 0 = auto by window area, 1/2/4 = forced level.
    pub const MSAA: Key = Key::Numeric(100);
    /// `NetSecureDns`: verify system DNS over DoH during updates (default on).
    pub const NET_SECURE_DNS: Key = Key::Numeric(103);
    /// `NetMirrorAll`: wait for every mirror before picking one.
    pub const NET_MIRROR_ALL: Key = Key::Numeric(104);
    /// `HideDelay`: delay before hiding once the cursor leaves, in ms.
    pub const HIDE_DELAY: Key = Key::Numeric(105);
    /// `ShowDelay`: dwell time in the hot zone before showing again, in ms.
    pub const SHOW_DELAY: Key = Key::Numeric(106);
    /// `HideDelayMax`: hide delay while a foreground window is maximized, in ms.
    pub const HIDE_DELAY_MAX: Key = Key::Numeric(107);
    /// `ShowDelayMax`: show delay while a foreground window is maximized, in ms.
    pub const SHOW_DELAY_MAX: Key = Key::Numeric(108);
    /// `WidgetOpacity`: widget card opacity; 0 = follow the bar.
    pub const WIDGET_OPACITY: Key = Key::Numeric(109);
    /// `WidgetRadius`: widget card corner radius; 0 = follow the bar.
    pub const WIDGET_RADIUS: Key = Key::Numeric(110);
    /// `WidgetGap`: extra gap between widget strip members, in logical pixels.
    pub const WIDGET_GAP: Key = Key::Numeric(111);
    /// `WidgetBlur`: pop-up blur radius; 0 = follow the global `Blur`.
    pub const WIDGET_BLUR: Key = Key::Numeric(112);
    /// `PanelOpacity`: panel fill opacity; 0 = follow the frost.
    pub const PANEL_OPACITY: Key = Key::Numeric(113);
    /// `PanelOverlay`: panel overlay (haze) strength.
    pub const PANEL_OVERLAY: Key = Key::Numeric(114);
    /// `FrostIllusion`: experimental temporal reprojection blur; 0 = keep the
    /// plain Gaussian.
    pub const FROST_ILLUSION: Key = Key::Numeric(115);
    /// `UpdChannel`: 0 = stable channel, 1 = beta channel.
    pub const UPD_CHANNEL: Key = Key::Numeric(116);
    /// `UpdPrompt`: show a centered update dialog on opening settings.
    pub const UPD_PROMPT: Key = Key::Numeric(117);
    /// `UpdAuto`: download silently and replace on the next launch.
    pub const UPD_AUTO: Key = Key::Numeric(118);

    /// `UiAccentDockIcons`: apply the accent color to dock icons.
    pub const UI_ACCENT_DOCK_ICONS: Key = Key::Numeric(101);
    /// `UiAccentCcIcons`: apply the accent color to control center tile icons.
    pub const UI_ACCENT_CC_ICONS: Key = Key::Numeric(102);
}
