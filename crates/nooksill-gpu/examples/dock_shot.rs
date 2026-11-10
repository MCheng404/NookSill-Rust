//! Renders the dock offscreen to a BMP, for effects only visible when running:
//! reflection, border, gradient.
//!
//! ```text
//! cargo run -p nooksill-gpu --example dock_shot -- out.bmp
//! cargo run -p nooksill-gpu --example dock_shot -- out.bmp light
//! ```
//!
//! Same render path as the app (`FrameRenderer` + `Dock::paint`), minus
//! `UpdateLayeredWindow`.
//!
//! In the real window the presenter applies the reflection straight to the DIB.
//! Offscreen there is no DIB, so the same `mirror_rows` runs on the readback
//! buffer: RGBA instead of BGRA, but every channel is scaled equally.

#[path = "common/bmp.rs"]
mod bmp;

use nooksill_core::config::{Config, Key};
use nooksill_core::layout::Rect as LRect;
use nooksill_gpu::dock::{Dock, DockItem};
use nooksill_gpu::frame::FrameRenderer;
use nooksill_gpu::gpu::Gpu;
use nooksill_gpu::present::mirror_rows;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    nooksill_core::i18n::configure("", nooksill_platform::locale::ui_language_id());
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| "dock.bmp".to_owned());
    let dark = args.next().as_deref() != Some("light");
    // `grad` enables the third background colour and the angle, to eyeball
    // those two keys.
    let demo_grad = args.next().as_deref() == Some("grad");
    // `insert` simulates an external file dropped between cells 2 and 3.
    let show_insert = args.next().as_deref() == Some("insert");
    let ppp = 1.25_f32;

    let mut cfg = Config::defaults();
    cfg.reflect = true;
    cfg.apply_i32(Key::REFL_SIZE, 55);
    cfg.apply_i32(Key::REFL_GAP, 6);
    cfg.apply_i32(Key::REFL_OPACITY, 55);
    cfg.apply_i32(Key::BORDER_WIDTH, 3);
    if demo_grad {
        cfg.apply_i32(Key::BG_COLOR3, 0x3A7BD5); // middle stop: blue
        cfg.apply_i32(Key::GRAD_ANGLE, 30);
        cfg.apply_i32(Key::GRAD_A1, 100);
        cfg.apply_i32(Key::GRAD_A2, 55);
        cfg.apply_i32(Key::GRAD_A3, 90);
        cfg.apply_i32(Key::BgMode, 1);
    }
    cfg.border = true;
    cfg.theme = nooksill_core::config::DockTheme::Dark;
    cfg.mode = nooksill_core::config::DockMode::Always;

    // Names drive the first-letter icons; the paths only need to look real.
    let items: Vec<DockItem> = [
        ("资源管理器", r"C:\Windows\explorer.exe"),
        ("记事本", r"C:\Windows\System32\notepad.exe"),
        ("画图", r"C:\Windows\System32\mspaint.exe"),
        ("计算器", r"C:\Windows\System32\calc.exe"),
    ]
    .iter()
    .map(|(name, path)| {
        DockItem::from_item(&nooksill_core::item::Item::new(
            nooksill_core::item::ItemKind::File,
            *name,
            *path,
        ))
    })
    .collect();

    let work = LRect::new(0, 0, 1280, 900);
    let mut dock = Dock::new(cfg.clone(), items, work);
    // Let the entrance animation settle: `from_item` starts `cur` at 0, so the
    // first frame would show icons still growing out of nothing.
    for _ in 0..120 {
        dock.update(120, nooksill_core::layout::REF_FRAME_MS);
    }

    let (w, h) = dock.physical_size_i32();
    let (w, h) = (w as u32, h as u32);
    if show_insert && dock.geom.act_c.len() >= 3 {
        let u = (dock.geom.act_c[1] + dock.geom.act_c[2]) * 0.5;
        let v = dock.geom.bar_top + 10.0;
        dock.set_external_drop(u, v);
        // One more frame so the reserved slot and the slide compensation apply.
        dock.update(120, nooksill_core::layout::REF_FRAME_MS);
        println!(
            "动态插入位：第 {} 项之前，标记 x={:.1}",
            dock.external_drop().unwrap_or(0),
            u
        );
    }
    let gpu = Gpu::new("")?;
    let mut frame = FrameRenderer::new(&gpu, w, h, ppp)?;
    // The clock card has its own paint path, so it is worth covering here.
    cfg.clock = true;
    let now = nooksill_core::clock::ClockParts {
        year: 2026,
        month: 11,
        day: 7,
        weekday: 5,
        hour: 0,
        minute: 10,
        second: 0,
    };
    let lines = nooksill_core::clock::clock_lines(&cfg, &now);
    let clock = Some(lines.as_slice());
    let (rgba, _) = frame.render_rgba(&gpu, raw_input(w, h, ppp), |ui| {
        dock.paint(ui, ppp, dark, clock);
    })?;

    // Exactly what the presenter does in the real window.
    let mut pixels = rgba.clone();
    if let Some(spec) = dock.reflection_spec() {
        let stride = w as usize;
        mirror_rows(&mut pixels, stride, h as i32, spec);
        println!(
            "倒影：src_top={} src_h={} gap={} height={} opacity={:.2}",
            spec.src_top, spec.src_h, spec.gap, spec.height, spec.opacity
        );
    }
    let bg = if dark { [18, 19, 23] } else { [238, 239, 243] };
    bmp::write_bmp(&path, w, h, &pixels, move |_, _| bg)?;
    println!("已写出 {path}（{w}×{h}）");
    Ok(())
}

fn raw_input(w: u32, h: u32, ppp: f32) -> egui::RawInput {
    let mut raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(w as f32 / ppp, h as f32 / ppp),
        )),
        time: Some(10.0),
        ..Default::default()
    };
    raw.viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo {
            native_pixels_per_point: Some(ppp),
            ..Default::default()
        },
    );
    raw
}
