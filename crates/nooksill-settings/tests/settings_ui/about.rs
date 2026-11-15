//! About page: the version row is display-only and hides nothing.

use egui_kittest::kittest::Queryable;
use nooksill_settings::Page;

use super::common::new_harness;

#[test]
fn 关于页没有隐藏的高级入口() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::About;
    h.run();
    assert_eq!(h.query_all_by_label("版本").count(), 1);
    assert!(h.query_by_label("使用安全的 DNS").is_none());
    assert!(h.query_by_label("实验性：轻量实时磨砂").is_none());
}
