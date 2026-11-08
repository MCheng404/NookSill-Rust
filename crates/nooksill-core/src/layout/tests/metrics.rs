use super::*;

#[test]
fn empty_dock_keeps_four_slots_wide() {
    let m = calc_metrics(&cfg96(), 96, 0, 0, work());
    // 2*12 + 48*4 = 216, plus a fixed 260 of interaction slack.
    // 12 is the `HPad` default; horizontal padding used to be `pad + 2` = 10.
    assert_eq!(m.lw, 216 + 260);
    assert!(m.bar_h > m.s, "栏高必须大于图标");
}

#[test]
fn bar_height_matches_c_formula() {
    let c = cfg96();
    let m = calc_metrics(&c, 96, 3, 0, work());
    assert_eq!(m.bar_h, m.s + 2.0 * c.pad as f32 + 4.0);
    // Horizontal padding comes from `HPad`, not from `pad + 2`.
    assert_eq!(m.pad_x, c.get_i32(Key::HPAD) as f32);
    assert_ne!(m.pad_x, c.pad as f32 + 2.0, "两者默认值已经不同");
    assert_eq!(m.s, c.icon_size as f32);
}

#[test]
fn too_many_items_shrink_instead_of_overflowing() {
    let c = cfg96();
    let m = calc_metrics(&c, 96, 128, 0, Rect::new(0, 0, 800, 600));
    assert!(m.s < c.icon_size as f32, "应当等比缩小");
    assert!(m.s >= c.icon_size as f32 * 0.25, "不得小于 25%");
    assert!(m.lw <= 800, "逻辑宽度不得超出工作区");
}

#[test]
fn clock_adds_width_only_when_enabled() {
    let mut c = cfg96();
    let without = calc_metrics(&c, 96, 3, 0, work());
    c.clock = true;
    let with = calc_metrics(&c, 96, 3, 0, work());
    assert!(with.widget_extra > 0.0);
    // Defaults show time and date with the weekday in month/day format, so
    // the width is the date one: Sc(166).
    assert_eq!(with.clock_w, 166);
    assert!(with.lw > without.lw);
}

/// Clock width follows what is displayed and in which format.
#[test]
fn clock_width_follows_upstream_rules() {
    let mut c = cfg96();
    c.clock = true;

    // `ClockShow`: 0 = both, 1 = time only, 2 = date only.

    // Time only, no seconds: Sc(74).
    c.apply_i32(Key::CLOCK_SHOW, 1);
    assert_eq!(calc_metrics(&c, 96, 3, 0, work()).clock_w, 74);

    // Time only with seconds: Sc(92).
    c.apply_i32(Key::CLOCK_SEC, 1);
    assert_eq!(calc_metrics(&c, 96, 3, 0, work()).clock_w, 92);

    // Date only, month/day with weekday: Sc(166).
    c.apply_i32(Key::CLOCK_SHOW, 2);
    c.apply_i32(Key::CLOCK_SEC, 0);
    assert_eq!(calc_metrics(&c, 96, 3, 0, work()).clock_w, 166);

    // Short format with weekday: Sc(146).
    c.apply_i32(Key::DATE_FMT, 1);
    assert_eq!(calc_metrics(&c, 96, 3, 0, work()).clock_w, 146);

    // Short format without weekday: Sc(110).
    c.apply_i32(Key::CLOCK_WEEK, 0);
    assert_eq!(calc_metrics(&c, 96, 3, 0, work()).clock_w, 110);
}

/// Separator width comes from `SepGap`.
#[test]
fn separator_width_follows_sep_gap() {
    let mut c = cfg96();
    let before = calc_metrics(&c, 96, 4, 1, work());
    c.apply_i32(Key::SEP_GAP, 20);
    let after = calc_metrics(&c, 96, 4, 1, work());
    assert!(after.sep_w > before.sep_w);
    assert_eq!(after.sep_w, 20.0);
}

/// Reflection height is `ReflSize` percent of the icon edge plus `ReflGap`.
#[test]
fn reflection_height_follows_config() {
    let mut c = cfg96();
    c.reflect = true;
    c.apply_i32(Key::REFL_SIZE, 50);
    c.apply_i32(Key::REFL_GAP, 10);
    let m = calc_metrics(&c, 96, 3, 0, work());
    // S * 50% + 10 + 2
    assert!((m.refl_h - (m.s * 0.5 + 12.0)).abs() < 0.01, "{m:?}");
}

#[test]
fn reflection_only_on_bottom_edge() {
    let mut c = cfg96();
    c.reflect = true;
    let bottom = calc_metrics(&c, 96, 3, 0, work());
    assert!(bottom.refl_h > 0.0);
    c.edge = Edge::Top;
    let top = calc_metrics(&c, 96, 3, 0, work());
    assert_eq!(top.refl_h, 0.0, "倒影只在底部停靠时有效");
}

#[test]
fn metric_scales_with_dpi() {
    let c = Config::defaults();
    let m = calc_metrics(&c, 144, 3, 0, Rect::new(0, 0, 2560, 1400));
    assert_eq!(m.s, 72.0);
}

#[test]
fn edge_mapping_roundtrips() {
    let lh = 400.0;
    // `to_logical` maps a point, `map_rect` a rect: top and left rects are
    // flipped into place, so the inverse takes the flipped corner.
    for edge in [Edge::Bottom, Edge::Top, Edge::Left, Edge::Right] {
        let (x, y, w, h) = map_rect(edge, 10.0, 20.0, 30.0, 40.0, lh);
        let (px, py) = match edge {
            Edge::Bottom => (x, y),
            Edge::Top => (x, y + h),
            Edge::Left => (x + w, y),
            Edge::Right => (x, y),
        };
        let (u, v) = to_logical(edge, px, py, lh);
        assert!((u - 10.0).abs() < 0.001, "{edge:?} u={u}");
        assert!((v - 20.0).abs() < 0.001, "{edge:?} v={v}");
        if edge == Edge::Left || edge == Edge::Right {
            assert_eq!((w, h), (40.0, 30.0), "竖向停靠要交换宽高");
        } else {
            assert_eq!((w, h), (30.0, 40.0));
        }
    }
}

#[test]
fn magnification_curve_is_smooth_at_edges() {
    assert!((magnify_weight(0.0, 100.0) - 1.0).abs() < 1e-6);
    assert!((magnify_weight(100.0, 100.0)).abs() < 1e-6);
    assert_eq!(magnify_weight(200.0, 100.0), 0.0);
    // Midpoint = 0.5.
    let mid = magnify_weight(50.0, 100.0);
    assert!((mid - 0.5).abs() < 1e-6);
    // Monotonically decreasing.
    let mut prev = f32::MAX;
    for i in 0..=10 {
        let w = magnify_weight(i as f32 * 5.0, 100.0);
        assert!(w <= prev + 1e-6);
        prev = w;
    }
}

/// The bounce starts and ends at zero and decays over `BOUNCE_MS`.
#[test]
fn bounce_curve_starts_and_ends_at_zero() {
    let s = 52.0;
    assert_eq!(bounce_offset(0, s), 0.0, "起点必须为 0，否则会瞬移");
    assert_eq!(bounce_offset(BOUNCE_MS, s), 0.0, "结束必须为 0");
    assert_eq!(bounce_offset(BOUNCE_MS + 500, s), 0.0, "超出时长后恒为 0");
    assert!(!bounce_active(BOUNCE_MS));
}

/// Peaks at x = 0.25 and 0.75, the second lower than the first.
#[test]
fn bounce_has_two_decaying_peaks() {
    let s = 52.0;
    let p1 = bounce_offset(BOUNCE_MS / 4, s);
    let p2 = bounce_offset(BOUNCE_MS * 3 / 4, s);
    assert!(p1 > 0.0 && p2 > 0.0);
    assert!(p1 > p2, "第二峰应当衰减：{p1} vs {p2}");
    // The first peak is about `s * 0.48 * 0.8625`.
    assert!((p1 - s * 0.48 * 0.8625).abs() < 0.5, "峰值 {p1} 与公式不符");
}

/// The offset is never negative; the caller picks the direction.
#[test]
fn bounce_offset_is_never_negative() {
    for t in (0..BOUNCE_MS).step_by(17) {
        assert!(bounce_offset(t, 52.0) >= 0.0, "t={t}");
    }
}
