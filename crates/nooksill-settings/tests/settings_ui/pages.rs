//! Per-page rendering and config writes: the control exists, a click lands.

use egui_kittest::kittest::Queryable;
use nooksill_core::config::{BgMode, Key};
use nooksill_settings::{Command, Page};

use super::common::new_harness;

#[test]
fn 点击开关会写入配置() {
    let mut h = new_harness();
    assert!(!h.state().cfg.lock, "默认不锁定");

    h.get_by_label("锁定 Dock 布局").click();
    h.run();

    assert!(h.state().cfg.lock, "点击后应当锁定");
}

#[test]
fn 页面切换后能看到该页控件() {
    let mut h = new_harness();
    // Only the appearance page has this slider, so it proves the switch.
    assert_eq!(h.query_all_by_label("图标大小").count(), 0);

    // Appearance is now the first tab of the Dock group.
    h.get_by_role_and_label(egui::Role::Tab, "Dock 栏").click();
    h.run();
    assert_eq!(h.state().ui.page, Page::Appearance);
    // Both the slider and its label match, hence the `all` query.
    assert!(
        h.query_all_by_label("图标大小").count() >= 1,
        "外观页应出现尺寸滑杆"
    );
}

#[test]
fn 磨砂开关映射到背景材质() {
    let mut h = new_harness();
    h.get_by_label("性能").click();
    h.run();
    assert_eq!(h.state().cfg.bg_mode, BgMode::Frost);

    h.get_by_label("磨砂玻璃背景").click();
    h.run();

    // Off means gradient, not a separate boolean key: the switch is derived
    // from `bg_mode`, which a refactor could easily break.
    assert_eq!(h.state().cfg.bg_mode, BgMode::Gradient);
}

#[test]
fn 重置按钮产出命令而不是直接改配置() {
    let mut h = new_harness();
    h.get_by_role_and_label(egui::Role::Tab, "内容与图标库")
        .click();
    h.run();
    h.get_by_role_and_label(egui::Role::Tab, "添加").click();
    h.run();

    h.get_by_label("恢复默认设置").click();
    h.run();

    assert_eq!(h.state().commands.last(), Some(&Command::ResetDefaults));
    // No side effects: the config changes only after the host runs the command.
    assert!(!h.state().cfg.lock);
}

#[test]
fn 每一页都能渲染且不崩溃() {
    let mut h = new_harness();
    for page in Page::ALL {
        h.state_mut().ui.page = page;
        h.run();
    }
}

/// Both note entry points: the add page adds an item, extras opens the panel.
#[test]
fn 速记入口产出命令() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::Add;
    h.run();
    h.get_by_label("添加速记").click();
    h.run();
    assert!(
        h.state()
            .commands
            .iter()
            .any(|c| matches!(c, Command::AddNote)),
        "应当产出 AddNote"
    );

    h.state_mut().ui.page = Page::Extras;
    h.run();
    h.get_by_label("打开速记…").click();
    h.run();
    assert!(
        h.state()
            .commands
            .iter()
            .any(|c| matches!(c, Command::OpenNotes)),
        "应当产出 OpenNotes"
    );
}

/// All widget sliders render on the appearance page, using the shared card.
#[test]
fn 外观页小组件参数四条滑条() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::Appearance;
    h.run();
    for label in [
        "卡片不透明度",
        "卡片圆角",
        "条带成员间距",
        "弹出层模糊半径",
        "弹出面板不透明度",
        "弹出面板叠加层",
    ] {
        assert_eq!(h.query_all_by_label(label).count(), 1, "缺滑条：{label}");
    }
    assert_eq!(
        h.state().cfg.get_i32(Key::WIDGET_OPACITY),
        0,
        "默认跟随栏体"
    );
    assert_eq!(h.state().cfg.get_i32(Key::WIDGET_RADIUS), 0, "默认跟随栏体");
    assert_eq!(
        h.state().cfg.get_i32(Key::WIDGET_GAP),
        6,
        "默认与旧硬编码一致"
    );
    assert_eq!(
        h.state().cfg.get_i32(Key::WIDGET_BLUR),
        0,
        "默认跟随全局 Blur"
    );
    assert_eq!(
        h.state().cfg.get_i32(Key::PANEL_OPACITY),
        0,
        "默认跟随毛玻璃"
    );
    assert_eq!(h.state().cfg.get_i32(Key::PANEL_OVERLAY), 28);
}

/// All four delay sliders render on the interaction page. Two scenarios, each
/// with a pair of same-named labels on different cards.
#[test]
fn 交互页显示隐藏灵敏度四条滑条() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::Interaction;
    h.run();

    // Each same-named label appears once per card.
    assert_eq!(h.query_all_by_label("移开光标后收起延迟").count(), 2);
    assert_eq!(h.query_all_by_label("隐藏时唤醒显示延迟").count(), 2);
    // Defaults: 450/180 normally, 900/400 while maximized.
    assert_eq!(h.state().cfg.get_i32(Key::HIDE_DELAY), 450);
    assert_eq!(h.state().cfg.get_i32(Key::SHOW_DELAY), 180);
    assert_eq!(h.state().cfg.get_i32(Key::HIDE_DELAY_MAX), 900);
    assert_eq!(h.state().cfg.get_i32(Key::SHOW_DELAY_MAX), 400);
}

#[test]
fn 热键输入框按配置初始化() {
    let mut h = new_harness();
    h.run();
    assert_eq!(
        h.state().ui.hotkey_input,
        h.state().cfg.hotkey_combo.display()
    );
    assert_eq!(h.state().ui.hotkey_input, "Ctrl+Alt+D");
    assert!(!h.state().ui.hotkey_error);
    assert_eq!(h.query_all_by_label("切换快捷键").count(), 1);
}
