use super::*;

/// Fade-out saturates at the deadline instead of underflowing.
#[test]
fn notice_alpha_saturates_at_the_deadline() {
    assert_eq!(Dock::notice_alpha(0), 1.0);
    assert_eq!(Dock::notice_alpha(1900), 1.0, "淡出尚未开始");
    assert!((Dock::notice_alpha(2050) - 0.5).abs() < 0.01, "淡出中点");
    assert!(
        (Dock::notice_alpha(2199) - 1.0 / 300.0).abs() < 0.001,
        "淡出最后一毫秒"
    );
    assert_eq!(Dock::notice_alpha(2200), 0.0, "恰好到点");
    assert_eq!(Dock::notice_alpha(2200), 0.0, "恰好到点");
    assert_eq!(
        Dock::notice_alpha(5000),
        0.0,
        "越过时限必须饱和，这是当初下溢的位置"
    );
}

/// `argbc` takes alpha first.
#[test]
fn argbc_keeps_alpha_first() {
    let [r, g, b, a] = argbc(120, 0, 0, 0).to_srgba_unmultiplied();
    assert_eq!((r, g, b, a), (0, 0, 0, 120), "120 应当是 alpha");

    let [r, g, b, a] = argbc(255, 245, 245, 247).to_srgba_unmultiplied();
    assert_eq!((r, g, b, a), (245, 245, 247, 255));
}

/// Degenerate rects return `Shape::Noop` instead of panicking.
#[test]
fn rounded_gradient_tolerates_degenerate_rects() {
    let grad = BarGradient {
        from: Color32::WHITE,
        mid: None,
        to: Color32::BLACK,
        angle_deg: 0.0,
    };
    for (w, h) in [
        (-50.66668, 76.0),
        (120.0, -8.0),
        (0.0, 76.0),
        (f32::NAN, 76.0),
    ] {
        let rect = EguiRect::from_min_size(Pos2::ZERO, egui::vec2(w, h));
        assert!(matches!(
            rounded_gradient(rect, 12.0, BarGradient { ..grad }),
            egui::epaint::Shape::Noop
        ));
    }
    // A normal rect must still produce a mesh.
    let ok = EguiRect::from_min_size(Pos2::ZERO, egui::vec2(120.0, 76.0));
    assert!(!matches!(
        rounded_gradient(ok, 12.0, grad),
        egui::epaint::Shape::Noop
    ));
}

/// Card ink is dark on light themes, light on dark, always opaque.
#[test]
fn widget_ink_is_readable_in_both_themes() {
    let [r, g, b, a] = WidgetStyle::ink(false).to_srgba_unmultiplied();
    assert!(a >= 200, "浅色文字不该是半透明：alpha={a}");
    assert!(
        r < 100 && g < 100 && b < 100,
        "浅色文字应当是深色：{r},{g},{b}"
    );

    let [r, g, b, a] = WidgetStyle::ink(true).to_srgba_unmultiplied();
    assert!(a >= 200, "深色文字不该是半透明：alpha={a}");
    assert!(
        r > 200 && g > 200 && b > 200,
        "深色主题文字应当是亮色：{r},{g},{b}"
    );

    let [r, _, _, a] = WidgetStyle::ink_sub(false).to_srgba_unmultiplied();
    assert!(a >= 200 && r < 120, "次要文字也要有足够对比度");
    let [_, _, _, a] = WidgetStyle::rule(false).to_srgba_unmultiplied();
    assert!(a > 0 && a < 150, "细线是装饰，不该压过文字");
}

/// Widget font size is the same logical size at every DPI.
#[test]
fn widget_font_keeps_logical_size_across_dpi() {
    let mut sizes = Vec::new();
    for dpi in [96, 120, 144, 192] {
        let ppp = dpi as f32 / 96.0;
        let cfg = Config::defaults();
        let m = layout::calc_metrics(&cfg, dpi, 3, 0, LRect::new(0, 0, 2560, 1400));
        // Card logical height = physical / ppp; two rows, 4 px padding.
        let line_h = (m.bar_h / ppp - 8.0) / 2.0;
        sizes.push((
            clock_line_size(line_h, false),
            clock_line_size(line_h, true),
        ));
    }
    // 1 logical point tolerance: `bar_h` is rounded to whole physical
    // pixels, so dividing by ppp already jitters by +/- 0.5.
    let (time0, date0) = sizes[0];
    for (t, d) in &sizes {
        assert!((t - time0).abs() <= 1.0, "时间字号随 DPI 漂了：{sizes:?}");
        assert!((d - date0).abs() <= 1.0, "日期字号随 DPI 漂了：{sizes:?}");
    }
    assert!(time0 > date0, "时间应当比日期大一号：{sizes:?}");
    // At 150% the old code produced a 9 point font; it must stay larger.
    assert!(time0 > 14.0, "时间字号偏小：{time0}");
}

/// Item count can change between two layouts (duplicate, remove, drop), and the
/// next paint still runs against the previous `geom`. Painting must skip the
/// slots that no longer exist instead of indexing past the arrays.
#[test]
fn painting_tolerates_a_list_change_before_relayout() {
    let mut dock = settled_dock(3);
    // Duplicate the first item: four entries, the last keeps its carried size.
    let items: Vec<DockItem> = ["C:/x0.exe", "C:/x0.exe", "C:/x1.exe", "C:/x2.exe"]
        .into_iter()
        .map(|p| DockItem::from_item(&Item::new(ItemKind::File, p, p)))
        .collect();
    assert_eq!(dock.items.len(), 3);
    dock.replace_items(items);
    assert_eq!(dock.items.len(), 4, "列表已经变长，几何还是上一帧的");

    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        dock.paint(ui, 1.0, true, None, None);
    });
    // Dropping an unapplied texture delta panics inside epaint.
    out.textures_delta.clear();
}

/// The rounded gradient mesh keeps its outline away from the corners.
#[test]
fn rounded_gradient_leaves_the_corners_empty() {
    let rect = EguiRect::from_min_size(Pos2::new(100.0, 50.0), egui::vec2(200.0, 60.0));
    let r = 20.0_f32;
    let shape = rounded_gradient(rect, r, grad_lr(Color32::WHITE, Color32::BLACK));
    let egui::epaint::Shape::Mesh(mesh) = shape else {
        panic!("应当产出网格");
    };
    // Far more than 4 vertices means this is not the plain gradient path.
    assert!(mesh.vertices.len() > 20, "{} 个顶点", mesh.vertices.len());

    // Feathering insets inward, so no vertex leaves the rect.
    for v in &mesh.vertices {
        assert!(
            v.pos.x >= rect.left() - 0.01
                && v.pos.x <= rect.right() + 0.01
                && v.pos.y >= rect.top() - 0.01
                && v.pos.y <= rect.bottom() + 0.01,
            "顶点越界: {:?}",
            v.pos
        );
    }

    // No vertex near a corner; the arc keeps them clear.
    for (cx, cy) in [
        (rect.left(), rect.top()),
        (rect.right(), rect.top()),
        (rect.left(), rect.bottom()),
        (rect.right(), rect.bottom()),
    ] {
        let nearest = mesh
            .vertices
            .iter()
            .map(|v| ((v.pos.x - cx).powi(2) + (v.pos.y - cy).powi(2)).sqrt())
            .fold(f32::MAX, f32::min);
        assert!(
            nearest > 0.3 * r,
            "角点 ({cx},{cy}) 附近 {nearest:.2}px 有顶点，说明填充仍是方角"
        );
    }
}

fn grad_lr(from: Color32, to: Color32) -> BarGradient {
    BarGradient {
        from,
        mid: None,
        to,
        angle_deg: 0.0,
    }
}

/// A frosted piece samples its own slice of the window, not the whole texture.
///
/// This is the whole point of `Backdrop`: the dock draws the blur under the bar
/// and under each widget card separately, so the UV of a point has to be its
/// window fraction. Stretching the full texture into one piece would put the
/// desktop from under the *whole window* behind every card.
#[test]
fn frost_mesh_maps_window_fractions() {
    let frost = crate::frost::Backdrop {
        tex: egui::TextureId::User(7),
        win: [1225.0, 247.0],
    };
    let piece = EguiRect::from_min_size(Pos2::new(600.0, 100.0), egui::vec2(300.0, 60.0));
    let egui::epaint::Shape::Mesh(mesh) = rounded_frost(piece, 18.0, frost) else {
        panic!("应当产出网格");
    };
    assert_eq!(mesh.texture_id, frost.tex, "网格必须用磨砂纹理");
    // The first vertex is the fan centre.
    let center = mesh.vertices[0];
    assert!((center.uv.x - 750.0 / 1225.0).abs() < 1e-3, "中心 UV x");
    assert!((center.uv.y - 130.0 / 247.0).abs() < 1e-3, "中心 UV y");
    for v in &mesh.vertices {
        assert!(
            (0.0..=1.0).contains(&v.uv.x) && (0.0..=1.0).contains(&v.uv.y),
            "UV 越界: {:?}",
            v.uv
        );
    }
}

/// A square piece keeps its sub-rect: the quad covers just that slice.
#[test]
fn square_frost_piece_keeps_its_slice() {
    let frost = crate::frost::Backdrop {
        tex: egui::TextureId::User(1),
        win: [100.0, 50.0],
    };
    let piece = EguiRect::from_min_size(Pos2::new(10.0, 25.0), egui::vec2(20.0, 5.0));
    let egui::epaint::Shape::Mesh(mesh) = rounded_frost(piece, 0.0, frost) else {
        panic!("应当产出网格");
    };
    assert_eq!(mesh.vertices.len(), 4, "方角只用 4 个顶点");
    let has = |x: f32, y: f32| {
        mesh.vertices
            .iter()
            .any(|v| (v.uv.x - x).abs() < 1e-4 && (v.uv.y - y).abs() < 1e-4)
    };
    assert!(has(0.1, 0.5), "左上角 UV 应当是 (10/100, 25/50)");
    assert!(has(0.3, 0.6), "右下角 UV 应当是 (30/100, 30/50)");
}

/// A collapsed piece draws nothing instead of panicking.
#[test]
fn rounded_frost_tolerates_degenerate_rects() {
    let frost = crate::frost::Backdrop {
        tex: egui::TextureId::User(2),
        win: [800.0, 600.0],
    };
    for (w, h) in [
        (-50.66668, 76.0),
        (120.0, -8.0),
        (0.0, 76.0),
        (f32::NAN, 76.0),
    ] {
        let rect = EguiRect::from_min_size(Pos2::ZERO, egui::vec2(w, h));
        assert!(matches!(
            rounded_frost(rect, 12.0, frost),
            egui::epaint::Shape::Noop
        ));
    }
    // A window of zero extent would divide by zero; the guard must still draw.
    let tiny = crate::frost::Backdrop {
        tex: egui::TextureId::User(3),
        win: [0.0, 0.0],
    };
    let ok = EguiRect::from_min_size(Pos2::ZERO, egui::vec2(120.0, 76.0));
    let egui::epaint::Shape::Mesh(mesh) = rounded_frost(ok, 12.0, tiny) else {
        panic!("零尺寸窗口也要画出网格");
    };
    assert!(mesh
        .vertices
        .iter()
        .all(|v| v.uv.x.is_finite() && v.uv.y.is_finite()));
}

/// A three-stop gradient hits the mid colour exactly at t = 0.5.
#[test]
fn three_stop_gradient_hits_the_middle_color() {
    let red = Color32::from_rgb(220, 30, 40);
    let g = BarGradient {
        from: Color32::WHITE,
        mid: Some(red),
        to: Color32::BLACK,
        angle_deg: 0.0,
    };
    assert_eq!(g.at(0.0), Color32::WHITE);
    assert_eq!(g.at(0.5), red);
    assert_eq!(g.at(1.0), Color32::BLACK);
    // The quarter point is mid-way from white to red, not the mid colour.
    let q = g.at(0.25);
    assert!(q.r() > 200 && q.g() < 180, "{q:?}");
}

/// The angle decides which edge is the start colour.
#[test]
fn angle_controls_which_edge_is_the_start() {
    let rect = EguiRect::from_min_size(Pos2::ZERO, egui::vec2(100.0, 100.0));
    let white = Color32::WHITE;
    let black = Color32::BLACK;

    let edge_color =
        |grad: BarGradient, pick: fn(&egui::epaint::Vertex, &egui::epaint::Vertex) -> bool| {
            let egui::epaint::Shape::Mesh(mesh) = rounded_gradient(rect, 8.0, grad) else {
                panic!("应当产出网格");
            };
            let solid: Vec<_> = mesh.vertices.iter().filter(|v| v.color.a() > 200).collect();
            assert!(!solid.is_empty());
            let mut best = solid[0];
            for v in &solid {
                if pick(v, best) {
                    best = v;
                }
            }
            best.color
        };

    // Leftmost and topmost opaque vertices.
    let left = edge_color(grad_lr(white, black), |v, b| v.pos.x < b.pos.x);
    let top = edge_color(
        BarGradient {
            angle_deg: 90.0,
            ..grad_lr(white, black)
        },
        |v, b| v.pos.y < b.pos.y,
    );
    assert!(left.r() > 240, "0° 时最左边应当是起点色：{left:?}");
    assert!(top.r() > 240, "90° 时最上边应当是起点色：{top:?}");

    // Rightmost and bottommost should be the end colour.
    let right = edge_color(grad_lr(white, black), |v, b| v.pos.x > b.pos.x);
    let bottom = edge_color(
        BarGradient {
            angle_deg: 90.0,
            ..grad_lr(white, black)
        },
        |v, b| v.pos.y > b.pos.y,
    );
    assert!(right.r() < 20, "0° 时最右边应当是终点色：{right:?}");
    assert!(bottom.r() < 20, "90° 时最下边应当是终点色：{bottom:?}");
}

/// Each stop alpha applies independently; A3 = 0 makes the mid stop clear.
#[test]
fn per_stop_alpha_is_applied() {
    let g = BarGradient {
        from: Color32::from_rgba_unmultiplied(255, 255, 255, 255),
        mid: Some(Color32::from_rgba_unmultiplied(255, 0, 0, 0)),
        to: Color32::from_rgba_unmultiplied(0, 0, 0, 255),
        angle_deg: 0.0,
    };
    assert_eq!(g.at(0.5).a(), 0, "A3=0 的中间色应当全透明");
}

/// Tiny radii fall back to the 4-vertex plain gradient.
#[test]
fn tiny_radius_falls_back_to_plain_gradient() {
    let rect = EguiRect::from_min_size(Pos2::ZERO, egui::vec2(10.0, 10.0));
    let shape = rounded_gradient(rect, 0.0, grad_lr(Color32::WHITE, Color32::BLACK));
    let egui::epaint::Shape::Mesh(mesh) = shape else {
        panic!("应当产出网格");
    };
    assert_eq!(mesh.vertices.len(), 4, "方角渐变只有 4 个顶点");
}

/// Gradient vertices take their colour from the direction.
#[test]
fn gradient_endpoints_follow_direction() {
    let rect = EguiRect::from_min_size(Pos2::ZERO, egui::vec2(100.0, 40.0));
    let white = Color32::from_rgb(255, 255, 255);
    let black = Color32::from_rgb(0, 0, 0);
    let shape = rounded_gradient(rect, 8.0, grad_lr(white, black));
    let egui::epaint::Shape::Mesh(mesh) = shape else {
        panic!("应当产出网格");
    };
    // The feather ring is transparent by design; ignore it when comparing.
    let solid: Vec<_> = mesh.vertices.iter().filter(|v| v.color.a() > 200).collect();
    let leftmost = solid
        .iter()
        .min_by(|a, b| a.pos.x.total_cmp(&b.pos.x))
        .unwrap();
    let rightmost = solid
        .iter()
        .max_by(|a, b| a.pos.x.total_cmp(&b.pos.x))
        .unwrap();
    assert!(leftmost.color.r() > 240, "左端应当接近白色");
    assert!(rightmost.color.r() < 15, "右端应当接近黑色");
}

// Separator resize.
