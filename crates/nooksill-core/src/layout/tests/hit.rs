use super::*;

#[test]
fn hit_test_takes_first_from_left() {
    let slot_l = [100.0, 160.0, 220.0];
    let slot_r = [130.0, 190.0, 250.0];
    let visible = [true, true, true];
    // gap=40 leaves 20 of slack: 145 falls inside icon 0 (borrowing gap/2) and
    // inside icon 1, so the left one wins.
    assert_eq!(
        hit_index(145.0, 0.0, &slot_l, &slot_r, &visible, 40.0, 0.0, 88.0, 200.0),
        Some(0)
    );
    assert_eq!(
        hit_index(100.0, 0.0, &slot_l, &slot_r, &visible, 20.0, 0.0, 88.0, 200.0),
        Some(0)
    );
    // 195 falls inside icon 1's range [150, 200).
    assert_eq!(
        hit_index(195.0, 0.0, &slot_l, &slot_r, &visible, 20.0, 0.0, 88.0, 200.0),
        Some(1)
    );
    // 200 is the exclusive right bound, so no hit.
    assert_eq!(
        hit_index(200.0, 0.0, &slot_l, &slot_r, &visible, 20.0, 0.0, 88.0, 200.0),
        None
    );
    // Left of the gap/2 slack, so no hit.
    assert_eq!(
        hit_index(50.0, 0.0, &slot_l, &slot_r, &visible, 20.0, 0.0, 88.0, 200.0),
        None
    );
}

/// Vertical range is `bar_top - zoom - 16` to `lh`.
#[test]
fn hit_test_respects_vertical_range() {
    let slot_l = [100.0];
    let slot_r = [130.0];
    let visible = [true];
    // bar_top=50, zoom=88 -> upper bound -54; v=0 is inside.
    assert_eq!(
        hit_index(110.0, 0.0, &slot_l, &slot_r, &visible, 0.0, 50.0, 88.0, 200.0),
        Some(0)
    );
    // Above the upper bound.
    assert_eq!(
        hit_index(110.0, -60.0, &slot_l, &slot_r, &visible, 0.0, 50.0, 88.0, 200.0),
        None
    );
    // Below the lower bound.
    assert_eq!(
        hit_index(110.0, 250.0, &slot_l, &slot_r, &visible, 0.0, 50.0, 88.0, 200.0),
        None
    );
}

/// Icons with `cur <= 1` are not hittable.
#[test]
fn hit_test_skips_invisible_slots() {
    let slot_l = [100.0, 160.0];
    let slot_r = [130.0, 190.0];
    let visible = [false, true];
    assert_eq!(
        hit_index(110.0, 0.0, &slot_l, &slot_r, &visible, 0.0, 0.0, 88.0, 200.0),
        None
    );
    assert_eq!(
        hit_index(170.0, 0.0, &slot_l, &slot_r, &visible, 0.0, 0.0, 88.0, 200.0),
        Some(1)
    );
}

/// Maps magnified positions back onto the base layout.
#[test]
fn actual_to_base_maps_interpolated() {
    let base_c = [100.0, 200.0];
    let act_c = [100.0, 240.0];
    // Middle of the actual interval to the middle of the base interval.
    assert!((map_actual_to_base(170.0, &base_c, &act_c) - 150.0).abs() < 0.01);
    // Outside the ends: shift by the endpoint.
    assert!((map_actual_to_base(50.0, &base_c, &act_c) - 50.0).abs() < 0.01);
    // Mismatched lengths: no mapping.
    assert_eq!(map_actual_to_base(170.0, &[100.0], &act_c), 170.0);
}

/// `hit_test` uses this frame's slots and respects "no hit while hiding".
#[test]
fn hit_test_uses_current_slots() {
    let c = cfg96();
    let m = calc_metrics(&c, 96, 3, 0, work());
    let bars = bars(&m);
    let cur = [m.s; 3];
    let s = place_slots(&m, &cur, 0.0);
    let center = s.act_c[1];
    let at = |hover, hide_cur| View {
        mouse: (center, bars.icon_bottom - 5.0),
        hover,
        hide_cur,
    };

    assert_eq!(hit_test(&m, &bars, &s, &cur, at(true, 0.0)), Some(1));
    assert_eq!(
        hit_test(&m, &bars, &s, &cur, at(true, 0.6)),
        None,
        "隐藏过半后不应命中"
    );
    assert_eq!(
        hit_test(&m, &bars, &s, &cur, at(false, 0.0)),
        None,
        "鼠标不在窗口内不应命中"
    );
}

/// Bottom edge: the whole band counts as near, widgets included.
#[test]
fn near_dock_bottom_covers_the_whole_strip() {
    // Icons span 400..1000, widgets reach 1700; the band is their union.
    let hot = (400.0, 1487.0, 1300.0, 88.0);
    let work = Rect::new(0, 0, 2560, 1600);
    let near = |x, y| near_dock(Edge::Bottom, (x, y), hot, work, 129.0, false);
    let keep = |x, y| near_dock(Edge::Bottom, (x, y), hot, work, 129.0, true);

    assert!(near(700, 1520), "图标上算靠近");
    assert!(near(1500, 1520), "时钟上也算靠近（早先这里判成离开）");
    assert!(near(1690, 1520), "带子右端仍在带内");
    assert!(near(400, 1560), "左边缘");
    // Reaching 129 px into the screen keeps the magnified icons near.
    assert!(near(700, 1360), "伸展区里算靠近");
    assert!(!near(700, 1300), "伸展区外不算");
    // Edge probe: must lie within the band span.
    assert!(near(1500, 1599), "贴底边 + 在带子跨度内");
    assert!(!near(200, 1599), "贴底边但在带子左边，不算");
    assert!(!near(700, 1200), "离得远不算");
    // Hysteresis: while expanded the tolerance is 32 px wider, so a cursor at
    // 1330 counts as near when open and as far when collapsed.
    assert!(keep(700, 1330), "展开时 1330 仍在保留区内");
    assert!(!near(700, 1330), "收起时同一位置算离开");
}

/// Top edge: reaches downward, probe against the top of the work area.
#[test]
fn near_dock_top_reaches_downward() {
    let hot = (400.0, 0.0, 1300.0, 88.0);
    let work = Rect::new(0, 0, 2560, 1600);
    let near = |x, y| near_dock(Edge::Top, (x, y), hot, work, 129.0, false);

    assert!(near(700, 40), "栏体上");
    assert!(near(700, 200), "往下伸展的放大区");
    assert!(!near(700, 300), "再往下就离开了");
    // Collapsed probe: flush with the top of the work area.
    assert!(near(700, 1), "贴上沿");
}

/// Left edge: extends sideways, probe against the left edge of the work area.
#[test]
fn near_dock_left_is_axis_swapped() {
    let hot = (0.0, 400.0, 88.0, 700.0);
    let work = Rect::new(0, 0, 2560, 1600);
    let near = |x, y| near_dock(Edge::Left, (x, y), hot, work, 129.0, false);

    assert!(near(40, 700), "栏体上");
    assert!(near(200, 700), "往右伸展的放大区");
    assert!(!near(300, 700), "太远");
    assert!(near(1, 700), "贴左边");
    assert!(!near(1, 200), "贴左边但不在跨度内");
}
