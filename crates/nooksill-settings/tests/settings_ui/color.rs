//! Custom colour rows: self-drawn switch, swatch and preset palette.

use egui_kittest::kittest::Queryable;
use nooksill_core::config::Key;
use nooksill_settings::Page;

use super::common::new_harness;

#[test]
fn 自绘开关启用自定义颜色() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::Appearance;
    h.run();
    assert_eq!(h.state().cfg.get_i32(Key::BgColor1), -1);

    h.get_by_role_and_label(egui::Role::Switch, "主题色")
        .click();
    h.run();
    assert!(h.state().cfg.get_i32(Key::BgColor1) >= 0);
}

#[test]
fn 色块展开预设色板并写回配置() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::Appearance;
    h.run();
    h.get_by_role_and_label(egui::Role::Switch, "主题色")
        .click();
    h.run();

    // The swatch reports its own hex as the accessible name.
    h.get_by_role_and_label(egui::Role::Button, "#996633")
        .click();
    h.run();
    assert_eq!(h.state().ui.color_edit, Some(Key::BgColor1));

    // First Windows preset is #0078D4; COLORREF stores it as BGR.
    h.get_by_role_and_label(egui::Role::Button, "#0078D4")
        .click();
    h.run();
    assert_eq!(h.state().cfg.get_i32(Key::BgColor1) as u32, 0x00D4_7800);
    assert!(h.state().ui.color_edit.is_none());
}
