//! Performance page: adapter config, the adapter in use, fallback and
//! restart-pending notes.

use egui_kittest::kittest::Queryable;
use nooksill_settings::Page;

use super::common::new_harness;

#[test]
fn 显卡默认自动且可切换到具体适配器() {
    let mut h = new_harness();
    h.state_mut().ui.adapters = vec![
        "AMD Radeon(TM) 610M".to_owned(),
        "NVIDIA GeForce RTX 5070 Laptop GPU".to_owned(),
    ];
    h.state_mut().ui.active_adapter = "AMD Radeon(TM) 610M".to_owned();
    h.state_mut().ui.page = Page::Performance;
    h.run();

    assert_eq!(h.state().cfg.adapter, "", "默认必须是自动");
    // The adapter actually in use must be shown, not just the configured one.
    // Its label is a whole sentence, so a bare name will not match.
    assert!(
        h.query_all_by_label("当前实际使用：AMD Radeon(TM) 610M")
            .count()
            >= 1,
        "应显示当前实际使用的适配器"
    );

    h.get_by_label("显卡").click();
    h.run();
    h.get_by_label("NVIDIA GeForce RTX 5070 Laptop GPU").click();
    h.run();

    assert_eq!(h.state().cfg.adapter, "NVIDIA GeForce RTX 5070 Laptop GPU");
}

#[test]
fn 显卡可以切回自动() {
    let mut h = new_harness();
    h.state_mut().ui.adapters = vec!["AMD Radeon(TM) 610M".to_owned()];
    h.state_mut().cfg.adapter = "AMD Radeon(TM) 610M".to_owned();
    h.state_mut().ui.page = Page::Performance;
    h.run();

    h.get_by_label("显卡").click();
    h.run();
    h.get_by_label("自动（推荐）").click();
    h.run();

    assert_eq!(h.state().cfg.adapter, "", "切回自动要落成空串");
}

#[test]
fn 适配器回退时给出提示() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::Performance;
    h.state_mut().ui.adapter_note = "配置的「X」未找到，已回退自动".to_owned();
    h.run();

    assert!(
        h.query_all_by_label("配置的「X」未找到，已回退自动")
            .count()
            >= 1,
        "回退必须可见，否则用户会以为选择生效了"
    );
}

/// The adapter only takes effect on the next launch, so a selection must leave
/// a visible note: otherwise the unchanged in-use line reads as no response.
#[test]
fn 显卡选择后提示重启生效() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::Performance;
    h.state_mut().ui.active_adapter = "AMD Radeon(TM) 610M".to_owned();
    h.state_mut().ui.adapter_note = "已选择「NVIDIA RTX 5070」，重启后生效".to_owned();
    h.run();

    assert!(
        h.query_all_by_label("当前实际使用：AMD Radeon(TM) 610M")
            .count()
            >= 1,
        "实际在用的卡仍是启动时那块"
    );
    assert!(
        h.query_all_by_label("已选择「NVIDIA RTX 5070」，重启后生效")
            .count()
            >= 1,
        "待生效的选择必须可见"
    );
}
