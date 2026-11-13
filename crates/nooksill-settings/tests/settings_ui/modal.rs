//! Update popup and the generic modal contract: pure UI, commands only.

use egui_kittest::kittest::Queryable;
use nooksill_core::config::Key;
use nooksill_settings::{Command, Modal, ModalAction, Page, SettingsState};

use super::common::new_harness;

fn offer(h: &mut egui_kittest::Harness<'static, super::common::St>) {
    h.state_mut().ui.update.available = true;
    h.state_mut().ui.update.version = "2.1.0".to_owned();
    h.state_mut().ui.update.notes = "Bug fixes".to_owned();
    h.state_mut().ui.update.prerelease = false;
    h.run();
}

#[test]
fn 有更新时默认弹出居中弹窗且稍后能关掉() {
    let mut h = new_harness();
    offer(&mut h);
    assert!(h.state().ui.modal.is_some(), "默认应弹出更新弹窗");
    assert!(h.state().ui.update_prompted);
    h.get_by_label("有可用更新");
    h.get_by_label("下载并校验");
    h.get_by_label("稍后");

    h.get_by_label("稍后").click();
    h.run();
    assert!(h.state().ui.modal.is_none(), "稍后应关闭弹窗");
    assert!(h.state().ui.update_prompted, "本窗口会话内不重复弹");
}

#[test]
fn 弹窗的主要动作只产出下载命令() {
    let mut h = new_harness();
    offer(&mut h);
    h.get_by_label("下载并校验").click();
    h.run();
    assert_eq!(h.state().commands.last(), Some(&Command::DownloadUpdate));
}

#[test]
fn 自动更新与关闭提示都不弹窗() {
    let mut h = new_harness();
    h.state_mut().cfg.apply_i32(Key::UPD_AUTO, 1);
    offer(&mut h);
    assert!(h.state().ui.modal.is_none(), "自动更新优先于弹窗");

    let mut h2 = new_harness();
    h2.state_mut().cfg.apply_i32(Key::UPD_PROMPT, 0);
    offer(&mut h2);
    assert!(h2.state().ui.modal.is_none(), "用户关掉提示就不弹");
}

#[test]
fn 不再提示写回配置而不是本地状态() {
    let mut h = new_harness();
    offer(&mut h);
    assert_eq!(h.state().cfg.get_i32(Key::UPD_PROMPT), 1);
    h.get_by_label("不再提示").click();
    h.run();
    assert_eq!(h.state().cfg.get_i32(Key::UPD_PROMPT), 0);
}

#[test]
fn 更新弹窗显示版本路径与通道徽标() {
    let mut h = new_harness();
    offer(&mut h);
    let modal = h.state().ui.modal.as_ref().expect("popup open");
    assert!(
        modal.body.contains(nooksill_core::VERSION),
        "应显示当前版本：{}",
        modal.body
    );
    assert!(modal.body.contains("2.1.0"), "应显示目标版本");
    assert_eq!(modal.badge, "稳定版", "通道要有可见标识");
}

#[test]
fn 点击弹窗内部不会误关() {
    let mut h = new_harness();
    offer(&mut h);
    // The veil covers the whole window; a click on the title is still inside
    // the panel and must not be treated as "click away".
    h.get_by_label("有可用更新").click();
    h.run();
    assert!(h.state().ui.modal.is_some(), "点标题不应关闭弹窗");
}

#[test]
fn 通用弹窗可承载任意标题与动作() {
    let mut h = new_harness();
    let mut modal = Modal::new("Custom title", "Custom body");
    modal.actions = vec![
        ModalAction::primary("Go", Command::CheckUpdate),
        ModalAction::close("Cancel"),
    ];
    h.state_mut().ui.modal = Some(modal);
    h.run();
    h.get_by_label("Custom title");
    h.get_by_label("Go").click();
    h.run();
    assert!(h.state().ui.modal.is_none());
    assert_eq!(h.state().commands.last(), Some(&Command::CheckUpdate));
    assert_eq!(h.state().ui.page, Page::General);
}

#[test]
fn 打开设置窗口重置提示机会() {
    let mut h = new_harness();
    offer(&mut h);
    h.get_by_label("稍后").click();
    h.run();
    assert!(h.state().ui.modal.is_none());

    // The host resets the flag each time it opens the window.
    h.state_mut().ui.update_prompted = false;
    h.run();
    assert!(h.state().ui.modal.is_some(), "重新打开应再提示一次");
}

#[test]
fn 默认状态字段不写进配置() {
    let state = SettingsState::default();
    assert!(state.sub.is_none());
    assert!(state.modal.is_none());
    assert!(!state.update_prompted);
    assert_eq!(state.return_page, Page::General);
}
