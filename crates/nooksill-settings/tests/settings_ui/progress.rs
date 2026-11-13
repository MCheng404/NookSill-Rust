//! Download progress: pure state helpers and the card that renders them.

use egui_kittest::kittest::Queryable;
use nooksill_settings::{Page, UpdateView};

use super::common::new_harness;

#[test]
fn 进度只在下载中且已知总大小时可用() {
    let mut view = UpdateView::default();
    assert_eq!(view.progress(), None);
    view.bytes_total = 100;
    assert_eq!(view.progress(), None, "非下载态不显示进度");
    view.busy = true;
    view.bytes_done = 25;
    assert_eq!(view.progress(), Some(0.25));
    view.bytes_done = 500;
    assert_eq!(view.progress(), Some(1.0), "超出总大小要夹紧");
    view.bytes_total = 0;
    assert_eq!(view.progress(), None, "未知大小不显示进度");
}

#[test]
fn 更新策略页在下载中渲染进度与字节数() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::UpdatePolicy;
    h.state_mut().ui.update.busy = true;
    h.state_mut().ui.update.status = "正在下载…".to_owned();
    h.state_mut().ui.update.bytes_done = 18 * 1024 * 1024;
    h.state_mut().ui.update.bytes_total = 32 * 1024 * 1024;
    h.run();
    assert_eq!(h.state().ui.update.progress(), Some(0.5625));
    h.get_by_label("18.0 / 32.0 MB");
}

#[test]
fn 下载中显示速度与剩余时间() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::UpdatePolicy;
    h.state_mut().ui.update.busy = true;
    h.state_mut().ui.update.status = "正在下载…".to_owned();
    h.state_mut().ui.update.bytes_done = 18 * 1024 * 1024;
    h.state_mut().ui.update.bytes_total = 32 * 1024 * 1024;
    h.state_mut().ui.update.bytes_per_sec = 2_400_000;
    h.state_mut().ui.update.eta_secs = 6;
    h.run();
    h.get_by_label("18.0 / 32.0 MB · 2.3 MB/s · 剩余 6 秒");
}

#[test]
fn 只有速度没有剩余时间时省略后半段() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::UpdatePolicy;
    h.state_mut().ui.update.busy = true;
    h.state_mut().ui.update.bytes_done = 1024 * 1024;
    h.state_mut().ui.update.bytes_total = 4 * 1024 * 1024;
    h.state_mut().ui.update.bytes_per_sec = 1024 * 1024;
    h.run();
    h.get_by_label("1.0 / 4.0 MB · 1.0 MB/s");
}
