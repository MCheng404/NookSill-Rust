//! What a dock costs with no GPU device at all -- the prize for a CPU path.
//!
//! ```text
//! cargo run -p nooksill-gpu --release --example mem_nogpu -- [ppp]
//! ```
//!
//! Builds everything the software path would own (layered window, DIB, egui
//! context with CJK fonts, tessellated meshes, raster buffer) and never calls
//! `Gpu::new`. Compare against `mem_surface`'s GPU phases: the difference is the
//! driver/shader floor the toggle would remove from the resident process.
//!
//! The raster step is a memset, not a real rasterizer: this probe measures
//! buffers, and a scanline rasterizer writes the same bytes into the same
//! allocation. Frame *time* is a separate question this probe does not answer.

use nooksill_core::layout::{self, Rect as LRect};
use nooksill_gpu::present::LayeredPresenter;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassW, ShowWindow, SW_SHOWNOACTIVATE, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};

#[path = "common/surfaces.rs"]
mod surfaces;

use surfaces::{mb, physical, sample};

fn raw(w: u32, h: u32, ppp: f32) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(w as f32 / ppp, h as f32 / ppp),
        )),
        ..Default::default()
    }
}

/// Alpha 1/255 keeps it invisible while DWM still treats it as a real surface.
fn make_window(width: i32, height: i32) -> Result<HWND, Box<dyn std::error::Error>> {
    let instance = unsafe { GetModuleHandleW(None)? };
    let class = WNDCLASSW {
        hInstance: instance.into(),
        lpfnWndProc: Some(wndproc),
        lpszClassName: w!("MemNoGpuWindow"),
        ..Default::default()
    };
    unsafe { RegisterClassW(&class) };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            w!("MemNoGpuWindow"),
            w!("mem_nogpu"),
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

struct Slot {
    hwnd: HWND,
    ctx: egui::Context,
    presenter: LayeredPresenter,
    surface: Vec<u8>,
    w: u32,
    h: u32,
}

impl Slot {
    fn new(name: &str, w: u32, h: u32) -> Result<Self, Box<dyn std::error::Error>> {
        let ctx = egui::Context::default();
        if !nooksill_settings::fonts::install_cjk(&ctx) {
            eprintln!("warn: no CJK font for {name}");
        }
        Ok(Self {
            hwnd: make_window(w as i32, h as i32)?,
            ctx,
            presenter: LayeredPresenter::new(w as i32, h as i32)?,
            surface: vec![0_u8; (w as usize) * (h as usize) * 4],
            w,
            h,
        })
    }

    /// Real layout and tessellation, then the bytes a rasterizer would produce.
    fn draw(&mut self, ppp: f32) -> usize {
        let out = self.ctx.run_ui(raw(self.w, self.h, ppp), |ui| {
            ui.painter().rect_filled(
                ui.max_rect(),
                8.0,
                egui::Color32::from_rgba_unmultiplied(28, 30, 34, 220),
            );
            ui.label("Dock 常驻内存 12345");
            ui.horizontal(|ui| {
                for i in 0..8 {
                    let _ = ui.button(format!("图标 {i}"));
                }
            });
        });
        let prims = self.ctx.tessellate(out.shapes, ppp);
        let tris: usize = prims
            .iter()
            .map(|p| match &p.primitive {
                egui::epaint::Primitive::Mesh(m) => m.indices.len() / 3,
                egui::epaint::Primitive::Callback(_) => 0,
            })
            .sum();
        // Premultiplied, alpha preserved -- what UpdateLayeredWindow needs.
        for px in self.surface.as_chunks_mut::<4>().0 {
            px.copy_from_slice(&[24, 26, 30, 220]);
        }
        self.presenter
            .upload_rgba_strided(&self.surface, self.w as usize * 4);
        let _ = self.presenter.present(self.hwnd, 0, 0);
        tris
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    nooksill_core::i18n::configure("", nooksill_platform::locale::ui_language_id());
    let ppp: f32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.5_f32);

    let base = sample();
    println!("no GPU device anywhere in this process   ppp={ppp}");
    println!(
        "baseline           ws {:>7.2}  priv {:>7.2}",
        base.ws_mb(),
        base.private_mb()
    );

    // The resident part: one dock bar, sized the way the product sizes it.
    let cfg = nooksill_core::config::Config::default();
    let dpi = (ppp * 96.0).round() as i32;
    let m = layout::calc_metrics(&cfg, dpi, 8, 1, LRect::new(0, 0, 2560, 1600));
    let (dw, dh) = layout::physical_size(cfg.edge, m.lw, m.lh);
    let mut dock = Slot::new("dock", dw as u32, dh as u32)?;
    let tris = dock.draw(ppp);
    dock.draw(ppp);
    let after_dock = sample();
    println!(
        "dock {dw}x{dh}  DIB {:.2} MiB  {tris} tris",
        mb(u64::from(dw as u32) * 4 * u64::from(dh as u32))
    );
    println!(
        "  + dock only      ws {:>7.2}  priv {:>7.2}   (delta ws {:>+6.2}  priv {:>+6.2})",
        after_dock.ws_mb(),
        after_dock.private_mb(),
        after_dock.ws_mb() - base.ws_mb(),
        after_dock.private_mb() - base.private_mb()
    );

    // The transient part: panels, which the plan keeps on the GPU path.
    let mut panels: Vec<Slot> = Vec::new();
    for (name, w, h) in physical(ppp) {
        let mut s = Slot::new(&name, w, h)?;
        s.draw(ppp);
        panels.push(s);
    }
    let after_all = sample();
    println!(
        "  + 7 panels       ws {:>7.2}  priv {:>7.2}   (delta ws {:>+6.2}  priv {:>+6.2})",
        after_all.ws_mb(),
        after_all.private_mb(),
        after_all.ws_mb() - base.ws_mb(),
        after_all.private_mb() - base.private_mb()
    );
    println!("\nGPU comparison: run mem_surface with the same ppp for the device floor.");
    Ok(())
}
