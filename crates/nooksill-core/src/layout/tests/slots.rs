use super::*;

/// Base width must equal the `wb` of `calc_metrics`; if they drift, the bar is
/// drawn outside the window.
#[test]
fn base_width_matches_metrics_formula() {
    let c = cfg96();
    let m = calc_metrics(&c, 96, 3, 0, work());
    assert_eq!(
        base_width(&m, &kinds(3)),
        2.0 * m.pad_x + 3.0 * m.s + 2.0 * m.gap
    );
    // Empty dock: 4 slots wide.
    assert_eq!(base_width(&m, &[]), 2.0 * m.pad_x + 4.0 * m.s);
}

/// Centers are evenly spaced and symmetric about the window center.
#[test]
fn base_centers_are_centered_and_evenly_spaced() {
    let c = cfg96();
    let m = calc_metrics(&c, 96, 3, 0, work());
    let centers = base_centers(&m, &kinds(3), 0.0);
    assert_eq!(centers.len(), 3);
    let step = m.s + m.gap;
    assert!((centers[1] - centers[0] - step).abs() < 0.01);
    assert!((centers[2] - centers[1] - step).abs() < 0.01);
    // First and last are symmetric about the window center.
    let mid = (centers[0] + centers[2]) / 2.0;
    assert!((mid - m.lw as f32 / 2.0).abs() < 0.01, "mid={mid}");
}

/// The strip reservation shifts the bar left by half its width.
#[test]
fn widget_extra_shifts_the_bar_left() {
    let c = cfg96();
    let m = calc_metrics(&c, 96, 3, 0, work());
    let plain = base_centers(&m, &kinds(3), 0.0);
    let with_clock = base_centers(&m, &kinds(3), 120.0);
    assert!((plain[0] - with_clock[0] - 60.0).abs() < 0.01);
}

/// Without hover the targets equal the base sizes.
#[test]
fn magnify_targets_fall_back_to_base_size() {
    let c = cfg96();
    let m = calc_metrics(&c, 96, 3, 0, work());
    let bc = base_centers(&m, &kinds(3), 0.0);
    let t = magnify_targets(&m, &kinds(3), &bc, &bc, View::default(), true);
    assert!(t.iter().all(|v| (*v - m.s).abs() < 0.01));
}

/// Peak under the cursor, weaker with distance, base size beyond the radius.
#[test]
fn magnify_targets_peak_under_the_mouse() {
    let mut c = cfg96();
    c.zoom = true;
    // 9 icons: R = (s + g) * 2.6 ~ 146 while the spacing is s + g = 56, so 3
    // slots (168 > 146) are needed to reach the region beyond R.
    let ks = kinds(9);
    let m = calc_metrics(&c, 96, 9, 0, work());
    assert!(m.z > m.s, "前提：放大尺寸大于基准");
    let bc = base_centers(&m, &ks, 0.0);
    let t = magnify_targets(
        &m,
        &ks,
        &bc,
        &bc,
        View {
            mouse: (bc[4], 0.0),
            hover: true,
            hide_cur: 0.0,
        },
        true,
    );

    assert!((t[4] - m.z).abs() < 0.01, "鼠标下的图标应到最大: {t:?}");
    // Neighbors are inside R and zoom partially, but stay below the peak.
    assert!(t[3] > m.s && t[3] < m.z, "相邻应部分放大: {t:?}");
    assert!(t[2] < t[3] && t[1] < t[2], "离得越远越小: {t:?}");
    // 4 slots = 224 > R, unaffected.
    assert!((t[0] - m.s).abs() < 0.01, "半径外应保持基准: {t:?}");
    assert!((t[8] - m.s).abs() < 0.01);
}

/// Separator slots keep `sep_w` and never zoom.
#[test]
fn separators_never_grow() {
    let mut c = cfg96();
    c.zoom = true;
    let m = calc_metrics(&c, 96, 3, 1, work());
    let ks = [SlotKind::Icon, SlotKind::Separator, SlotKind::Icon];
    let bc = base_centers(&m, &ks, 0.0);
    let t = magnify_targets(
        &m,
        &ks,
        &bc,
        &bc,
        View {
            mouse: (bc[1], 0.0),
            hover: true,
            hide_cur: 0.0,
        },
        true,
    );
    assert!((t[1] - m.sep_w).abs() < 0.01);
}

/// No magnification past half hidden, or icons would press on the screen edge.
#[test]
fn hidden_dock_does_not_magnify() {
    let mut c = cfg96();
    c.zoom = true;
    let m = calc_metrics(&c, 96, 3, 0, work());
    let bc = base_centers(&m, &kinds(3), 0.0);
    let t = magnify_targets(
        &m,
        &kinds(3),
        &bc,
        &bc,
        View {
            mouse: (bc[1], 0.0),
            hover: true,
            hide_cur: 0.6,
        },
        true,
    );
    assert!(t.iter().all(|v| (*v - m.s).abs() < 0.01));
}

/// Slots tile with no overlap and gaps of exactly `gap`.
#[test]
fn place_slots_tile_without_overlap() {
    let c = cfg96();
    let m = calc_metrics(&c, 96, 3, 0, work());
    let cur = [m.s; 3];
    let s = place_slots(&m, &cur, 0.0);
    assert_eq!(s.slot_l.len(), 3);
    for i in 0..2 {
        assert!((s.slot_l[i + 1] - s.slot_r[i] - m.gap).abs() < 0.01);
    }
    assert!((s.slot_r[2] - s.slot_l[0] - s.bar_w + 2.0 * m.pad_x).abs() < 0.01);
}

/// An empty dock keeps 4 slots plus padding.
#[test]
fn place_slots_keeps_empty_dock_wide_enough() {
    let c = cfg96();
    let m = calc_metrics(&c, 96, 0, 0, work());
    let s = place_slots(&m, &[], 0.0);
    assert!((s.bar_w - (2.0 * m.pad_x + 4.0 * m.s)).abs() < 0.01);
    assert!(s.slot_l.is_empty());
}

/// Snaps inside `SNAP` and reports the animation as finished.
#[test]
fn lerp_snaps_when_close() {
    let mut v = 10.0;
    assert!(!lerp_to(&mut v, 10.2, REF_FRAME_MS));
    assert_eq!(v, 10.2);

    let mut v = 10.0;
    assert!(lerp_to(&mut v, 20.0, REF_FRAME_MS));
    assert!(v > 10.0 && v < 20.0, "必须只走一帧的距离: {v}");
}

/// Same wall-clock time gives the same result at 60 Hz (60 steps of 16.7 ms)
/// and at 240 Hz (240 steps of 4.17 ms).
#[test]
fn lerp_is_frame_rate_independent() {
    let mut slow = 0.0f32;
    let mut fast = 0.0f32;
    for _ in 0..60 {
        lerp_to(&mut slow, 100.0, REF_FRAME_MS);
    }
    for _ in 0..240 {
        lerp_to(&mut fast, 100.0, REF_FRAME_MS / 4.0);
    }
    assert!(
        (slow - fast).abs() < 1.0,
        "60 Hz 落在 {slow}，240 Hz 落在 {fast}，差得太多"
    );
}

/// `dt` is capped at 4 frames, so one stall cannot finish the animation.
#[test]
fn lerp_caps_a_huge_frame_gap() {
    let mut v = 0.0f32;
    lerp_to(&mut v, 100.0, 5_000.0);
    assert!(v < 90.0, "一帧跳太远: {v}");
}

/// Bar top to bottom is exactly `bar_h`.
#[test]
fn bars_are_consistent_with_metrics() {
    let c = cfg96();
    let m = calc_metrics(&c, 96, 3, 0, work());
    let b = bars(&m);
    assert!((b.bottom - b.top - m.bar_h).abs() < 0.01);
    assert!((b.bottom - b.icon_bottom - m.pad - 2.0).abs() < 0.01);
}

/// Upward drag grows, downward shrinks in steps of 0.6; zoom follows the ratio.
#[test]
fn sep_scale_follows_drag_direction() {
    assert_eq!(sep_scale(0.0, 64, 96), (64, 96));
    assert_eq!(sep_scale(-100.0, 64, 96), (124, 186), "上拖 100px → +60");
    assert_eq!(
        sep_scale(100.0, 64, 96),
        (24, 36),
        "下拖 100px → -60，触底夹紧"
    );
}

/// Clamps both directions: icon 24..=128, zoom 32..=220.
#[test]
fn sep_scale_clamps_both_bounds() {
    assert_eq!(sep_scale(-1000.0, 64, 96), (128, 192));
    assert_eq!(sep_scale(1000.0, 64, 96), (24, 36));
    // Zoom has its own clamp: a big icon with a small zoom lands below 32.
    assert_eq!(sep_scale(1000.0, 128, 32), (24, 32));
    assert_eq!(sep_scale(-1000.0, 24, 220), (128, 220));
}

/// Dragging back to the start restores the original sizes.
#[test]
fn sep_scale_roundtrip_returns_to_start() {
    let (icon, zoom) = sep_scale(-137.0, 64, 96);
    assert!(icon > 64 && zoom > 96);
    // dy=0 restores the start because the base is always icon0/zoom0.
    assert_eq!(sep_scale(0.0, 64, 96), (64, 96));
}
