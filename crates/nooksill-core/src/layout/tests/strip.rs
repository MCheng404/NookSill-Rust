use super::*;

/// A hole in the strip resolves to `None` instead of panicking.
#[test]
fn sparse_widget_strip_does_not_panic() {
    let mut items = [None; 7];
    items[0] = Some((WidgetKind::Clock, 100));
    items[1] = None; // hole
    items[2] = Some((WidgetKind::Resource, 90));
    let strip = WidgetStrip {
        items,
        len: 3,
        extra: 0.0,
        on_left: false,
        gap_extra: 6.0,
    };

    // i=0 returns before reading the hole.
    assert_eq!(strip.left_of(0, 500.0, 200.0, 8.0), Some(714.0));
    assert_eq!(strip.left_of(2, 500.0, 200.0, 8.0), None);
    assert_eq!(strip.left_of(9, 500.0, 200.0, 8.0), None);

    // Left-side layout walks backwards; only the leftmost slot sees the hole.
    let left = WidgetStrip {
        on_left: true,
        ..strip
    };
    assert_eq!(left.left_of(2, 500.0, 200.0, 8.0), Some(396.0));
    assert_eq!(left.left_of(0, 500.0, 200.0, 8.0), None);
}

/// Inserting a gap shifts icons on both sides apart, opening one icon width.
#[test]
fn insert_slot_pushes_icons_apart() {
    let m = calc_metrics(&cfg96(), 96, 0, 0, work());
    let cur = vec![48.0; 4];
    let plain = place_slots(&m, &cur, 0.0);
    let with = place_slots_insert(&m, &cur, 0.0, Some((2, 48.0)));
    let extra = 48.0 + m.gap;
    // A wider bar shifts everything left, and icons right of the gap move one more slot.
    // "Right offset - left offset" cancels the centering shift and must equal the gap.
    let left = with.slot_l[0] - plain.slot_l[0];
    let right = with.slot_l[2] - plain.slot_l[2];
    assert!((right - left - extra).abs() < 0.01, "两侧没有让出位置");
    assert!((with.slot_l[1] - plain.slot_l[1] - left).abs() < 0.01);
    assert!((with.slot_l[3] - plain.slot_l[3] - right).abs() < 0.01);
    assert!(plain.insert_l.is_none());
    assert!(with.insert_l.is_some(), "绘制层要拿到插入标记位置");
}

#[test]
fn widget_strip_larger_than_window_never_yields_negative_bar_width() {
    let m = calc_metrics(&cfg96(), 96, 0, 0, work());
    let slots = place_slots(&m, &[], m.lw as f32 + 200.0);
    assert!(slots.bar_w >= 0.0, "bar_w 不能为负：{}", slots.bar_w);
    assert!(slots.bar_w.is_finite());

    // Same with items present.
    let slots = place_slots(&m, &[48.0, 48.0], m.lw as f32 + 200.0);
    assert!(slots.bar_w >= 0.0, "bar_w 不能为负：{}", slots.bar_w);
}

#[test]
fn widget_gap_shifts_strip_consistently() {
    let mut cfg = cfg96();
    let mut build = |gap: i32| {
        cfg.apply_i32(Key::WIDGET_GAP, gap);
        WidgetStrip::build(&cfg, 96, 76.0, 8.0, 166)
    };
    let a = build(6);
    let b = build(18);
    assert!(b.gap_extra > a.gap_extra);
    assert!(b.extra > a.extra, "占位要随间距变大");
    // Right-side layout: the first member's left edge moves by the same delta.
    let la = a.left_of(0, 500.0, 200.0, 8.0).expect("条带非空");
    let lb = b.left_of(0, 500.0, 200.0, 8.0).expect("条带非空");
    assert!((lb - la - (b.gap_extra - a.gap_extra)).abs() < 0.01);
}

/// With only the clock enabled, the strip must equal the pre-strip layout.
#[test]
fn widget_strip_matches_v140_when_only_clock() {
    let mut c = cfg96();
    c.clock = true;
    // Disable every other implemented card first: defaults enable `ResMon`,
    // `ToolW`, `CtrlCenter` and `TrayW`, and those did not exist before the strip.
    c.apply_i32(Key::RES_MON, 0);
    c.apply_i32(Key::TOOL_W, 0);
    c.apply_i32(Key::CTRL_CENTER, 0);
    c.apply_i32(Key::TRAY_W, 0);
    let m = calc_metrics(&c, 96, 3, 0, work());

    // The strip holds the clock alone.
    assert_eq!(m.widgets.len, 1);
    assert_eq!(m.widgets.width_of(WidgetKind::Clock), Some(166));

    // Pre-strip value: `clock_w + gap + 6`.
    let legacy = m.clock_w as f32 + m.gap + 6.0;
    assert!(
        (m.widget_extra - legacy).abs() < 0.01,
        "条带占位 {} 与 v1.4.0 的 {} 不一致",
        m.widget_extra,
        legacy
    );
    assert!((m.widget_extra - m.clock_w as f32 - m.gap - 6.0).abs() < 0.01);
}

/// Disabled clock: empty strip, zero reserved space.
#[test]
fn widget_strip_is_empty_without_clock() {
    let mut c = cfg96();
    // All implemented cards default to on, so disable them all.
    c.apply_i32(Key::RES_MON, 0);
    c.apply_i32(Key::TOOL_W, 0);
    c.apply_i32(Key::CTRL_CENTER, 0);
    c.apply_i32(Key::TRAY_W, 0);
    let m = calc_metrics(&c, 96, 3, 0, work());
    assert_eq!(m.widgets.len, 0);
    assert_eq!(m.widget_extra, 0.0);
    assert!(m.widgets.width_of(WidgetKind::Clock).is_none());
}

/// Unimplemented widgets stay out of the strip, or the bar keeps a blank gap.
///
/// Defaults enable control center, resources, tool and focus. The two without a
/// renderer (notify, focus) must not reserve space even when their key is 1.
#[test]
fn unimplemented_widgets_do_not_take_space() {
    let mut c = cfg96();
    c.clock = true;
    // These are the defaults.
    assert_eq!(c.get_i32(Key::CTRL_CENTER), 1);
    assert_eq!(c.get_i32(Key::TOOL_W), 1);
    assert_eq!(c.get_i32(Key::MODE_W), 1);

    let m = calc_metrics(&c, 96, 3, 0, work());
    // All five implemented cards enter the strip.
    assert_eq!(
        m.widgets.len, 5,
        "时钟 + 控制中心 + 资源监视器 + 计时器 + 托盘"
    );
    assert!(m.widgets.width_of(WidgetKind::CtrlCenter).is_some());
    assert!(m.widgets.width_of(WidgetKind::Resource).is_some());
    assert!(m.widgets.width_of(WidgetKind::Tool).is_some());
    assert!(m.widgets.width_of(WidgetKind::Tray).is_some());
    // Neither of these has a renderer yet.
    assert!(m.widgets.width_of(WidgetKind::Notify).is_none());
    assert!(m.widgets.width_of(WidgetKind::Focus).is_none());
}

/// The resource card joins the strip when enabled.
#[test]
fn resource_widget_enters_the_strip() {
    let mut c = cfg96();
    c.clock = true;
    c.apply_i32(Key::RES_MON, 1);
    c.apply_i32(Key::TOOL_W, 0);
    c.apply_i32(Key::CTRL_CENTER, 0);
    c.apply_i32(Key::TRAY_W, 0);
    let m = calc_metrics(&c, 96, 3, 0, work());

    assert_eq!(m.widgets.len, 2, "时钟 + 资源监视器");
    assert!(m.widgets.width_of(WidgetKind::Resource).is_some());
    // Order: clock first, resource after control center (disabled here).
    let kinds: Vec<WidgetKind> = m.widgets.iter().map(|(k, _)| k).collect();
    assert_eq!(kinds[0], WidgetKind::Clock);
    assert_eq!(kinds[1], WidgetKind::Resource);
}

/// Tray is always last, width is 0.72 of the bar height.
#[test]
fn tray_widget_is_last_and_fixed_width() {
    let c = cfg96();
    assert_eq!(c.get_i32(Key::TRAY_W), 1, "1.6.5 起默认开");

    let m = calc_metrics(&c, 96, 3, 0, work());
    let kinds: Vec<WidgetKind> = m.widgets.iter().map(|(k, _)| k).collect();
    assert_eq!(
        kinds.last().copied(),
        Some(WidgetKind::Tray),
        "托盘应当在条带最外侧"
    );

    let bar_h = m.bar_h;
    assert_eq!(
        m.widgets.width_of(WidgetKind::Tray),
        Some((bar_h * 0.72) as i32)
    );
    // `WOrder` only swaps clock and notify.
    let mut swapped = c;
    swapped.apply_i32(Key::W_ORDER, 1);
    let m2 = calc_metrics(&swapped, 96, 3, 0, work());
    let kinds2: Vec<WidgetKind> = m2.widgets.iter().map(|(k, _)| k).collect();
    assert_eq!(kinds2.last().copied(), Some(WidgetKind::Tray));
}

/// Tool card width is 0.72 of the bar height, 1.6 when double wide.
#[test]
fn tool_widget_enters_the_strip() {
    let mut c = cfg96();
    c.apply_i32(Key::TOOL_W, 1);
    c.apply_i32(Key::RES_MON, 0);

    let m = calc_metrics(&c, 96, 3, 0, work());
    // Use the real bar height: `s + 2*pad + 4`.
    let bar_h = m.bar_h;
    assert_eq!(
        m.widgets.width_of(WidgetKind::Tool),
        Some((bar_h * 0.72) as i32)
    );

    // `WideMask` bit 4 = tool.
    c.apply_i32(Key::WIDE_MASK, 1 << 4);
    assert_eq!(WidgetKind::Tool.width(&c, bar_h), (bar_h * 1.6) as i32);
}

/// Resource card width is 1.16 of the bar height, 2.2 when double wide.
#[test]
fn resource_widget_width_follows_upstream() {
    let mut c = cfg96();
    c.apply_i32(Key::RES_MON, 1);
    let bar_h = 76.0_f32;
    assert_eq!(WidgetKind::Resource.width(&c, bar_h), (bar_h * 1.16) as i32);
    // `WideMask` bit 3 = resource.
    c.apply_i32(Key::WIDE_MASK, 1 << 3);
    assert_eq!(WidgetKind::Resource.width(&c, bar_h), (bar_h * 2.2) as i32);
}

/// Disabling `ResMon` keeps the resource card out of the strip.
#[test]
fn resource_widget_respects_config() {
    let mut c = cfg96();
    c.clock = true;
    c.apply_i32(Key::RES_MON, 0);
    c.apply_i32(Key::TOOL_W, 0);
    c.apply_i32(Key::CTRL_CENTER, 0);
    c.apply_i32(Key::TRAY_W, 0);
    let m = calc_metrics(&c, 96, 3, 0, work());
    assert_eq!(m.widgets.len, 1);
    assert!(m.widgets.width_of(WidgetKind::Resource).is_none());
}

/// Right side: the first member sits `gap + 6` past the bar's right edge.
#[test]
fn widget_strip_positions_on_the_right() {
    let mut c = cfg96();
    c.clock = true;
    let m = calc_metrics(&c, 96, 3, 0, work());
    let strip = &m.widgets;
    let bar_l = 100.0_f32;
    let bar_w = 300.0_f32;

    let left = strip.left_of(0, bar_l, bar_w, m.gap).expect("应当有位置");
    assert!(
        (left - (bar_l + bar_w + m.gap + 6.0)).abs() < 0.01,
        "左侧条带的第一个成员应当紧贴栏体右缘，实际 {left}"
    );
}

/// Left side (`WSide=1`): laid out leftward from the bar's left edge.
#[test]
fn widget_strip_positions_on_the_left() {
    let mut c = cfg96();
    c.clock = true;
    // Clock only, so the relation to the bar edge stays simple.
    c.apply_i32(Key::RES_MON, 0);
    c.apply_i32(Key::TOOL_W, 0);
    c.apply_i32(Key::CTRL_CENTER, 0);
    c.apply_i32(Key::TRAY_W, 0);
    c.apply_i32(Key::W_SIDE, 1);
    let m = calc_metrics(&c, 96, 3, 0, work());
    assert!(m.widgets.on_left, "应当记为左侧条带");

    let bar_l = 100.0_f32;
    let bar_w = 300.0_f32;
    let left = m
        .widgets
        .left_of(0, bar_l, bar_w, m.gap)
        .expect("应当有位置");
    // Left side: `u = bar_l - gap_w`, then subtract the member's own width.
    let want = bar_l - (m.gap + 6.0) - m.clock_w as f32;
    assert!(
        (left - want).abs() < 0.01,
        "左侧条带位置 {left} 与预期 {want} 不符"
    );
}

/// On the left, the last member is the one nearest the bar.
#[test]
fn widget_strip_left_side_order_is_reversed() {
    let mut c = cfg96();
    c.clock = true;
    c.apply_i32(Key::RES_MON, 1);
    c.apply_i32(Key::TOOL_W, 0);
    c.apply_i32(Key::CTRL_CENTER, 0);
    c.apply_i32(Key::TRAY_W, 0);
    c.apply_i32(Key::W_SIDE, 1);
    let m = calc_metrics(&c, 96, 3, 0, work());
    let bar_l = 100.0_f32;
    let bar_w = 300.0_f32;

    // Strip order is clock (0), resource (1); on the left the last one hugs the bar.
    let clock = m.widgets.left_of(0, bar_l, bar_w, m.gap).expect("时钟");
    let res = m.widgets.left_of(1, bar_l, bar_w, m.gap).expect("资源");
    assert!(
        res > clock,
        "左侧条带里资源卡片({res})应当在时钟({clock})右边、贴着栏体"
    );
    // The resource card's right edge sits `gap + 6` from the bar's left edge.
    let res_w = m.widgets.width_of(WidgetKind::Resource).expect("宽度");
    let gap_w = m.gap + 6.0;
    assert!((res + res_w as f32 - (bar_l - gap_w)).abs() < 0.01);
}

/// `WSide` moves the strip but does not change its reserved width.
#[test]
fn widget_side_does_not_change_extra() {
    let mut c = cfg96();
    c.clock = true;
    let right = calc_metrics(&c, 96, 3, 0, work());
    c.apply_i32(Key::W_SIDE, 1);
    let left = calc_metrics(&c, 96, 3, 0, work());
    assert!(
        (right.widget_extra - left.widget_extra).abs() < 0.01,
        "左右两侧的占位应当相同"
    );
    assert_ne!(right.widgets.on_left, left.widgets.on_left);
}

/// Card widths are fixed fractions of the bar height.
///
/// The clock is not included: its width follows the content. The other formulas
/// are checked against the upstream factors.
#[test]
fn widget_widths_follow_upstream_formulas() {
    let c = cfg96();
    let bar_h = 76.0_f32;
    assert_eq!(WidgetKind::Notify.width(&c, bar_h), (bar_h * 0.92) as i32);
    assert_eq!(
        WidgetKind::CtrlCenter.width(&c, bar_h),
        (bar_h * 0.72) as i32
    );
    assert_eq!(WidgetKind::Resource.width(&c, bar_h), (bar_h * 1.16) as i32);
    assert_eq!(WidgetKind::Tool.width(&c, bar_h), (bar_h * 0.72) as i32);
    assert_eq!(WidgetKind::Focus.width(&c, bar_h), (bar_h * 0.72) as i32);
}

/// Double width comes from the `WideMask` bit at the enum index.
#[test]
fn wide_mask_doubles_widget_width() {
    let mut c = cfg96();
    let bar_h = 76.0_f32;
    assert_eq!(WidgetKind::Resource.width(&c, bar_h), (bar_h * 1.16) as i32);
    // Bit 3 = resource (clock=0, notify=1, control center=2, resource=3).
    c.apply_i32(Key::WIDE_MASK, 1 << 3);
    assert_eq!(WidgetKind::Resource.width(&c, bar_h), (bar_h * 2.2) as i32);
    // Other bits leave it alone.
    c.apply_i32(Key::WIDE_MASK, 1 << 5);
    assert_eq!(WidgetKind::Resource.width(&c, bar_h), (bar_h * 1.16) as i32);
}
