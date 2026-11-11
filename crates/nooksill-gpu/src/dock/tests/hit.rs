use super::*;

/// A hit test at the slot centre lands on that slot.
#[test]
fn hit_test_lands_on_the_icon_under_the_cursor() {
    let dock = settled_dock(3);
    let s = dock.metrics.s;
    for i in 0..3 {
        let center = dock.geom.slot_l[i] + s / 2.0;
        assert_eq!(
            dock.hit_at(center, dock.geom.icon_bottom - s / 2.0),
            Some(i),
            "第 {i} 个槽位的中心没命中自己"
        );
    }
    // Left of the bar hits nothing.
    assert_eq!(dock.hit_at(dock.geom.bar_l - 30.0, 120.0), None);
}

/// Reflection exists only for a bottom dock with the option on.
#[test]
fn reflection_only_for_a_bottom_dock() {
    let mut c = Config::defaults();
    c.reflect = true;
    let mut dock = Dock::new(c.clone(), Vec::new(), LRect::new(0, 0, 2560, 1600));
    dock.update(96, nooksill_core::layout::REF_FRAME_MS);
    let spec = dock.reflection_spec().expect("底部栏应当有倒影");
    assert!(spec.src_h > 0 && spec.height > 0, "{spec:?}");
    assert!(spec.opacity > 0.0, "默认不透明度不该是 0");

    // Disabled: no reflection.
    c.reflect = false;
    let mut off = Dock::new(c.clone(), Vec::new(), LRect::new(0, 0, 2560, 1600));
    off.update(96, nooksill_core::layout::REF_FRAME_MS);
    assert!(off.reflection_spec().is_none());

    // Left docks have no reflection; horizontal mirroring is unimplemented.
    c.reflect = true;
    c.edge = Edge::Left;
    let mut left = Dock::new(c, Vec::new(), LRect::new(0, 0, 2560, 1600));
    left.update(96, nooksill_core::layout::REF_FRAME_MS);
    assert!(left.reflection_spec().is_none());
}

/// Widget-card hit test.
#[test]
fn widget_at_finds_the_card() {
    let mut c = Config::defaults();
    c.clock = true;
    c.apply_i32(Key::RES_MON, 1);
    let mut dock = Dock::new(c, Vec::new(), LRect::new(0, 0, 2560, 1600));
    dock.update(96, nooksill_core::layout::REF_FRAME_MS);

    // The resource card is in the strip.
    let idx = dock
        .metrics
        .widgets
        .iter()
        .position(|(k, _)| k == layout::WidgetKind::Resource);
    let idx = idx.expect("条带里应当有资源卡片");
    let left = dock
        .metrics
        .widgets
        .left_of(idx, dock.geom.bar_l, dock.geom.bar_w, dock.metrics.gap)
        .expect("应当有位置");
    let w = dock
        .metrics
        .widgets
        .width_of(layout::WidgetKind::Resource)
        .expect("应当有宽度");
    let y = dock.geom.bar_top + dock.metrics.bar_h / 2.0;

    assert_eq!(
        dock.widget_at(left + w as f32 / 2.0, y),
        Some(layout::WidgetKind::Resource)
    );
    // The icon area does not hit a card.
    assert_eq!(dock.widget_at(dock.geom.bar_l + 10.0, y), None);
}

/// The hot zone covers the widget strip, not just the icon bar.
#[test]
fn hot_zone_covers_the_widget_strip() {
    let mut c = Config::defaults();
    c.clock = true;
    c.apply_i32(Key::RES_MON, 1);
    let mut dock = Dock::new(c, Vec::new(), LRect::new(0, 0, 2560, 1600));
    dock.update(96, nooksill_core::layout::REF_FRAME_MS);
    assert!(dock.metrics.widgets.len >= 2, "时钟 + 资源卡片应当都在");

    let (bx, by, bw, bh) = dock.bar_phys();
    let (hx, hy, hw, hh) = dock.hot_phys();
    // Right edge of the last card.
    let mut right = bx + bw;
    for (i, (_, w)) in dock.metrics.widgets.iter().enumerate() {
        let left = dock
            .metrics
            .widgets
            .left_of(i, dock.geom.bar_l, dock.geom.bar_w, dock.metrics.gap)
            .expect("条带成员必有位置");
        let (wx, _, ww, _) = layout::map_rect(
            dock.cfg.edge,
            left,
            dock.geom.bar_top,
            w as f32,
            dock.metrics.bar_h,
            dock.metrics.lh as f32,
        );
        right = right.max(wx + ww);
    }

    assert!(hx <= bx, "热区左缘不该比栏体靠里");
    assert!(
        hx + hw >= right - 0.5,
        "热区右缘应当覆盖到最后一张小组件卡片（{hx}+{hw} < {right}）"
    );
    // Height includes the shadow margin and must contain the whole bar.
    assert!(hy <= by, "热区上缘不该裁掉栏体");
    assert!(hy + hh >= by + bh, "热区下缘要含投影留白");
}

/// A cursor over the clock counts as near for a bottom dock.
#[test]
fn cursor_on_clock_counts_as_near() {
    let mut c = Config::defaults();
    c.apply_i32(Key::RES_MON, 1);
    c.edge = Edge::Bottom;
    let mut dock = Dock::new(c, Vec::new(), LRect::new(0, 0, 2560, 1600));
    dock.update(96, nooksill_core::layout::REF_FRAME_MS);

    let (hx, hy, hw, hh) = dock.hot_phys();
    // Just inside the right end of the hot zone, over the widget strip.
    let cursor = ((hx + hw - 20.0) as i32, (hy + hh / 2.0) as i32);
    assert!(
        layout::near_dock(
            dock.cfg.edge,
            cursor,
            (hx, hy, hw, hh),
            dock.work,
            0.0,
            false
        ),
        "光标在小组件上应当算靠近"
    );
    // Blank space to the left, outside the hot zone, is not near.
    let far = ((hx - 200.0) as i32, (hy + hh / 2.0) as i32);
    assert!(!layout::near_dock(
        dock.cfg.edge,
        far,
        (hx, hy, hw, hh),
        dock.work,
        0.0,
        false
    ));
}

/// A click needs press and release on the same item.
#[test]
fn click_requires_press_and_release_on_the_same_item() {
    let mut dock = settled_dock(2);
    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let c0 = dock.geom.slot_l[0] + s / 2.0;
    let c1 = dock.geom.slot_l[1] + s / 2.0;

    // Normal click.
    assert!(dock.press_at(c0, y, false));
    assert_eq!(dock.release_at(c0, y), Some(0));
    assert_eq!(dock.take_click(), Some((0, false)));

    // Press item 0, drag to item 1, release: not a click.
    // A real release is preceded by WM_MOUSEMOVE, hence `drag_to`.
    assert!(dock.press_at(c0, y, false));
    dock.drag_to(c1, y);
    assert_eq!(dock.release_at(c1, y), None);
    assert_eq!(dock.take_click(), None);

    // Pressing outside any item does not arm a click.
    assert!(!dock.press_at(dock.geom.bar_l - 40.0, y, false));
    assert_eq!(dock.release_at(c0, y), None);
}

/// Bounce honours the `Bounce` config.
#[test]
fn bounce_respects_config() {
    let mut dock = settled_dock(1);
    assert!(dock.cfg.bounce);
    dock.start_bounce(0);
    assert!(dock.items[0].bounce_at.is_some());

    dock.cfg.bounce = false;
    dock.start_bounce(0);
    // Clear the previous bounce so only the new attempt is observed.
    dock.items[0].bounce_at = None;
    dock.start_bounce(0);
    assert!(dock.items[0].bounce_at.is_none(), "Bounce=0 时不该起动画");
}

/// Input events mark the dock dirty so `hot` is recomputed.
#[test]
fn input_marks_dirty() {
    let mut dock = settled_dock(1);
    let _ = dock.take_dirty();
    dock.mark_dirty();
    assert!(dock.take_dirty());
    assert!(!dock.take_dirty(), "取走之后应当清零");
}

/// Window pixels and logical dock axes round-trip on all four edges.
#[test]
fn point_mapping_roundtrips_on_every_edge() {
    let mut dock = settled_dock(3);
    for edge in [Edge::Bottom, Edge::Top, Edge::Left, Edge::Right] {
        dock.cfg.edge = edge;
        dock.update(96, nooksill_core::layout::REF_FRAME_MS);
        for (u, v) in [(120.0, 24.0), (dock.metrics.lh as f32 * 0.4, 8.0)] {
            let (px, py) = dock.window_point(u, v);
            let (u2, v2) = dock.logical_point(px, py);
            assert!((u2 - u).abs() < 0.01, "{edge:?}: u {u2} != {u}");
            assert!((v2 - v).abs() < 0.01, "{edge:?}: v {v2} != {v}");
        }
    }
}

/// A window-space click on a vertical dock still reaches the right slot.
#[test]
fn vertical_dock_hit_uses_logical_axes() {
    for edge in [Edge::Left, Edge::Right] {
        let mut dock = settled_dock(3);
        dock.cfg.edge = edge;
        dock.update(96, nooksill_core::layout::REF_FRAME_MS);
        let s = dock.metrics.s;
        let logical = (
            dock.geom.slot_l[1] + s / 2.0,
            dock.geom.icon_bottom - s / 2.0,
        );
        let (px, py) = dock.window_point(logical.0, logical.1);
        let (u, v) = dock.logical_point(px, py);
        assert_eq!(
            dock.hit_at(u, v),
            Some(1),
            "{edge:?}: 窗口坐标没落到第 1 槽"
        );

        let (l, t, r, b) = dock.slot_screen_rect(1, 100, 200).expect("第二个槽位");
        assert!(
            ((r - l) as f32 - dock.metrics.bar_h).abs() <= 2.0,
            "{edge:?}: 横向应是栏体厚度"
        );
        assert!(
            ((b - t) as f32 - dock.metrics.s).abs() <= 2.0,
            "{edge:?}: 纵向应是槽宽"
        );
        assert!(l >= 100 && t >= 200, "{edge:?}: 矩形跑出窗口");
    }
}

/// Widget-card anchors transpose too, or popups open on the wrong side.
#[test]
fn vertical_widget_anchor_is_transposed() {
    for edge in [Edge::Left, Edge::Right] {
        let mut dock = settled_dock(1);
        dock.cfg.edge = edge;
        dock.cfg.clock = true;
        dock.update(96, nooksill_core::layout::REF_FRAME_MS);
        let (l, t, r, b) = dock
            .widget_screen_rect(layout::WidgetKind::Clock, 100, 200)
            .expect("时钟卡片");
        assert!(b - t > r - l, "{edge:?}: 竖排卡片应是竖长矩形");
        assert!(l >= 100.0 && t >= 200.0, "{edge:?}: 锚点跑出窗口");
    }
}
