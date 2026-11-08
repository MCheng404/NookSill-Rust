//! Items page: list, edit form and reorder buttons emit commands only.

use egui_kittest::kittest::Queryable;
use nooksill_settings::{Command, Page};

use super::common::new_harness;

/// The items page lists items and group children.
///
/// The render-every-page test exercises the empty state; a populated list takes
/// a different path that also builds the edit form and the group combo.
#[test]
fn 条目页列出条目与分组子项() {
    use nooksill_core::item::{Item, ItemKind};

    let mut h = new_harness();
    let mut group = Item::new(ItemKind::Group, "开发工具", "");
    group
        .kids
        .push(Item::new(ItemKind::File, "VS Code", r"C:\Code.exe"));
    group
        .kids
        .push(Item::new(ItemKind::Url, "GitHub", "https://github.com"));
    h.state_mut().ui.items = vec![
        Item::new(ItemKind::File, "记事本", r"C:\Windows\notepad.exe"),
        group,
        Item::new(ItemKind::Separator, "", ""),
    ];
    h.state_mut().ui.page = Page::Items;
    h.run();

    assert!(
        h.query_all_by_label("记事本").count() >= 1,
        "普通条目应当列出来"
    );
    assert!(
        h.query_all_by_label("开发工具").count() >= 1,
        "分组应当列出来"
    );
    assert!(
        h.query_all_by_label("· VS Code").count() >= 1,
        "分组里的子项应当列出来"
    );
    assert!(h.query_all_by_label("解散").count() >= 1, "组要有解散入口");
}

/// Delete queues a command carrying the index; the UI never mutates items.
#[test]
fn 条目页的删除只发命令() {
    use nooksill_core::item::{Item, ItemKind};

    let mut h = new_harness();
    // One item only: `get_by_label` needs a unique match, two rows mean two
    // delete buttons.
    h.state_mut().ui.items = vec![Item::new(
        ItemKind::File,
        "记事本",
        r"C:\Windows\notepad.exe",
    )];
    h.state_mut().ui.page = Page::Items;
    h.run();

    h.get_by_label("删除").click();
    h.run();

    let cmds = &h.state().commands;
    assert!(
        cmds.iter().any(|c| matches!(c, Command::RemoveItem(0))),
        "应当产出 RemoveItem(0) 命令：{cmds:?}"
    );
    assert_eq!(h.state().ui.items.len(), 1, "UI 层不能自己删条目");
}

/// New group queues a command instead of pushing into the snapshot.
#[test]
fn 条目页新建分组只发命令() {
    let mut h = new_harness();
    h.state_mut().ui.page = Page::Items;
    h.run();

    h.get_by_label("新建分组…").click();
    h.run();

    assert!(
        h.state()
            .commands
            .iter()
            .any(|c| matches!(c, Command::NewGroup)),
        "应当产出 NewGroup 命令"
    );
}

/// Custom icon entry points in the edit form queue commands only.
#[test]
fn 条目页更换与恢复图标只发命令() {
    use nooksill_core::item::{Item, ItemKind};

    let mut h = new_harness();
    let mut item = Item::new(ItemKind::File, "记事本", r"C:\Windows\notepad.exe");
    item.icon_src = r"C:\custom.ico".to_owned();
    h.state_mut().ui.items = vec![item];
    h.state_mut().ui.page = Page::Items;
    h.run();

    h.get_by_label("编辑").click();
    h.run();
    h.get_by_label("更换…").click();
    h.run();
    assert!(
        h.state()
            .commands
            .iter()
            .any(|c| matches!(c, Command::PickIcon(0))),
        "应当产出 PickIcon(0)"
    );

    h.get_by_label("恢复默认").click();
    h.run();
    assert!(
        h.state()
            .commands
            .iter()
            .any(|c| matches!(c, Command::ResetIcon(0))),
        "应当产出 ResetIcon(0)"
    );
    assert_eq!(
        h.state().ui.items[0].icon_src,
        r"C:\custom.ico",
        "UI 层不能自己改数据"
    );
}

/// Reordering a group child carries both the group and child indices.
#[test]
fn 条目页分组子项排序只发命令() {
    use nooksill_core::item::{Item, ItemKind};

    let mut h = new_harness();
    let mut group = Item::new(ItemKind::Group, "工具", "");
    group.kids.push(Item::new(ItemKind::File, "A", r"C:\a.exe"));
    group.kids.push(Item::new(ItemKind::File, "B", r"C:\b.exe"));
    h.state_mut().ui.items = vec![group];
    h.state_mut().ui.page = Page::Items;
    h.run();

    // With two children only the first has a down button and only the second
    // an up button.
    h.get_by_label("下移").click();
    h.run();
    assert!(
        h.state()
            .commands
            .iter()
            .any(|c| matches!(c, Command::MoveKid(0, 0, 1))),
        "应当产出 MoveKid(0, 0, 1)"
    );

    h.get_by_label("上移").click();
    h.run();
    assert!(
        h.state()
            .commands
            .iter()
            .any(|c| matches!(c, Command::MoveKid(0, 1, -1))),
        "应当产出 MoveKid(0, 1, -1)"
    );
    assert_eq!(h.state().ui.items[0].kids[0].name, "A", "UI 层不能自己排序");
}
