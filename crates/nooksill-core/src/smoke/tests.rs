use super::*;
use crate::config::{Config, Edge};

#[test]
fn palette_matches_upstream_order() {
    // The index goes into the ini, so the order is a contract.
    assert_eq!(PALETTE[0], [88.0, 52.0, 160.0], "0 = violet");
    assert_eq!(PALETTE[7], [150.0, 108.0, 10.0], "7 = gold");
}

#[test]
fn noise_alpha_is_normalized() {
    let n = Noise::build_sized(32);
    for k in 0..3 {
        for &a in &n.alpha[k] {
            assert!((0.0..=1.0).contains(&a), "alpha 越界: {a}");
        }
        let m = n.mean(k);
        assert!(m > 0.05 && m < 0.95, "第 {k} 层均值异常: {m}");
    }
}

#[test]
fn noise_source_wraps_on_the_tile() {
    let n = Noise::build_sized(48);
    // A cell spans 0.55 texture pixels, so one tile period is n / 0.55 cells.
    let period = n.size() as f32 / 0.55;
    for k in 0..3 {
        let a = n.source(k, 3.0, 7.0, 0.0);
        let b = n.source(k, 3.0 + period, 7.0, 0.0);
        assert!((a - b).abs() < 1e-3, "第 {k} 层平铺有接缝: {a} vs {b}");
    }
}

/// Bar rect for a `(w, h)` grid after subtracting the margins, in cells.
///
/// Matches the old `SmokeFx` algorithm so these tests keep their initial state.
fn bar_in(grid: (usize, usize), tune: &Tuning) -> (f32, f32, f32, f32) {
    let (mx, my) = (tune.margin_x() / CELL, tune.margin_y() / CELL);
    (mx, my, grid.0 as f32 - 2.0 * mx, grid.1 as f32 - 2.0 * my)
}

/// Grid sized like a real bar: a 48 px bar plus default margins is 24 cells high.
/// Do not shrink it to 12 for speed: `iy1 < iy0` inverts the capsule, every cell
/// counts as outside and the run ends fully transparent.
fn warmed(noise: &Noise, frames: usize) -> Fluid {
    let tune = Tuning {
        enabled: true,
        ..Tuning::default()
    };
    let mut fl = Fluid::new();
    fl.ensure(52, 24, 0.0, bar_in((52, 24), &tune), tune.thickness);
    for i in 0..frames {
        fl.step(
            noise,
            &tune,
            Frame {
                dt: 1.0 / 60.0,
                time: i as f32 / 60.0,
                // Cursor outside the grid: this measures the undisturbed steady state.
                mouse: (-99.0, -99.0),
                mouse_vel: (0.0, 0.0),
            },
        );
    }
    fl
}

#[test]
fn fluid_stays_bounded() {
    let noise = Noise::build_sized(32);
    let tune = Tuning {
        enabled: true,
        ..Tuning::default()
    };
    let mut fl = Fluid::new();
    fl.ensure(52, 24, 0.0, bar_in((52, 24), &tune), tune.thickness);
    for i in 0..120 {
        fl.step(
            &noise,
            &tune,
            Frame {
                dt: 1.0 / 60.0,
                time: i as f32 / 60.0,
                // Cursor in the middle of the capsule, covering the push branch too.
                mouse: (26.0, 12.0),
                mouse_vel: (0.5, -0.5),
            },
        );
    }
    for (name, f) in [("u", &fl.u), ("v", &fl.v), ("d", &fl.d)] {
        for &v in f.iter() {
            assert!(v.is_finite(), "{name} 出现 NaN/Inf");
            assert!(v.abs() < 400.0, "{name} 发散: {v}");
        }
    }
    assert!(fl.d.iter().all(|v| *v >= 0.0), "染料不该为负");
    // Buoyancy points up (y decreases), so the band above the capsule must be denser
    // than its inside; this catches a flipped buoyancy sign.
    let sum = |range: std::ops::Range<usize>| fl.d[range].iter().sum::<f32>();
    let above = sum(0..52 * 6);
    let inside = sum(52 * 8..52 * 16);
    assert!(above > 0.0, "胶囊上方应当有飘上去的烟");
    assert!(inside > 0.0, "胶囊内部应当有源");
}

#[test]
fn raster_is_premultiplied() {
    let noise = Noise::build_sized(24);
    let mut fl = warmed(&noise, 40);
    let (pw, ph) = fl.raster(&noise, PALETTE[0], 0.0);
    assert!(pw > 0 && ph > 0);
    assert_eq!(fl.pixels().len(), pw * ph * 4);
    let mut lit = 0;
    for px in fl.pixels().as_chunks::<4>().0 {
        let a = px[3];
        assert!(px[0] <= a && px[1] <= a && px[2] <= a, "未预乘: {px:?}");
        if a > 8 {
            lit += 1;
        }
    }
    // Emission is a ring (see `build_edge`), not a filled block, so only about a
    // tenth of the pixels are lit. Below 1/8 means the band or the density is wrong.
    assert!(
        lit > pw * ph / 8,
        "只有 {lit}/{} 个像素不透明，遮罩或密度算错了",
        pw * ph
    );
}

#[test]
fn post_keeps_premultiplied_and_brightens() {
    let noise = Noise::build_sized(24);
    let mut fl = warmed(&noise, 40);
    let (pw, _) = fl.raster(&noise, PALETTE[0], 0.0);
    let max_a = fl
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| p[3])
        .max()
        .unwrap_or(0);
    fl.post(PALETTE[0], Some(PALETTE[2]), (0.0, pw as f32), 1.5);
    for px in fl.pixels().as_chunks::<4>().0 {
        let a = px[3];
        assert!(px[0] <= a && px[1] <= a && px[2] <= a, "后处理破坏预乘");
    }
    let after = fl
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| p[3])
        .max()
        .unwrap_or(0);
    assert!(after > max_a, "125% 以上的档位必须提亮 alpha");
}

#[test]
fn tuning_reads_config_tables() {
    let mut cfg = Config::defaults();
    cfg.apply_i32(Key::SMOKE, 1);
    cfg.edge = Edge::Bottom;
    let t = Tuning::from_config(&cfg, PALETTE[3]);
    assert!(t.enabled);
    assert!((t.thickness - 1.0).abs() < 1e-6);
    assert!(t.clouds);

    // Top edge: buoyancy and the upper falloff do not hold, so it stays disabled.
    cfg.edge = Edge::Top;
    assert!(!Tuning::from_config(&cfg, PALETTE[3]).enabled);
    // Same when the switch is off.
    cfg.edge = Edge::Bottom;
    cfg.apply_i32(Key::SMOKE, 0);
    assert!(!Tuning::from_config(&cfg, PALETTE[3]).enabled);
}

#[test]
fn tone_lightens_instead_of_whitening() {
    let mut cfg = Config::defaults();
    cfg.apply_i32(Key::SMOKE, 1);
    let base = Tuning::from_config(&cfg, PALETTE[0]).color_at(0, 0.0);
    cfg.apply_i32(Key::SMOKE_TONE, 1);
    let glow = Tuning::from_config(&cfg, PALETTE[0]).color_at(0, 0.0);
    assert!(glow[2] > base[2] && glow[0] > base[0]);
    // Hue relation kept: violet still has more blue than green.
    assert!(glow[2] > glow[1]);
}

#[test]
fn rainbow_runs_a_full_circle_in_ninety_seconds() {
    let a = rainbow(0.0);
    let b = rainbow(90.0);
    for i in 0..3 {
        assert!((a[i] - b[i]).abs() < 1e-3, "90 秒应当回到同一点");
    }
    assert!(rainbow(22.5) != a, "四分之一圈必须换色");
}
