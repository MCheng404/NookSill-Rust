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
fn 资源监视器行可勾选与排序() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::Extras;
    h.run();
    assert_eq!(
        h.state().ui.page,
        Page::Extras,
        "page={:?}",
        h.state().ui.page
    );
    assert_eq!(h.state().cfg.res_order, "", "默认走内置顺序，不写 ini");

    h.get_by_role_and_label(egui::Role::Switch, "网络").click();
    h.run();
    let hidden = h.state().cfg.res_order.clone();
    assert!(
        !hidden.is_empty() && !hidden.split(',').any(|k| k == "net"),
        "hidden={hidden:?}"
    );

    // The second up-arrow belongs to the GPU row and swaps it with CPU.
    let ups: Vec<_> = h.query_all_by_label("↑").collect();
    assert!(ups.len() >= 2, "可见行都应有上移按钮");
    ups[1].click();
    h.run();
    let order = h.state().cfg.res_order.clone();
    assert_eq!(
        order.split(',').next(),
        Some("gpu"),
        "排序应写回配置: {order}"
    );
}

#[test]
fn 页面切换后能看到该页控件() {
    let mut h = new_harness();
    // Only the appearance page has this slider, so it proves the switch.
    assert_eq!(h.query_all_by_label("图标大小").count(), 0);

    // Appearance is the first tab of the Look group.
    h.get_by_role_and_label(egui::Role::Tab, "外观与质感")
        .click();
    h.run();
    assert_eq!(h.state().ui.page, Page::Appearance);
    // Both the slider and its label match, hence the `all` query.
    assert!(
        h.query_all_by_label("图标大小").count() >= 1,
        "外观页应出现尺寸滑杆"
    );
}

#[test]
fn 性能页不再重复磨砂开关() {
    let mut h = new_harness();
    // Performance now lives under the System group, not a primary row.
    h.state_mut().ui.page = Page::Performance;
    h.run();
    // The material is chosen once, on the appearance page; a second switch
    // here used to write the same `BgMode` key and could disagree with it.
    assert_eq!(h.query_all_by_label("磨砂玻璃背景").count(), 0);
    assert_eq!(h.state().cfg.bg_mode, BgMode::Frost);
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
fn 外观页参数集中在磨砂与卡片两张卡() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::Appearance;
    h.run();
    // One frost card owns material, blur, glow and the popup tint; the card
    // card only keeps the plate geometry. No control may appear twice.
    for label in [
        "卡片不透明度",
        "卡片圆角",
        "条带成员间距",
        "磨砂模糊强度",
        "磨砂柔光",
        "弹出面板不透明度",
        "弹出面板叠加层",
    ] {
        assert_eq!(h.query_all_by_label(label).count(), 1, "缺滑条：{label}");
    }
    assert_eq!(
        h.query_all_by_label("背景材质").count(),
        1,
        "材质下拉是唯一的磨砂入口"
    );
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

#[test]
fn 托盘组件开关写回配置() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::Extras;
    h.run();
    assert_eq!(h.state().cfg.get_i32(Key::TRAY_W), 1, "默认显示");

    h.get_by_label("系统托盘组件").click();
    h.run();

    assert_eq!(h.state().cfg.get_i32(Key::TRAY_W), 0, "点击后应当隐藏");
}

#[test]
fn 扩展页开关与命令写回() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::Extensions;
    h.run();

    assert_eq!(h.state().cfg.get_i32(Key::HOST_LINK_ENABLED), 1, "默认启用");
    h.get_by_label("启用扩展").click();
    h.run();
    assert_eq!(
        h.state().cfg.get_i32(Key::HOST_LINK_ENABLED),
        0,
        "点击后应关闭"
    );

    h.get_by_label("立即重新扫描").click();
    h.run();
    assert_eq!(h.state().commands.last(), Some(&Command::HostLinkRescan));
}

#[test]
fn 已支持扩展显示项目链接() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::Extensions;
    h.run();

    h.get_by_label("ModularClipboard").click();
    h.run();

    assert_eq!(
        h.state().commands.last(),
        Some(&Command::OpenUrl(
            "https://github.com/MCheng404/ModularClipboard"
        ))
    );
}

#[test]
fn 扩展页可添加到工具栏() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::Extensions;
    h.state_mut()
        .ui
        .extensions
        .push(nooksill_settings::ExtensionRow {
            id: "com.example.clip".to_owned(),
            name: "Clipboard".to_owned(),
            exe: r"C:\clip.exe".to_owned(),
            ready: true,
            detail: "ready".to_owned(),
        });
    h.run();

    h.get_by_label("添加到 Dock 栏").click();
    h.run();

    assert!(matches!(
        h.state().commands.last(),
        Some(Command::HostLinkAddToDock { .. })
    ));
}

#[test]
fn 插件列表可重新扫描() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::Extensions;
    h.run();

    h.get_by_label("重新扫描插件").click();
    h.run();

    assert_eq!(h.state().commands.last(), Some(&Command::PluginRescan));
}
