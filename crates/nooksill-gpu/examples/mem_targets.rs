//! A/B probe for ADR-117: what releasing a hidden window's DIB actually saves.
//!
//! ```text
//! cargo run -p nooksill-gpu --release --example mem_targets -- [ppp] [msaa] [rounds]
//! ```
//!
//! Live targets, trimmed DIB, live again -- over the same renderers. Comparing
//! phases inside one process removes the +-15 MB driver jitter that makes two
//! separate runs incomparable (BUILD.md section 5).
//!
//! Frames go through `render`, not `render_rgba`: only the present path writes
//! the DIB, and an untouched DIB never becomes resident. Measuring through
//! `render_rgba` reports a fake 0.5 MiB.

#[path = "common/surfaces.rs"]
mod surfaces;

use nooksill_gpu::frame::FrameRenderer;
use nooksill_gpu::gpu::Gpu;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassW, ShowWindow, SW_SHOWNOACTIVATE, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};

use surfaces::{mb, physical, sample};

fn raw(w: u32, h: u32) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(w as f32, h as f32),
        )),
        ..Default::default()
    }
}

/// A real layered window: DWM treats it as a surface, but alpha 1/255 keeps it
/// invisible. `render_rgba` would not do -- it never writes the DIB.
fn make_window(width: i32, height: i32) -> Result<HWND, Box<dyn std::error::Error>> {
    let instance = unsafe { GetModuleHandleW(None)? };
    let class = WNDCLASSW {
        hInstance: instance.into(),
        lpfnWndProc: Some(wndproc),
        lpszClassName: w!("MemTargetsWindow"),
        ..Default::default()
    };
    unsafe { RegisterClassW(&class) };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            w!("MemTargetsWindow"),
            w!("mem_targets"),
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
    frame: FrameRenderer,
}

/// Draw every window once so its buffers exist and the DIB is populated.
fn draw_all(gpu: &Gpu, set: &mut [Slot]) {
    for s in set.iter_mut() {
        let (w, h) = s.frame.size();
        let _ = s.frame.render(gpu, raw(w, h), s.hwnd, (0, 0), |ui| {
            ui.painter().rect_filled(
                ui.max_rect(),
                8.0,
                egui::Color32::from_rgba_unmultiplied(28, 30, 34, 1),
            );
        });
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    nooksill_core::i18n::configure("", nooksill_platform::locale::ui_language_id());
    let mut args = std::env::args().skip(1);
    let ppp: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1.5_f32);
    let msaa: i32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(4);
    let rounds: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(5);
    let adapter = std::env::var("PROBE_ADAPTER").unwrap_or_default();

    let gpu = Gpu::new(&adapter)?;
    nooksill_gpu::gpu::set_msaa_preference(msaa);
    println!("{} ppp={ppp} msaa={msaa}", gpu.selection.describe());

    let mut set: Vec<Slot> = Vec::new();
    let mut dib_bytes: u64 = 0;
    for (name, w, h) in physical(ppp) {
        let frame = FrameRenderer::new(&gpu, w, h, ppp)?;
        let dib = u64::from(w) * 4 * u64::from(h);
        dib_bytes += dib;
        println!(
            "  {name:<9} {w:>5}x{h:<5} {}x  DIB {:>5.2} MiB",
            frame.samples(),
            mb(dib)
        );
        set.push(Slot {
            hwnd: make_window(w as i32, h as i32)?,
            frame,
        });
    }
    println!("DIB total {:.2} MiB", mb(dib_bytes));

    // Warm up: the first frames compile pipelines and grow the glyph atlas.
    draw_all(&gpu, &mut set);
    draw_all(&gpu, &mut set);

    println!(
        "\nround   live ws/priv        trimmed ws/priv     restored ws/priv    saved ws  drift"
    );
    for round in 1..=rounds {
        let live = sample();
        for s in set.iter_mut() {
            s.frame.release_dib();
        }
        let trimmed = sample();
        draw_all(&gpu, &mut set);
        let restored = sample();
        println!(
            "{round:>5}   {:>7.2}/{:<7.2}    {:>7.2}/{:<7.2}    {:>7.2}/{:<7.2}    {:>6.2}  {:>+6.2}",
            live.ws_mb(),
            live.private_mb(),
            trimmed.ws_mb(),
            trimmed.private_mb(),
            restored.ws_mb(),
            restored.private_mb(),
            live.ws_mb() - trimmed.ws_mb(),
            restored.ws_mb() - live.ws_mb()
        );
    }
    Ok(())
}
