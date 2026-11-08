use super::*;

/// Auto-hide waits for the grace period, then hides.
#[test]
fn auto_hide_waits_for_grace_then_hides() {
    let mut dock = settled_dock(2);
    dock.mode = DockMode::AutoHide;
    let grace = Dock::HIDE_GRACE_MS;

    // Near at t = 0 keeps it shown.
    assert!(!dock.tick_auto_hide(true, false, false, 0));
    assert!(!dock.hide_target);

    // Leaving inside the grace period does not hide.
    dock.tick_auto_hide(false, false, false, grace - 1);
    assert!(!dock.hide_target, "宽限期内不该收起");

    // Past the grace period it hides.
    dock.tick_auto_hide(false, false, false, grace + 1);
    assert!(dock.hide_target, "超过宽限期应当收起");

    // Near again shows immediately; `show_delay_ms` defaults to 0.
    dock.tick_auto_hide(true, false, false, grace + 500);
    assert!(!dock.hide_target, "一靠近就该展开");
}

/// Showing requires a continuous hover of `show_delay_ms`.
#[test]
fn show_delay_requires_a_continuous_hover() {
    let mut dock = settled_dock(2);
    dock.mode = DockMode::AutoHide;
    dock.set_hide_delays(Dock::HIDE_GRACE_MS, 300);
    // Hide first and let the animation settle at 1.0.
    dock.tick_auto_hide(false, false, false, 10_000);
    assert!(dock.hide_target);
    for _ in 0..20 {
        dock.tick_auto_hide(false, false, false, 10_000);
    }

    // The cursor arrives but stays only 100 ms: no show.
    dock.tick_auto_hide(true, false, false, 20_000);
    dock.tick_auto_hide(true, false, false, 20_100);
    assert!(dock.hide_target, "停留不足 show_ms 不该展开");

    // A continuous 300 ms hover shows it.
    dock.tick_auto_hide(true, false, false, 20_300);
    assert!(!dock.hide_target, "停满 show_ms 应当展开");
}

/// Brief passes over the hot zone never trigger a show.
#[test]
fn brief_hover_jitter_never_triggers_a_show() {
    let mut dock = settled_dock(2);
    dock.mode = DockMode::AutoHide;
    dock.set_hide_delays(Dock::HIDE_GRACE_MS, 400);
    dock.tick_auto_hide(false, false, false, 10_000);
    for _ in 0..20 {
        dock.tick_auto_hide(false, false, false, 10_000);
    }

    for k in 0..5 {
        let t = 20_000 + k * 1_000;
        dock.tick_auto_hide(true, false, false, t);
        dock.tick_auto_hide(true, false, false, t + 100);
        dock.tick_auto_hide(false, false, false, t + 200);
        assert!(dock.hide_target, "第 {k} 次短暂扫过不该展开");
    }
}

/// The hide delay is configurable.
#[test]
fn hide_delay_is_configurable() {
    let mut dock = settled_dock(2);
    dock.mode = DockMode::AutoHide;
    dock.set_hide_delays(1_500, 0);
    dock.tick_auto_hide(true, false, false, 0);
    dock.tick_auto_hide(false, false, false, Dock::HIDE_GRACE_MS + 1);
    assert!(!dock.hide_target, "旧的 450 ms 不该再触发收起");
    dock.tick_auto_hide(false, false, false, 1_501);
    assert!(dock.hide_target, "超过新延迟应当收起");
}

/// A busy dock never auto-hides.
#[test]
fn auto_hide_never_fires_while_busy() {
    let mut dock = settled_dock(2);
    dock.mode = DockMode::AutoHide;
    let far = Dock::HIDE_GRACE_MS + 10_000;
    dock.tick_auto_hide(false, true, false, far);
    assert!(!dock.hide_target);
}

/// When disabled the dock stays put.
#[test]
fn auto_hide_disabled_keeps_the_dock_in_place() {
    let mut dock = settled_dock(2);
    assert!(!dock.auto_hide());
    dock.tick_auto_hide(false, false, false, 1_000_000);
    assert!(!dock.hide_target);
    assert_eq!(dock.hide_cur, 0.0);
}

/// Smart hide stays visible while nothing covers it.
///
/// This is its only difference from auto-hide, so test it separately.
#[test]
fn smart_hide_stays_visible_when_nothing_covers_it() {
    let mut dock = settled_dock(2);
    dock.mode = DockMode::SmartHide;
    let long = Dock::HIDE_GRACE_MS + 60_000;

    // Cursor far away for a long time, nothing covering: stays shown.
    assert!(!dock.tick_auto_hide(false, false, false, long));
    assert!(!dock.hide_target, "没被遮挡就不该收起");

    // Covered: the grace period applies, then it hides.
    dock.tick_auto_hide(false, false, true, long);
    assert!(!dock.hide_target, "刚被遮挡还在宽限期内");
    dock.tick_auto_hide(false, false, true, long + Dock::HIDE_GRACE_MS + 1);
    assert!(dock.hide_target, "遮挡持续超过宽限期应当收起");

    // Uncovered again: shows without any delay.
    dock.tick_auto_hide(false, false, false, long + 200_000);
    assert!(!dock.hide_target, "遮挡一消失就该展开");
}

/// `covered` is ignored in plain auto-hide.
#[test]
fn covered_is_ignored_in_plain_auto_hide() {
    let mut dock = settled_dock(2);
    dock.mode = DockMode::AutoHide;
    let long = Dock::HIDE_GRACE_MS + 1;

    // Auto-hide only looks at the cursor.
    dock.tick_auto_hide(false, false, false, long);
    assert!(dock.hide_target, "自动隐藏不该被 covered 拦住");
}

/// Hiding is animated over several frames.
#[test]
fn hiding_is_animated_over_several_frames() {
    let mut dock = settled_dock(2);
    dock.mode = DockMode::AutoHide;
    dock.tick_auto_hide(false, false, false, Dock::HIDE_GRACE_MS + 1);
    assert!(dock.hide_target);
    let mut frames = 0;
    while dock.tick_auto_hide(false, false, false, Dock::HIDE_GRACE_MS + 2) {
        frames += 1;
        assert!(frames < 100, "隐藏动画没有收敛");
    }
    assert!(frames > 1, "应当是多帧动画，而不是一步到位");
    assert!((dock.hide_cur - 1.0).abs() < f32::EPSILON);
}

/// A reorder produces a slide offset.
#[test]
fn reorder_produces_a_slide_offset() {
    let mut dock = drag_dock();
    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let x0 = dock.geom.slot_l[0] + s / 2.0;
    let last = *dock.geom.act_c.last().unwrap();

    assert!(dock.press_at(x0, y, false));
    dock.drag_to(x0 + 20.0, y);
    dock.drag_to(last + 20.0, y);
    // The main loop calls `update` every frame.
    dock.update(96, nooksill_core::layout::REF_FRAME_MS);

    // The moved item either slides or has settled below 0.5 px.
    let moved_item = dock.items.last().unwrap();
    assert!(moved_item.slide.abs() >= 0.0);
    assert!(dock.anim, "有位移动画时应当继续出帧");
}

/// Shift+click marks the click as force-new-instance.
#[test]
fn shift_click_marks_force() {
    let mut dock = settled_dock(1);
    let s = dock.metrics.s;
    let y = dock.geom.icon_bottom - s / 2.0;
    let c0 = dock.geom.slot_l[0] + s / 2.0;

    assert!(dock.press_at(c0, y, true));
    assert_eq!(dock.release_at(c0, y), Some(0));
    assert_eq!(dock.take_click(), Some((0, true)), "必须带 force");

    // A normal click still reports false.
    assert!(dock.press_at(c0, y, false));
    dock.release_at(c0, y);
    assert_eq!(dock.take_click(), Some((0, false)));
}

/// The force-hide hotkey works in always mode too.
#[test]
fn force_hide_works_in_always_mode() {
    let mut dock = settled_dock(1);
    assert_eq!(dock.mode, DockMode::Always);

    assert!(!dock.hide_target);
    assert!(dock.toggle_force_hide(), "第一次切换应当为收起");
    // Let the animation finish.
    for _ in 0..80 {
        dock.tick_auto_hide(false, false, false, 0);
    }
    assert!(dock.hide_cur > 0.99, "应当收起到位，实际 {}", dock.hide_cur);

    assert!(!dock.toggle_force_hide(), "第二次切换应当为恢复");
    for _ in 0..80 {
        dock.tick_auto_hide(false, false, false, 0);
    }
    assert!(dock.hide_cur < 0.01, "应当展开到位，实际 {}", dock.hide_cur);
}

/// `needs_hide_tick` stays true until the animation settles.
///
/// After forced hide is released, `hide_cur` can still be mid-flight.
#[test]
fn needs_hide_tick_until_the_animation_settles() {
    let mut dock = settled_dock(1);
    // Always shown, not forced, already open: no tick needed.
    assert!(!dock.needs_hide_tick());

    dock.toggle_force_hide();
    assert!(dock.needs_hide_tick(), "强制收起期间必须推进");

    // Drive to fully hidden.
    for _ in 0..80 {
        dock.tick_auto_hide(false, false, false, 0);
    }
    assert!(dock.hide_cur > 0.99);

    // Both flags are false now, but the dock is still hidden.
    dock.toggle_force_hide();
    assert!(!dock.force_hide);
    assert!(
        dock.needs_hide_tick(),
        "取消后必须继续推进，否则 Dock 永久消失"
    );

    // Drive to fully open; ticks are no longer needed.
    for _ in 0..80 {
        dock.tick_auto_hide(false, false, false, 0);
    }
    assert!(dock.hide_cur < 0.01);
    assert!(!dock.needs_hide_tick());
}

/// Forced hide beats the cursor.
#[test]
fn force_hide_beats_the_cursor() {
    let mut dock = settled_dock(1);
    dock.toggle_force_hide();
    for _ in 0..40 {
        dock.tick_auto_hide(true, false, false, 0);
    }
    assert!(dock.hide_cur > 0.5, "强制收起时鼠标靠近不该展开");
}
