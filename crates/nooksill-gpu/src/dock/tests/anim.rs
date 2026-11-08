use super::*;

/// The entrance animation converges to the base size.
#[test]
fn entrance_animation_converges_to_base_size() {
    let cfg = Config::defaults();
    let items: Vec<DockItem> = (0..3)
        .map(|i| {
            DockItem::from_item(&Item::new(
                ItemKind::File,
                format!("第{i}项"),
                format!("C:/x{i}.exe"),
            ))
        })
        .collect();
    let mut dock = Dock::new(cfg, items, LRect::new(0, 0, 2560, 1400));

    // Take the target from config; at 96 dpi `calc_metrics` does not scale.
    let want = dock.cfg.icon_size as f32;
    assert!(want > 0.0, "基准尺寸应当为正");

    let mut frames = 0;
    loop {
        dock.update(96, nooksill_core::layout::REF_FRAME_MS);
        frames += 1;
        let done = dock.items.iter().all(|i| (i.cur - want).abs() < 0.01);
        if done || frames > 200 {
            break;
        }
    }
    assert!(frames < 100, "{frames} 帧还没收敛到 {want}");
    for it in &dock.items {
        assert!(
            (it.cur - want).abs() < 0.01,
            "cur={} 没收敛到 {want}（跑了 {frames} 帧）",
            it.cur
        );
    }
}

/// A click is settled before capture is released, and survives
/// `cancel_drag`. `ReleaseCapture` sends `WM_CAPTURECHANGED` synchronously.
#[test]
fn click_survives_release_capture() {
    let mut dock = settled_dock(2);
    let (u, v) = item_center(&dock, 0);

    assert!(dock.press_at(u, v, false), "按下应当命中第一项");
    // Same order as the fixed `WM_LBUTTONUP`: settle, then release.
    assert_eq!(dock.release_at(u, v), Some(0), "抬起应当判成点击");
    dock.cancel_drag(); // What `ReleaseCapture` triggers.

    assert_eq!(
        dock.take_click().map(|c| c.0),
        Some(0),
        "交还捕获不该把已经结算好的点击抹掉"
    );
}

/// A real capture loss drops the click.
#[test]
fn lost_capture_drops_the_click() {
    let mut dock = settled_dock(2);
    let (u, v) = item_center(&dock, 0);
    assert!(dock.press_at(u, v, false));
    dock.cancel_drag();
    assert_eq!(dock.release_at(u, v), None, "丢了捕获就不该再触发点击");
    assert!(dock.take_click().is_none());
}

/// An external drop reserves a slot and releases it.
#[test]
fn external_drop_reserves_and_releases_an_insert_slot() {
    let mut dock = settled_dock(3);
    let (u, v) = item_center(&dock, 1);
    assert_eq!(dock.external_drop(), None);

    dock.set_external_drop(u, v);
    assert!(dock.external_drop().is_some(), "应当记下插入位");
    dock.update(96, 16.0);
    assert!(dock.geom.insert_l.is_some(), "布局必须画出插入标记位置");

    dock.clear_external_drop();
    assert_eq!(dock.external_drop(), None);
    dock.update(96, 16.0);
    assert!(dock.geom.insert_l.is_none(), "离开后不能残留空槽");
}

/// Middle click forces a click and clears the press state.
#[test]
fn middle_click_forces_and_clears_press() {
    let mut dock = settled_dock(1);
    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let c0 = dock.geom.slot_l[0] + s / 2.0;

    assert_eq!(dock.force_click(c0, y), Some(0));
    assert_eq!(dock.take_click(), Some((0, true)));
    // The press is cleared: a later release produces nothing.
    assert_eq!(dock.release_at(c0, y), None);
    assert_eq!(dock.take_click(), None);

    // Middle click on empty space produces nothing.
    assert_eq!(dock.force_click(dock.geom.bar_l - 40.0, y), None);
    assert_eq!(dock.take_click(), None);
}

/// Capture loss cancels the drag without a click.
#[test]
fn capture_loss_cancels_without_clicking() {
    let mut dock = settled_dock(2);
    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let c0 = dock.geom.slot_l[0] + s / 2.0;

    assert!(dock.press_at(c0, y, false));
    dock.drag_to(c0 + 40.0, y); // Enter the drag.
    dock.cancel_drag();
    assert!(dock.dragging().is_none(), "拖拽应当被取消");
    // After cancel, a release produces no click.
    assert_eq!(dock.release_at(c0, y), None);
    assert_eq!(dock.take_click(), None);
}

/// With the option on, pressing a separator scales instead of dragging.
#[test]
fn sep_scale_takes_over_the_press() {
    let (mut dock, u, v) = dock_with_sep(true);
    let icon0 = dock.cfg.icon_size;
    assert!(dock.press_at(u, v, false), "需要 SetCapture");
    assert!(dock.dragging().is_none(), "不该进入条目拖拽");
    assert!(dock.sep_cursor(u, v), "光标应当是上下双箭头");
    assert!(!dock.take_sep_dirty());

    dock.drag_to(u, v - 100.0);
    assert!(dock.cfg.icon_size > icon0, "上拖应当放大图标");
    assert!(dock.cfg.zoom_size > 96);
    assert!(dock.take_sep_dirty(), "改过配置要通知主循环写盘");

    assert_eq!(dock.release_at(u, v - 100.0), None, "缩放不是点击");
}

/// With the option off, a separator is a normal draggable item.
#[test]
fn sep_scale_disabled_keeps_normal_behaviour() {
    let (mut dock, u, v) = dock_with_sep(false);
    let icon0 = dock.cfg.icon_size;
    assert!(!dock.sep_cursor(u, v));
    assert!(dock.press_at(u, v, false));
    dock.drag_to(u, v - 100.0);
    assert_eq!(dock.cfg.icon_size, icon0);
    assert!(!dock.take_sep_dirty());
}

/// An unchanged size does not mark dirty again.
#[test]
fn sep_scale_only_marks_dirty_when_size_changes() {
    let (mut dock, u, v) = dock_with_sep(true);
    dock.press_at(u, v, false);
    dock.drag_to(u, v - 100.0);
    assert!(dock.take_sep_dirty());
    dock.drag_to(u, v - 100.0);
    assert!(!dock.take_sep_dirty(), "同样的位移不该再改一次配置");
}

/// Capture loss stops scaling but keeps the changed size.
#[test]
fn losing_capture_stops_sep_scale() {
    let (mut dock, u, v) = dock_with_sep(true);
    let icon0 = dock.cfg.icon_size;
    dock.press_at(u, v, false);
    dock.drag_to(u, v - 100.0);
    dock.cancel_drag();
    assert!(dock.cfg.icon_size > icon0, "尺寸改动保留");
    assert!(!dock.sep_cursor(u + 400.0, v), "已经不在拖拽态");
}
