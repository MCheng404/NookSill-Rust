use super::*;

/// Movement below 6 px does not start a drag.
#[test]
fn drag_needs_to_cross_the_threshold() {
    let mut dock = drag_dock();
    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let x = dock.geom.slot_l[0] + s / 2.0;

    assert!(dock.press_at(x, y, false));
    dock.drag_to(x + 3.0, y);
    assert!(dock.dragging().is_none(), "3 px 不该起拖");

    dock.drag_to(x + 20.0, y);
    assert!(dock.dragging().is_some(), "20 px 应当起拖");
}

/// Dropping on a group merges instead of reordering.
#[test]
fn dragging_onto_a_group_merges_instead_of_reordering() {
    let cfg = Config::defaults();
    let items: Vec<DockItem> = vec![
        {
            let it = Item::new(ItemKind::File, "记事本", "C:/a.exe");
            let mut d = DockItem::from_item(&it);
            d.cur = cfg.icon_size as f32;
            d
        },
        {
            let it = Item::new(ItemKind::Group, "工具", "");
            let mut d = DockItem::from_item(&it);
            d.cur = cfg.icon_size as f32;
            d
        },
    ];
    let mut dock = Dock::new(cfg, items, LRect::new(0, 0, 2560, 1400));
    dock.update(96, nooksill_core::layout::REF_FRAME_MS);

    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let x0 = dock.geom.slot_l[0] + s / 2.0;
    let x1 = dock.geom.slot_l[1] + s / 2.0;

    assert!(dock.press_at(x0, y, false));
    // Cross the threshold first, then stop on the group icon.
    dock.drag_to(x0 + 20.0, y);
    dock.drag_to(x1, y);
    assert_eq!(dock.merge_target(), Some(1), "应当认出下面是组");
    // No reorder while over the group.
    assert!(dock.take_pending_moves().is_empty(), "停在组上时不该重排");

    assert_eq!(dock.release_at(x1, y), None, "拖过不算点击");
    assert_eq!(dock.take_pending_merge(), Some((0, 1)));
    assert_eq!(dock.take_click(), None, "归组不是点击");
}

/// Reordering resumes after leaving the group.
#[test]
fn reorder_resumes_after_leaving_the_group() {
    let cfg = Config::defaults();
    let items: Vec<DockItem> = vec![
        {
            let it = Item::new(ItemKind::File, "记事本", "C:/a.exe");
            let mut d = DockItem::from_item(&it);
            d.cur = cfg.icon_size as f32;
            d
        },
        {
            let it = Item::new(ItemKind::Group, "工具", "");
            let mut d = DockItem::from_item(&it);
            d.cur = cfg.icon_size as f32;
            d
        },
    ];
    let mut dock = Dock::new(cfg, items, LRect::new(0, 0, 2560, 1400));
    dock.update(96, nooksill_core::layout::REF_FRAME_MS);

    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let x0 = dock.geom.slot_l[0] + s / 2.0;
    let x1 = dock.geom.slot_l[1] + s / 2.0;

    assert!(dock.press_at(x0, y, false));
    dock.drag_to(x0 + 20.0, y);
    dock.drag_to(x1, y);
    assert_eq!(dock.merge_target(), Some(1));
    // Move to the blank area right of the group.
    dock.drag_to(x1 + s, y);
    assert_eq!(dock.merge_target(), None, "离开组后不该再提示归组");
    assert!(!dock.take_pending_moves().is_empty(), "离开组后应当能重排");
}

/// Releasing outside the window requests removal.
#[test]
fn dragging_outside_the_window_requests_removal() {
    let mut dock = drag_dock();
    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let x0 = dock.geom.slot_l[0] + s / 2.0;

    assert!(dock.press_at(x0, y, false));
    dock.drag_to(x0 + 20.0, y);
    // Far above the window, beyond the 24 px tolerance.
    dock.drag_to(x0 + 20.0, -100.0);
    assert!(dock.drag_outside(), "应当认出拖出了栏体");
    assert!(dock.take_pending_moves().is_empty(), "拖出去时不该重排");

    assert_eq!(dock.release_at(x0 + 20.0, -100.0), None);
    assert_eq!(dock.take_pending_remove(), Some(0));
    assert_eq!(dock.take_pending_merge(), None);
    assert_eq!(dock.take_click(), None);
}

/// Dragging back into the bar cancels removal.
#[test]
fn dragging_back_into_the_bar_cancels_removal() {
    let mut dock = drag_dock();
    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let x0 = dock.geom.slot_l[0] + s / 2.0;

    assert!(dock.press_at(x0, y, false));
    dock.drag_to(x0 + 20.0, y);
    dock.drag_to(x0 + 20.0, -100.0);
    assert!(dock.drag_outside());

    // `drag_to` recomputes `drop_outside` every frame.
    dock.drag_to(x0 + 30.0, y);
    assert!(!dock.drag_outside(), "回来之后删除手势要作废");
    assert_eq!(dock.release_at(x0 + 30.0, y), None);
    assert_eq!(dock.take_pending_remove(), None, "不该删条目");
}

/// A dropped batch round-trips through take.
#[test]
fn dropped_paths_round_trip() {
    let mut dock = drag_dock();
    let batch = dock.take_dropped_paths();
    assert!(batch.paths.is_empty());
    assert_eq!(batch.index, None);
    assert_eq!(batch.group, None);
    dock.push_dropped_paths(vec!["C:/a.exe".to_owned(), "C:/d".to_owned()], 10.0, 40.0);
    let batch = dock.take_dropped_paths();
    assert_eq!(batch.paths.len(), 2);
    assert_eq!(batch.paths[0], "C:/a.exe");
    assert!(batch.index.is_some(), "应当记下插入下标");
    let batch = dock.take_dropped_paths();
    assert!(batch.paths.is_empty(), "取走之后不该还有残留");
    assert_eq!(batch.index, None, "落点也要一并清掉");
    assert_eq!(batch.group, None);
}

/// The drop index follows the cursor.
#[test]
fn drop_index_follows_the_cursor() {
    let dock = drag_dock();
    let y = dock.geom.bar_top + 10.0;
    let first = dock.drop_index_at(dock.geom.slot_l[0] + 2.0, y);
    let last = dock.drop_index_at(dock.geom.slot_l[2] + dock.metrics.s, y);
    assert!(last >= first, "光标往右不该让插入位置变靠前");
    assert!(last <= dock.items.len());
}

/// A batch keeps the anchor of its first message.
#[test]
fn a_drop_batch_keeps_its_first_anchor() {
    let mut dock = drag_dock();
    dock.push_dropped_paths(vec!["C:/a.exe".into()], 5.0, 40.0);
    let first = dock.dropped_at;
    dock.push_dropped_paths(vec!["C:/b.exe".into()], 900.0, 40.0);
    assert_eq!(dock.dropped_at, first, "落点不该被第二条消息覆盖");
    assert_eq!(dock.dropped_paths.len(), 2);
}

/// Dragging past the last icon moves the item to the end.
#[test]
fn dragging_right_moves_the_item_to_the_end() {
    let mut dock = drag_dock();
    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let x0 = dock.geom.slot_l[0] + s / 2.0;
    let last = *dock.geom.act_c.last().unwrap();

    assert!(dock.press_at(x0, y, false));
    dock.drag_to(x0 + 20.0, y);
    dock.drag_to(last + 20.0, y);

    let names: Vec<&str> = dock.items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["第1项", "第2项", "第0项"], "第 0 项应当被挪到最后");
    assert_eq!(dock.take_pending_moves(), vec![(0, 2)]);
    assert!(dock.take_order_changed());
}

/// A drag never produces a click.
#[test]
fn drag_does_not_count_as_a_click() {
    let mut dock = drag_dock();
    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let x0 = dock.geom.slot_l[0] + s / 2.0;

    assert!(dock.press_at(x0, y, false));
    dock.drag_to(x0 + 40.0, y);
    assert_eq!(dock.release_at(x0 + 40.0, y), None, "拖拽不该产出点击");
    assert_eq!(dock.take_click(), None);
}

/// A locked dock refuses to drag.
#[test]
fn locked_dock_refuses_to_drag() {
    let mut dock = drag_dock();
    dock.cfg.lock = true;
    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let x0 = dock.geom.slot_l[0] + s / 2.0;

    assert!(dock.press_at(x0, y, false));
    dock.drag_to(x0 + 40.0, y);
    assert!(dock.dragging().is_none(), "锁定状态不该起拖");
}

/// Sweeping across the bar reorders monotonically, never ping-ponging.
///
/// Each step is followed by `update`, which changes `act_c`.
#[test]
fn dragging_across_the_bar_never_ping_pongs() {
    let mut dock = drag_dock();
    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let x0 = dock.geom.act_c[0];
    let last = *dock.geom.act_c.last().unwrap();

    assert!(dock.press_at(x0, y, false));
    let mut orders: Vec<Vec<String>> = Vec::new();
    let mut x = x0;
    while x < last + s * 1.5 {
        // The step must exceed the drag threshold.
        x += 10.0;
        dock.mouse = (x, y);
        dock.hover = true;
        dock.drag_to(x, y);
        assert!(
            dock.dragging().is_some(),
            "x={x:.0} 时拖拽状态丢了: drag={:?} press={:?} sep={} lock={}",
            dock.drag,
            dock.press,
            dock.sep.is_some(),
            dock.cfg.lock
        );
        // Real frames run `update`, which can move `act_c` under `place_for`.
        dock.update(96, nooksill_core::layout::REF_FRAME_MS);
        let order: Vec<String> = dock.items.iter().map(|i| i.name.clone()).collect();
        if orders.last() != Some(&order) {
            orders.push(order);
        }
    }
    // Item 0 walks to the end without going back.
    let seen: Vec<Vec<&str>> = orders
        .iter()
        .map(|o| o.iter().map(String::as_str).collect())
        .collect();
    assert_eq!(
        seen,
        vec![
            vec!["第0项", "第1项", "第2项"],
            vec!["第1项", "第0项", "第2项"],
            vec!["第1项", "第2项", "第0项"],
        ],
        "顺序应当单调向右挪，出现回头就是抖动"
    );
}

/// Sweep right across the dock; returns the reorder queue.
fn sweep_right(dock: &mut Dock) -> Vec<(usize, usize)> {
    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let x0 = dock.geom.act_c[0];
    let last = *dock.geom.act_c.last().unwrap();
    assert!(dock.press_at(x0, y, false));
    let mut x = x0;
    while x < last + s {
        x += 12.0;
        dock.mouse = (x, y);
        dock.hover = true;
        dock.drag_to(x, y);
        dock.update(96, nooksill_core::layout::REF_FRAME_MS);
    }
    dock.take_pending_moves()
}

/// Pinned items cannot cross into the running-app segment.
///
/// Config has no order where a pinned item follows a temporary one.
#[test]
fn dragging_never_crosses_the_pinned_boundary() {
    let mut dock = dock_with_temps(2, 4);
    let moves = sweep_right(&mut dock);
    let names: Vec<&str> = dock.items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(
        names,
        ["第1项", "第0项", "第2项", "第3项"],
        "被拖的固定条目应当停在第 2 段之前"
    );
    assert_eq!(moves, vec![(0, 1)], "只该换一格");
    assert!(dock.dragging().is_some(), "拖动不该被换位打断");
}

/// Running apps can be dragged into the pinned zone.
///
/// `Dock` does not change `pinned`; the main loop calls `set_pinned`.
#[test]
fn running_items_can_be_dragged_into_the_pinned_zone() {
    let mut dock = dock_with_temps(2, 4);
    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let x3 = dock.geom.act_c[3];
    let x0 = dock.geom.act_c[0];

    assert!(dock.press_at(x3, y, false), "按在第 4 项上");
    // `place_for` counts centres left of the cursor, so aim left of icon 0.
    let mut x = x3;
    while x > x0 - 2.0 * s {
        x -= 12.0;
        dock.mouse = (x, y);
        dock.hover = true;
        dock.drag_to(x, y);
        dock.update(96, nooksill_core::layout::REF_FRAME_MS);
    }
    let names: Vec<&str> = dock.items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names[0], "第3项", "运行中应用应当能拖到最前面");
    assert!(dock.dragging().is_some(), "拖动不该被换位打断");
}

/// Running-only docks can still reorder.
#[test]
fn running_items_swap_among_themselves() {
    let mut dock = dock_with_temps(0, 3);
    sweep_right(&mut dock);
    let names: Vec<&str> = dock.items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["第1项", "第2项", "第0项"], "临时条目之间要能换位");
}

/// A press below the drag threshold still counts as held.
#[test]
fn press_state_covers_the_frames_before_the_drag_starts() {
    let mut dock = drag_dock();
    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let x = dock.geom.act_c[0];

    assert!(!dock.press_active());
    assert!(dock.press_at(x, y, false));
    assert!(dock.press_active(), "按下即算，不能等到越过阈值");
    dock.drag_to(x + 40.0, y);
    assert!(dock.press_active());
    assert_eq!(dock.release_at(x + 40.0, y), None);
    assert!(!dock.press_active(), "松手之后必须放行重建");
}

/// Replacing the list keeps the current icon size.
#[test]
fn replacing_items_keeps_the_current_icon_size() {
    let mut dock = drag_dock();
    let s = dock.metrics.s;
    dock.items[1].cur = s * 0.5;
    let keep = dock.items.clone();
    dock.replace_items(keep);
    assert_eq!(dock.items[1].cur, s * 0.5, "同路径的条目要沿用尺寸");

    let mut more = dock.items.clone();
    more.push(DockItem::from_item(&Item::new(
        ItemKind::File,
        "新",
        "C:/new.exe",
    )));
    dock.replace_items(more);
    assert_eq!(dock.items[1].cur, s * 0.5);
    assert_eq!(dock.items[3].cur, 0.0, "真正新出现的条目仍要入场动画");
}

/// A stale `pinned` count cannot break the clamp.
#[test]
fn a_stale_pinned_count_cannot_break_the_clamp() {
    let mut dock = dock_with_temps(3, 3);
    dock.replace_items(vec![DockItem::from_item(&Item::new(
        ItemKind::File,
        "只剩一项",
        "C:/only.exe",
    ))]);
    dock.update(96, nooksill_core::layout::REF_FRAME_MS);
    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let x = dock.geom.act_c[0];
    assert!(dock.press_at(x, y, false));
    dock.drag_to(x + 40.0, y);
    assert!(dock.take_pending_moves().is_empty(), "一项没有可换的位置");
}
