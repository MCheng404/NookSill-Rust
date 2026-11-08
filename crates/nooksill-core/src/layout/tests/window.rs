use super::*;

/// Positioning on all four edges.
///
/// All four are tested: a wrong edge only misplaces the window, it does not
/// crash.
#[test]
fn window_origin_places_the_dock_on_each_edge() {
    let work = Rect::new(0, 0, 1920, 1040);
    let (w, h) = (800, 100);

    // No reservation: flush with the work-area edge.
    assert_eq!(
        window_origin(Edge::Bottom, work, None, w, h),
        (560, 940),
        "底边：水平居中 + 贴底"
    );
    assert_eq!(window_origin(Edge::Top, work, None, w, h), (560, 0));
    assert_eq!(window_origin(Edge::Left, work, None, w, h), (0, 470));
    assert_eq!(window_origin(Edge::Right, work, None, w, h), (1120, 470));

    // A reservation carves a slice out of the inside of `rcWork`, sharing the
    // same outer edge, so the window position does not change; only whether
    // other windows may use that slice does.
    let bottom_bar = Rect::new(0, 948, 1920, 1040);
    assert_eq!(
        window_origin(Edge::Bottom, work, Some(bottom_bar), w, h),
        (560, 940),
        "正常预留不改变窗口位置，只改变 rcWork"
    );
    let top_bar = Rect::new(0, 0, 1920, 92);
    assert_eq!(
        window_origin(Edge::Top, work, Some(top_bar), w, h),
        (560, 0)
    );
    let left_bar = Rect::new(0, 0, 92, 1040);
    assert_eq!(
        window_origin(Edge::Left, work, Some(left_bar), w, h),
        (0, 470)
    );
    let right_bar = Rect::new(1828, 0, 1920, 1040);
    assert_eq!(
        window_origin(Edge::Right, work, Some(right_bar), w, h),
        (1120, 470),
        "右侧预留：窗口右边贴预留区右边"
    );
}

/// Uses the reserved rect when a neighbor toolbar squeezed it.
///
/// `ABM_QUERYPOS` adjusts the requested rect, so `ab.rc` is not always flush
/// with the work area; positioning from `rcWork` would overlap the neighbor.
#[test]
fn window_origin_prefers_the_reserved_rect() {
    let work = Rect::new(0, 0, 1920, 1040);
    // The AppBar only got a narrow strip.
    let squeezed = Rect::new(0, 1000, 1920, 1020);
    assert_eq!(
        window_origin(Edge::Bottom, work, Some(squeezed), 800, 100),
        (560, 920)
    );
}

/// Vertical edges transpose the window size, otherwise the dock is a sliver.
#[test]
fn physical_size_transposes_on_vertical_edges() {
    assert_eq!(physical_size(Edge::Bottom, 1181, 186), (1181, 186));
    assert_eq!(physical_size(Edge::Top, 1181, 186), (1181, 186));
    assert_eq!(physical_size(Edge::Left, 1181, 186), (186, 1181));
    assert_eq!(physical_size(Edge::Right, 1181, 186), (186, 1181));
}

/// Reserved thickness is bar height plus margin plus 4.
#[test]
fn reserve_thickness_matches_upstream() {
    // Measured: bar_h=76, margin=12 -> 92.
    assert_eq!(reserve_thickness(76.0, 12.0), 92);
    // Truncated, matching the `(int)` cast.
    assert_eq!(reserve_thickness(76.9, 12.9), 93);
}
