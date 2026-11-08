//! Layout contract and navigation behaviour: geometry, hints, filtering,
//! control alignment.

use egui_kittest::kittest::Queryable;
use nooksill_settings::{Command, Page};

use super::common::{harness, new_harness};

// Layout contract: these assert where things are drawn, not config values.
// Screenshot regression needs wgpu, which this layer deliberately avoids, so
// rect assertions are the only guard for "nav left, content right, no overlap".

/// Navigation items are full-width and always left of the content area.
#[test]
fn 导航栏在左且整行可点() {
    let mut h = new_harness();
    h.get_by_label("外观").click();
    h.run();
    assert_eq!(h.state().ui.page, Page::Appearance);

    // Query by role: the page heading carries the same label, so a text
    // query would match two nodes.
    let nav = h.get_by_role_and_label(egui::Role::Tab, "外观").rect();
    // The slider itself (`Role::Slider`); a text query also hits the title.
    let content = h
        .get_by_role_and_label(egui::Role::Slider, "图标大小")
        .rect();
    assert!(
        nav.right() <= content.left(),
        "导航项与内容区重叠：nav.right={} content.left={}",
        nav.right(),
        content.left()
    );
    assert!(
        nav.width() >= 140.0,
        "导航项应撑满整行（文字宽度只有几十像素），实得 {}",
        nav.width()
    );
}

/// With a title bar already drawn by the shell, the columns must not cover it.
#[test]
fn 有标题栏时两列不会盖住标题() {
    let mut h = harness(true);
    h.run();

    let title = h.get_by_label("NookSill").rect();
    let nav = h.get_by_role_and_label(egui::Role::Tab, "常规").rect();
    assert!(
        nav.top() >= title.bottom() - 1.0,
        "导航栏盖住了标题：nav.top={} title.bottom={}",
        nav.top(),
        title.bottom()
    );
}

/// Row notes take no layout space: they live in the dot after the title and
/// expand into a floating bubble on click.
#[test]
fn 描述收进感叹号气泡() {
    let mut h = new_harness();
    h.run();

    // The note is not laid out by default; it is only bubble content.
    assert_eq!(
        h.query_all_by_label("防止误拖动、误移除图标").count(),
        0,
        "说明文字不该再排版出来"
    );

    // The dot exists and its accessible name starts with the hint prefix.
    let hint = h.get_by_role_and_label(egui::Role::Button, "说明：防止误拖动、误移除图标");
    hint.click();
    h.run();

    // After the click the bubble is open and the note is visible.
    assert!(
        h.query_all_by_label("防止误拖动、误移除图标").count() >= 1,
        "点击感叹号后应展开说明气泡"
    );
}

/// Clicking outside closes the bubble, pinned or not.
#[test]
fn 点击气泡外收起() {
    let mut h = new_harness();
    h.run();

    h.get_by_role_and_label(egui::Role::Button, "说明：防止误拖动、误移除图标")
        .click();
    h.run();
    assert!(
        h.query_all_by_label("防止误拖动、误移除图标").count() >= 1,
        "前置：气泡应已展开"
    );

    // Click an unrelated spot (the page heading); the click still lands.
    h.get_by_label("外观").click();
    h.run();

    assert_eq!(
        h.query_all_by_label("防止误拖动、误移除图标").count(),
        0,
        "点气泡外应收起"
    );
}

/// Clicking an item above the current one must land on that page.
///
/// Regression: switching mutates `state.page` mid-loop, so the "is the old page
/// visible" check followed iteration order and the auto-jump reset the page.
/// Only upward switches triggered it.
#[test]
fn 点击上一项不会跳回第一页() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::Effect; // item 7
    h.run();

    h.get_by_role_and_label(egui::Role::Tab, "外观").click(); // item 2
    h.run();

    assert_eq!(
        h.state().ui.page,
        Page::Appearance,
        "点当前项之上的项应停在所点页，而不是跳回常规"
    );
}

/// With a filter active, a click on a non-first match must beat the auto-jump.
#[test]
fn 过滤态点击优先于自动跳页() {
    let mut h = new_harness();
    h.state_mut().ui.nav_filter = "图".to_owned(); // matches Icons and Library
    h.run();
    assert_eq!(h.state().ui.page, Page::Icons); // General filtered out, so it jumps

    h.get_by_role_and_label(egui::Role::Tab, "图标库").click();
    h.run();
    assert_eq!(
        h.state().ui.page,
        Page::Library,
        "点击的页不能被自动跳页盖掉"
    );
}

/// Search shows only matching labels, and a filtered-out current page jumps to
/// the first match.
#[test]
fn 搜索过滤导航并自动切页() {
    let mut h = new_harness();
    h.state_mut().ui.nav_filter = "图".to_owned();
    h.run();

    assert_eq!(
        h.state().ui.page,
        Page::Icons,
        "常规页不含'图'，应自动跳到图标页"
    );
    assert_eq!(h.state().ui.nav_filter, "图", "过滤词要一直留在输入框里");

    // Clearing the filter does not move back; the page stays on Icons.
    h.state_mut().ui.nav_filter = String::new();
    h.run();
    assert_eq!(h.state().ui.page, Page::Icons);
}

/// Autostart uses the same pill switch (`Role::Switch`) as config keys, and
/// toggling it only queues a command: the registry is written by the host.
#[test]
fn 自启动开关与配置开关统一() {
    let mut h = new_harness(); // opens on the general page
    h.run();
    let before = h.state().ui.auto_start;
    let (right, other_right) = {
        let a = h
            .get_by_role_and_label(egui::Role::Switch, "开机自启动")
            .rect();
        let b = h
            .get_by_role_and_label(egui::Role::Switch, "锁定 Dock 布局")
            .rect();
        (a.right(), b.right())
    };
    assert!(
        (right - other_right).abs() < 1.0,
        "自启动开关右边缘 {right} 应与其它开关 {other_right} 对齐"
    );

    h.get_by_role_and_label(egui::Role::Switch, "开机自启动")
        .click();
    h.run();
    assert_eq!(h.state().ui.auto_start, !before);
    assert_eq!(
        h.state().commands.last(),
        Some(&Command::ToggleAutostart(!before))
    );
}

/// Switches and combos must share a right edge; the widget card on the extras
/// page holds both and is where a mismatch shows up.
#[test]
fn 控件右边缘对齐() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::Extras;
    h.run();

    let toggle = h.get_by_role_and_label(egui::Role::Switch, "通知").rect();
    let combo = h
        .get_by_role_and_label(egui::Role::ComboBox, "小组件位置")
        .rect();
    assert!(
        (toggle.right() - combo.right()).abs() < 1.0,
        "开关右边缘 {} 与下拉右边缘 {} 没对齐",
        toggle.right(),
        combo.right()
    );
}
