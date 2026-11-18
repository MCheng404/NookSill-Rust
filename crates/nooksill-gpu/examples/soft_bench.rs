//! What the CPU rasterizer costs per frame -- the number ADR-119 was missing.
//!
//! ```text
//! cargo run -p nooksill-gpu --release --example soft_bench -- [ppp] [icons]
//! ```
//!
//! `mem_nogpu` answers "how much memory does a GPU-free dock hold"; it stands in
//! a memset for the raster step and so says nothing about speed. This probe runs
//! the real `SoftRenderer` over a textured, rounded, zoomed dock and times each
//! stage separately, because the stages have different owners: tessellation is
//! egui's, raster is ours, present is Win32's.
//!
//! Textured icons are the point. An untextured bar is mostly flat fills that hit
//! the blend fast path; icons force bilinear sampling plus the sRGB decode and
//! encode tables on every covered pixel, which is where a scanline rasterizer
//! actually spends its time.

use nooksill_core::layout::{self, Rect as LRect};
use nooksill_gpu::present::LayeredPresenter;
use nooksill_gpu::soft::SoftRenderer;
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassW, SetWindowPos, ShowWindow, SWP_NOACTIVATE,
    SWP_NOZORDER, SW_SHOWNOACTIVATE, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT, WS_POPUP,
};

#[path = "common/surfaces.rs"]
mod surfaces;

use surfaces::{mb, sample};

/// One icon's worth of atlas: RGBA noise, so no pixel compresses or shortcuts.
fn icon_image(side: usize) -> egui::ColorImage {
    let mut px = vec![egui::Color32::TRANSPARENT; side * side];
    for (i, p) in px.iter_mut().enumerate() {
        let x = i % side;
        let y = i / side;
        // A soft disc with an opaque core: both blend paths get exercised.
        let dx = x as f32 - side as f32 / 2.0;
        let dy = y as f32 - side as f32 / 2.0;
        let d = (dx * dx + dy * dy).sqrt() / (side as f32 / 2.0);
        let a = ((1.0 - d).clamp(0.0, 1.0) * 255.0) as u8;
        *p = egui::Color32::from_rgba_premultiplied(
            ((x * 255 / side) as u8).min(a),
            ((y * 255 / side) as u8).min(a),
            160_u8.min(a),
            a,
        );
    }
    egui::ColorImage {
        size: [side, side],
        pixels: px,
        source_size: egui::Vec2::splat(side as f32),
    }
}

fn raw(w: u32, h: u32, ppp: f32) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(w as f32 / ppp, h as f32 / ppp),
        )),
        ..Default::default()
    }
}

fn make_window(width: i32, height: i32) -> Result<HWND, Box<dyn std::error::Error>> {
    let instance = unsafe { GetModuleHandleW(None)? };
    let class = WNDCLASSW {
        hInstance: instance.into(),
        lpfnWndProc: Some(wndproc),
        lpszClassName: w!("SoftBenchWindow"),
        ..Default::default()
    };
    unsafe { RegisterClassW(&class) };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            w!("SoftBenchWindow"),
            w!("soft_bench"),
            WS_POPUP,
            0,
            0,
            width,
            height,
            None,
            None,
            Some(instance.into()),
            None,
        )?
    };
    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    Ok(hwnd)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

struct Scene {
    ctx: egui::Context,
    icons: Vec<egui::TextureHandle>,
    w: u32,
    h: u32,
}

impl Scene {
    fn new(w: u32, h: u32, count: usize) -> Self {
        let ctx = egui::Context::default();
        if !nooksill_settings::fonts::install_cjk(&ctx) {
            eprintln!("warn: no CJK font installed");
        }
        let img = egui::ImageData::Color(std::sync::Arc::new(icon_image(128)));
        let icons = (0..count)
            .map(|i| {
                ctx.load_texture(
                    format!("icon{i}"),
                    img.clone(),
                    egui::TextureOptions::LINEAR,
                )
            })
            .collect();
        Self { ctx, icons, w, h }
    }

    /// `zoom` is the hovered icon's scale; the neighbours fall off like the dock.
    fn shapes(
        &self,
        ppp: f32,
        zoom: f32,
    ) -> (
        Vec<egui::epaint::ClippedPrimitive>,
        usize,
        egui::TexturesDelta,
    ) {
        let mut out = self.ctx.run_ui(raw(self.w, self.h, ppp), |ui| {
            let bar = ui.max_rect();
            ui.painter().rect_filled(
                bar,
                12.0,
                egui::Color32::from_rgba_unmultiplied(26, 28, 32, 214),
            );
            let n = self.icons.len().max(1);
            let slot = bar.width() / n as f32;
            let base = slot * 0.62;
            let mid = n as f32 / 2.0;
            for (i, tex) in self.icons.iter().enumerate() {
                // A gaussian bump around the cursor, the dock's magnification.
                let d = (i as f32 - mid).abs();
                let bump = (-d * d / 2.5).exp();
                let side = base * (1.0 + (zoom - 1.0) * bump);
                let cx = bar.left() + slot * (i as f32 + 0.5);
                let cy = bar.center().y;
                let r = egui::Rect::from_center_size(egui::pos2(cx, cy), egui::vec2(side, side));
                ui.painter().rect_filled(
                    r.expand(2.0),
                    side * 0.22,
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 26),
                );
                ui.painter().image(
                    tex.id(),
                    r,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            ui.painter().text(
                bar.left_center() + egui::vec2(6.0, -bar.height() * 0.36),
                egui::Align2::LEFT_CENTER,
                "Dock 常驻 12345",
                egui::FontId::proportional(11.0),
                egui::Color32::from_gray(220),
            );
        });
        let delta = std::mem::take(&mut out.textures_delta);
        let prims = self.ctx.tessellate(out.shapes, ppp);
        let tris = prims
            .iter()
            .map(|p| match &p.primitive {
                egui::epaint::Primitive::Mesh(m) => m.indices.len() / 3,
                egui::epaint::Primitive::Callback(_) => 0,
            })
            .sum();
        (prims, tris, delta)
    }
}

struct Timing {
    tess: Duration,
    raster: Duration,
    present: Duration,
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Median rather than mean: one scheduler hiccup should not set the headline.
fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    nooksill_core::i18n::configure("", nooksill_platform::locale::ui_language_id());
    let mut args = std::env::args().skip(1);
    let ppp: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1.5);
    let icons: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(12);
    let frames: usize = 120;

    let cfg = nooksill_core::config::Config::default();
    let dpi = (ppp * 96.0).round() as i32;
    let m = layout::calc_metrics(&cfg, dpi, icons, 1, LRect::new(0, 0, 2560, 1600));
    let (dw, dh) = layout::physical_size(cfg.edge, m.lw, m.lh);
    let (w, h) = (dw as u32, dh as u32);

    let hwnd = make_window(dw, dh)?;
    let mut presenter = LayeredPresenter::new(dw, dh)?;
    let mut soft = SoftRenderer::new(w, h);
    let scene = Scene::new(w, h, icons);

    println!("CPU rasterizer, no GPU device   ppp={ppp}  icons={icons}  frames={frames}");
    println!(
        "dock {w}x{h} = {:.2} Mpx   DIB {:.2} MiB",
        f64::from(w) * f64::from(h) / 1e6,
        mb(u64::from(w) * 4 * u64::from(h))
    );

    // Warm up: first touch faults the pages in and fills the texture store.
    let mut first_tris = 0;
    for i in 0..8 {
        let zoom = 1.0 + 0.35 * (i as f32 / 8.0);
        let (prims, tris, delta) = scene.shapes(ppp, zoom);
        first_tris = tris;
        soft.apply_textures(&delta);
        soft.clear();
        soft.paint(&prims, ppp);
        presenter.upload_rgba_strided(soft.pixels(), soft.stride());
        let _ = presenter.present(hwnd, 0, 0);
    }

    let mut rows: Vec<(f32, Timing, usize)> = Vec::new();
    for &zoom in &[1.0_f32, 1.2, 1.5, 1.8] {
        let mut tess = Vec::new();
        let mut raster = Vec::new();
        let mut present_t = Vec::new();
        let mut tris = 0;
        for _ in 0..frames {
            let t0 = Instant::now();
            let (prims, n, delta) = scene.shapes(ppp, zoom);
            let t1 = Instant::now();
            soft.apply_textures(&delta);
            soft.clear();
            soft.paint(&prims, ppp);
            let t2 = Instant::now();
            presenter.upload_rgba_strided(soft.pixels(), soft.stride());
            let _ = presenter.present(hwnd, 0, 0);
            let t3 = Instant::now();
            tris = n;
            tess.push(ms(t1 - t0));
            raster.push(ms(t2 - t1));
            present_t.push(ms(t3 - t2));
        }
        rows.push((
            zoom,
            Timing {
                tess: Duration::from_secs_f64(median(&mut tess) / 1000.0),
                raster: Duration::from_secs_f64(median(&mut raster) / 1000.0),
                present: Duration::from_secs_f64(median(&mut present_t) / 1000.0),
            },
            tris,
        ));
    }

    println!("\nmedian per frame (ms)          tess   raster  present    total     fps");
    for (zoom, t, tris) in &rows {
        let total = ms(t.tess) + ms(t.raster) + ms(t.present);
        println!(
            "  zoom {zoom:.2}  {tris:>5} tris   {:>6.2} {:>8.2} {:>8.2} {:>8.2} {:>7.1}",
            ms(t.tess),
            ms(t.raster),
            ms(t.present),
            total,
            1000.0 / total
        );
    }
    println!("  (first frame {first_tris} tris)");

    let mem = sample();
    println!(
        "\nresident after bench   ws {:.2} MiB   priv {:.2} MiB",
        mem.ws_mb(),
        mem.private_mb()
    );

    // The settings window is the largest surface and stays on the GPU path, so
    // its CPU cost is the reason for that split rather than a target to hit.
    if let Some((name, sw, sh)) = surfaces::physical(ppp)
        .into_iter()
        .find(|(n, ..)| n == "settings")
    {
        let big_hwnd = make_window(sw as i32, sh as i32)?;
        let mut big_pres = LayeredPresenter::new(sw as i32, sh as i32)?;
        let mut big_soft = SoftRenderer::new(sw, sh);
        let big = Scene::new(sw, sh, icons);
        let mut samples = Vec::new();
        for i in 0..40 {
            let zoom = 1.0 + 0.5 * (i as f32 / 40.0);
            let (prims, _, delta) = big.shapes(ppp, zoom);
            let t0 = Instant::now();
            big_soft.apply_textures(&delta);
            big_soft.clear();
            big_soft.paint(&prims, ppp);
            big_pres.upload_rgba_strided(big_soft.pixels(), big_soft.stride());
            let _ = big_pres.present(big_hwnd, 0, 0);
            samples.push(ms(t0.elapsed()));
        }
        let med = median(&mut samples);
        println!(
            "{name} {sw}x{sh} = {:.2} Mpx   raster+present {:.2} ms   {:.1} fps",
            f64::from(sw) * f64::from(sh) / 1e6,
            med,
            1000.0 / med
        );
    }

    // The production CPU frame type end to end: same window, real present,
    // frost-free bar. This is what the dock now calls when GpuAccel is off.
    {
        let mut sf = nooksill_gpu::softframe::SoftFrame::new(w, h, ppp)?;
        let tex = sf.ctx().load_texture(
            "bench-icon",
            egui::ImageData::Color(std::sync::Arc::new(icon_image(128))),
            egui::TextureOptions::LINEAR,
        );
        let mut times = Vec::new();
        for i in 0..60 {
            let zoom = 1.0 + 0.5 * (i as f32 / 60.0);
            let t0 = Instant::now();
            sf.render(raw(w, h, ppp), hwnd, (0, 0), |ui| {
                let bar = ui.max_rect();
                ui.painter().rect_filled(
                    bar,
                    12.0,
                    egui::Color32::from_rgba_unmultiplied(26, 28, 32, 214),
                );
                let slot = bar.width() / icons.max(1) as f32;
                for i in 0..icons {
                    let side = slot * 0.62 * zoom;
                    let cx = bar.left() + slot * (i as f32 + 0.5);
                    let r = egui::Rect::from_center_size(
                        egui::pos2(cx, bar.center().y),
                        egui::vec2(side, side),
                    );
                    ui.painter().image(
                        tex.id(),
                        r,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
            })?;
            times.push(ms(t0.elapsed()));
        }
        let med = median(&mut times);
        let m = sample();
        println!(
            "SoftFrame (production path)   {med:.2} ms   {:.1} fps   ws {:.2} MiB   priv {:.2} MiB",
            1000.0 / med,
            m.ws_mb(),
            m.private_mb()
        );

        // What a slide adds per frame here: the GPU path pays this on its live
        // thread, the CPU path pays it inline whenever the window rect moves.
        let spec = nooksill_gpu::frost::Spec {
            radius: 14,
            corner_pt: 0.0,
        };
        sf.start_live_frost(hwnd, spec, ppp);
        let mut caps = Vec::new();
        for i in 0..40 {
            unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    40 + i,
                    40,
                    w as i32,
                    h as i32,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )?;
            }
            let t0 = Instant::now();
            sf.refresh_frost(hwnd);
            caps.push(ms(t0.elapsed()));
        }
        let cap_med = median(&mut caps);
        println!("frost capture+blur radius 14   {cap_med:.2} ms per re-capture");
        let idle = Instant::now();
        sf.refresh_frost(hwnd);
        println!("frost refresh while idle      {:.4} ms", ms(idle.elapsed()));
        // Drain the texture delta the captures left behind.
        sf.render(raw(w, h, ppp), hwnd, (0, 0), |_| {})?;
    }

    println!("60 fps needs a total under 16.67 ms; 120 fps under 8.33 ms.");
    Ok(())
}
