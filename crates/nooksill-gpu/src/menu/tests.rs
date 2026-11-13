//! Menu construction and layout tests.

use super::paint::estimate_width;
use super::*;

use nooksill_core::item::Item;

fn item(kind: ItemKind, name: &str) -> Item {
    Item::new(kind, name, "")
}

/// A plain app offers open, rename, duplicate, separator and remove.
#[test]
fn file_menu_has_open_and_remove() {
    let it = item(ItemKind::File, "记事本");
    let m = Menu::build(Some(&it), false, false, MenuOpts::default()).expect("应当有菜单");
    let cmds: Vec<_> = m.entries.iter().map(|e| e.cmd).collect();
    assert_eq!(
        cmds,
        vec![
            MenuCmd::Open,
            MenuCmd::Rename,
            MenuCmd::Duplicate,
            MenuCmd::ChangeIcon,
            MenuCmd::ResetIcon,
            MenuCmd::AddSepAfter,
            MenuCmd::RemoveItem,
            MenuCmd::Launchpad,
            MenuCmd::Settings,
        ]
    );
    assert!(!m.entries[0].danger);
    assert!(m.entries[5].sep_after, "加分割线之后要画分隔");
    assert!(m.entries[6].danger, "移除类必须是警告色");
}

/// The notes entry in the blank-area menu follows its switch.
#[test]
fn notes_entry_follows_the_switch() {
    let opts = MenuOpts {
        autostart: None,
        add_items: false,
        notes: true,
        windows: Vec::new(),
    };
    let on = Menu::build(None, false, false, opts.clone()).expect("应当有菜单");
    assert!(on.entries.iter().any(|e| e.cmd == MenuCmd::Notes));
    let off = Menu::build(
        None,
        false,
        false,
        MenuOpts {
            notes: false,
            ..opts
        },
    )
    .expect("应当有菜单");
    assert!(
        !off.entries.iter().any(|e| e.cmd == MenuCmd::Notes),
        "关掉开关后不该留一个死入口"
    );
}

/// The tray menu toggles the Dock and carries none of the new-item commands.
#[test]
fn tray_menu_offers_toggle_settings_and_exit() {
    let hidden = Menu::tray(true);
    let cmds: Vec<_> = hidden.entries.iter().map(|e| e.cmd).collect();
    assert_eq!(
        cmds,
        vec![
            MenuCmd::ToggleDock,
            MenuCmd::Launchpad,
            MenuCmd::Settings,
            MenuCmd::ExitDock,
        ]
    );
    assert_eq!(hidden.entries[0].label, "显示 Dock");
    assert_eq!(Menu::tray(false).entries[0].label, "隐藏 Dock");
    assert!(!hidden.entries.iter().any(|e| e.cmd == MenuCmd::AddFile));
}

/// A group can be opened, renamed and dissolved.
#[test]
fn group_menu_offers_open_rename_and_dissolve() {
    let g = item(ItemKind::Group, "工具");
    let m = Menu::build(Some(&g), false, false, MenuOpts::default()).expect("应当有菜单");
    let cmds: Vec<_> = m.entries.iter().map(|e| e.cmd).collect();
    assert_eq!(
        cmds,
        vec![
            MenuCmd::OpenGroup,
            MenuCmd::Rename,
            MenuCmd::DissolveGroup,
            MenuCmd::AddSepAfter,
            MenuCmd::RemoveItem,
            MenuCmd::Launchpad,
            MenuCmd::Settings,
        ]
    );
    assert!(
        m.entries
            .iter()
            .any(|e| e.cmd == MenuCmd::DissolveGroup && e.danger),
        "解散是不可逆操作，要用警告色"
    );
    assert!(
        !m.entries.iter().any(|e| e.cmd == MenuCmd::Open),
        "组不能走\"打开应用\"那条路"
    );
}

/// A running app replaces open with show/minimise and adds quit.
#[test]
fn running_file_menu_offers_toggle_and_quit() {
    let it = item(ItemKind::File, "记事本");
    let m = Menu::build(Some(&it), true, false, MenuOpts::default()).expect("应当有菜单");
    assert_eq!(m.entries[0].label, "显示 / 最小化");
    assert!(m.entries.iter().any(|e| e.cmd == MenuCmd::QuitApp));
}

/// An idle app must not offer quit.
#[test]
fn idle_file_menu_has_no_quit() {
    let it = item(ItemKind::File, "记事本");
    let m = Menu::build(Some(&it), false, false, MenuOpts::default()).expect("应当有菜单");
    assert!(!m.entries.iter().any(|e| e.cmd == MenuCmd::QuitApp));
}

/// Only folders offer open-in-explorer.
#[test]
fn only_folders_open_in_explorer() {
    let folder = item(ItemKind::Folder, "下载");
    assert!(
        Menu::build(Some(&folder), false, false, MenuOpts::default())
            .expect("应当有菜单")
            .entries
            .iter()
            .any(|e| e.cmd == MenuCmd::OpenInExplorer)
    );
    let file = item(ItemKind::File, "记事本");
    assert!(!Menu::build(Some(&file), false, false, MenuOpts::default())
        .expect("应当有菜单")
        .entries
        .iter()
        .any(|e| e.cmd == MenuCmd::OpenInExplorer));
}

/// The blank area addresses the Dock itself: exit, launchpad, settings.
#[test]
fn blank_area_offers_settings_and_exit() {
    // Both dock-level switches off, to get the minimal menu.
    let opts = MenuOpts {
        autostart: None,
        add_items: false,
        notes: false,
        windows: Vec::new(),
    };
    let m = Menu::build(None, false, false, opts.clone()).expect("应当有菜单");
    let cmds: Vec<MenuCmd> = m.entries.iter().map(|e| e.cmd).collect();
    assert_eq!(
        cmds,
        vec![MenuCmd::ExitDock, MenuCmd::Launchpad, MenuCmd::Settings]
    );
    // Settings are separated from exit, whose consequences differ a lot.
    assert!(m.entries[0].danger);
    assert!(m.entries[0].sep_after);
}

/// The blank-area menu offers add-file and add-folder when enabled.
#[test]
fn blank_menu_offers_new_items_when_enabled() {
    let m = Menu::build(
        None,
        false,
        false,
        MenuOpts {
            autostart: None,
            add_items: true,
            notes: false,
            windows: Vec::new(),
        },
    )
    .expect("应当有菜单");
    let labels: Vec<&str> = m.entries.iter().map(|e| e.label.as_str()).collect();
    assert_eq!(
        &labels[..4],
        &["添加文件…", "添加文件夹…", "新建文件夹…", "新建应用组…"]
    );
    assert!(labels.contains(&"退出 Dock"));
    // New-folder and new-group are separated: one is a disk directory,
    // the other one lives inside the Dock.
    assert!(m.entries[2].sep_after);
}

/// The autostart entry carries its state and reserves a check gutter.
#[test]
fn autostart_entry_reflects_state_and_reserves_a_gutter() {
    let on = Menu::build(
        None,
        false,
        false,
        MenuOpts {
            autostart: Some(true),
            add_items: false,
            notes: false,
            windows: Vec::new(),
        },
    )
    .expect("应当有菜单");
    let entry = on
        .entries
        .iter()
        .find(|e| e.cmd == MenuCmd::ToggleAutostart)
        .expect("应当有开机自启项");
    assert!(entry.checked, "开启时要显示勾");

    let off = Menu::build(
        None,
        false,
        false,
        MenuOpts {
            autostart: Some(false),
            add_items: false,
            notes: false,
            windows: Vec::new(),
        },
    )
    .expect("应当有菜单");
    let entry = off
        .entries
        .iter()
        .find(|e| e.cmd == MenuCmd::ToggleAutostart)
        .expect("应当有开机自启项");
    assert!(!entry.checked, "关闭时不该有勾");

    // A checkable entry reserves the gutter; whether the width grows
    // depends on MIN_W, so ask `gutter` instead of guessing from labels.
    let narrow = Menu::build(
        None,
        false,
        false,
        MenuOpts {
            autostart: None,
            add_items: false,
            notes: false,
            windows: Vec::new(),
        },
    )
    .expect("应当有菜单");
    assert!(on.gutter() > 0.0, "有勾选项时要留槽");
    assert_eq!(narrow.gutter(), 0.0, "没有勾选项就不留");
}

/// Dock-level switches must not leak into item menus.
#[test]
fn item_menu_ignores_dock_level_options() {
    let it = item(ItemKind::File, "应用");
    let m = Menu::build(
        Some(&it),
        false,
        false,
        MenuOpts {
            autostart: Some(true),
            add_items: true,
            notes: false,
            windows: Vec::new(),
        },
    )
    .expect("应当有菜单");
    assert!(!m.entries.iter().any(|e| e.cmd == MenuCmd::AddFile));
    assert!(!m.entries.iter().any(|e| e.cmd == MenuCmd::ToggleAutostart));
}

/// A temp item offers only open and quit.
///
/// The structural commands edit the config by index, and a temp item has
/// no config slot, so they would hit the wrong entry.
#[test]
fn temp_item_has_no_structural_commands() {
    let mut it = Item::new(ItemKind::File, "Chrome", r"C:\x\chrome.exe");
    it.running = true;
    let m = Menu::build(Some(&it), true, true, MenuOpts::default()).expect("应当有菜单");
    let has = |c: MenuCmd| m.entries.iter().any(|e| e.cmd == c);
    assert!(has(MenuCmd::Open), "临时条目必须能点击打开");
    assert!(has(MenuCmd::QuitApp), "运行中的条目要能退出");
    assert!(!has(MenuCmd::RemoveItem), "临时条目不在配置里，不能移除");
    assert!(!has(MenuCmd::AddSepAfter), "临时条目不能插分割线");
}

/// A separator offers delete, plus the always-present settings entry.
#[test]
fn separator_offers_delete_and_settings() {
    let sep = item(ItemKind::Separator, "");
    let m = Menu::build(Some(&sep), false, false, MenuOpts::default()).expect("应当有菜单");
    let cmds: Vec<_> = m.entries.iter().map(|e| e.cmd).collect();
    assert_eq!(
        cmds,
        vec![MenuCmd::RemoveSep, MenuCmd::Launchpad, MenuCmd::Settings]
    );
    assert!(m.entries[0].danger);
}

/// Hit testing and painting must share the vertical coordinates.
///
/// Separator height counts too: without it every entry below a separator
/// shifts, so the user sees one row and clicks another.
#[test]
fn hit_matches_layout_with_separators() {
    let m = Menu {
        entries: vec![
            MenuEntry::new("打开", MenuCmd::Open, false, false),
            MenuEntry::new("退出应用", MenuCmd::QuitApp, false, true),
            MenuEntry::new("移除", MenuCmd::ExitDock, true, false),
        ],
        hover: None,
    };
    assert_eq!(m.hit(10.0, PAD_Y + 1.0), Some(0));
    assert_eq!(m.hit(10.0, PAD_Y + ITEM_H + 1.0), Some(1));
    // The separator band comes after entry 1 and is not clickable.
    let sep_band = PAD_Y + ITEM_H * 2.0 + 2.0;
    assert_eq!(m.hit(10.0, sep_band), None, "分隔带不该命中任何项");
    // Entry 2 starts after the separator band.
    let third_top = PAD_Y + ITEM_H * 2.0 + SEP_H;
    assert_eq!(m.hit(10.0, third_top + 1.0), Some(2));
    // Outside the menu width does not hit.
    let (w, _) = m.logical_size();
    assert_eq!(m.hit(w + 5.0, third_top + 1.0), None);
    // Below the bottom does not hit either.
    assert_eq!(m.hit(10.0, 1000.0), None);
}

/// The menu height must equal the sum of the entries plus separators.
#[test]
fn menu_height_covers_all_entries() {
    let m = Menu {
        entries: vec![
            MenuEntry::new("打开", MenuCmd::Open, false, false),
            MenuEntry::new("退出应用", MenuCmd::QuitApp, false, true),
            MenuEntry::new("移除", MenuCmd::ExitDock, true, false),
        ],
        hover: None,
    };
    let (_, h) = m.logical_size();
    let expect = PAD_Y * 2.0 + ITEM_H * 3.0 + SEP_H;
    assert!((h - expect).abs() < 0.01, "实际 {h}");
}

/// The menu flips up when there is no room below.
#[test]
fn place_flips_up_near_bottom_edge() {
    let size = (200.0, 240.0);
    let bounds = (0.0, 0.0, 2560.0, 1600.0);
    let (x, y) = Menu::place((1200.0, 1500.0), size, bounds);
    assert_eq!(x, 1200.0);
    assert!((y - (1500.0 - 240.0)).abs() < 0.01, "应当翻到光标上方");
}

/// The menu is pulled in at the right screen edge.
#[test]
fn place_clamps_to_right_edge() {
    let size = (200.0, 100.0);
    let bounds = (0.0, 0.0, 2560.0, 1600.0);
    let (x, _) = Menu::place((2500.0, 100.0), size, bounds);
    assert!((x - (2560.0 - 200.0)).abs() < 0.01);
}

/// The menu sticks to the work-area top when it fits neither way.
#[test]
fn place_clamps_to_top_edge() {
    let size = (200.0, 1200.0);
    let bounds = (0.0, 60.0, 2560.0, 1600.0);
    // 1200 + 1200 > 1600, flips up, 1200-1200 = 0 < 60, so clamp to 60.
    let (_, y) = Menu::place((500.0, 1200.0), size, bounds);
    assert!((y - 60.0).abs() < 0.01, "实际 {y}");
}

/// The menu keeps its position when it fits.
#[test]
fn place_keeps_position_when_it_fits() {
    let size = (200.0, 300.0);
    let bounds = (0.0, 60.0, 2560.0, 1600.0);
    let (x, y) = Menu::place((500.0, 700.0), size, bounds);
    assert_eq!((x, y), (500.0, 700.0));
}

/// CJK labels are wider than Latin ones, or long items get clipped.
#[test]
fn chinese_labels_are_wider_than_latin() {
    assert!(estimate_width("在资源管理器中打开") > estimate_width("open in explorer"));
    let m = Menu::build(None, false, false, MenuOpts::default()).expect("应当有菜单");
    let (w, _) = m.logical_size();
    assert!(w >= MIN_W, "宽度不小于最小值，实际 {w}");
}

/// A hover change must be reported, the run loop relies on it.
#[test]
fn set_hover_reports_change() {
    let mut m = Menu::build(None, false, false, MenuOpts::default()).expect("应当有菜单");
    assert!(m.set_hover(Some(0)));
    assert!(!m.set_hover(Some(0)), "同一个下标不算变化");
    assert!(m.set_hover(None));
}

/// An out-of-range index returns `None` instead of panicking.
#[test]
fn cmd_at_is_bounds_checked() {
    // Minimal menu, so the indices are stable.
    let opts = MenuOpts {
        autostart: None,
        add_items: false,
        notes: false,
        windows: Vec::new(),
    };
    let m = Menu::build(None, false, false, opts.clone()).expect("应当有菜单");
    assert_eq!(m.cmd_at(0), Some(MenuCmd::ExitDock));
    assert_eq!(m.cmd_at(9), None);
}
