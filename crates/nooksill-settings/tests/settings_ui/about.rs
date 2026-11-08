//! About page: version-click unlock and the experimental switches behind it.

use egui_kittest::kittest::Queryable;
use nooksill_core::config::Key;
use nooksill_settings::Page;

use super::common::new_harness;

/// Hidden entry: five version clicks unlock the advanced card, four do not.
#[test]
fn 连点版本号五次才解锁高级入口() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::About;
    h.run();

    for _ in 0..4 {
        h.get_by_label("版本").click();
        h.run();
    }
    assert!(!h.state().ui.advanced, "四次还不该解锁");
    assert!(
        h.query_by_label("使用安全的 DNS").is_none(),
        "没解锁就不该有高级项"
    );

    h.get_by_label("版本").click();
    h.run();
    assert!(h.state().ui.advanced, "第五次点击解锁");

    // Unlocked, the card shows its defaults: secure DNS on, mirror-all off.
    h.get_by_label("使用安全的 DNS");
    h.get_by_label("镜像全部测速后再选");
    assert_eq!(
        h.state().cfg.get_i32(Key::NET_SECURE_DNS),
        1,
        "安全 DNS 默认开"
    );
    assert_eq!(
        h.state().cfg.get_i32(Key::NET_MIRROR_ALL),
        0,
        "默认够快就先下"
    );
}

/// The experimental blur switch is hidden until unlock, defaults off, and
/// writes to the config on click.
#[test]
fn 实验性模糊开关藏在高级入口且能切换() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::About;
    h.run();
    assert!(
        h.query_by_label("实验性：轻量实时磨砂").is_none(),
        "没解锁时不该出现在关于页"
    );

    for _ in 0..5 {
        h.get_by_label("版本").click();
        h.run();
    }
    assert_eq!(
        h.state().cfg.get_i32(Key::FROST_ILLUSION),
        0,
        "默认走原高斯"
    );
    h.get_by_label("实验性：轻量实时磨砂").click();
    h.run();
    assert_eq!(
        h.state().cfg.get_i32(Key::FROST_ILLUSION),
        1,
        "开关必须落到 Config，主循环才能同步给渲染层"
    );
}
