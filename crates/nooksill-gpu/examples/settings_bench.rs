//! Per-frame cost of the settings UI, split into egui and the GPU path.
//!
//! ```text
//! cargo run --release -p nooksill-gpu --example settings_bench -- [frames]
//! ```
//!
//! Both windows of the process share one thread, so "how expensive is a
//! settings frame" is not academic: it decides how much of the loop the bar
//! leaves for it (see `DECISIONS.md` ADR-081). The two columns answer the two
//! possible answers:
//!
//! - `ui` is the build closure: every widget, label and row of that page.
//! - `rest` is everything after it: tessellation, texture upload, submission
//!   and the readback.
//!
//! Defaults to 30 frames per page at 100% scale. Run in `--release`; debug
//! timings are several times higher and only good for ratios.

use std::time::Instant;

use nooksill_core::config::Config;
use nooksill_core::item::{Item, ItemKind};
use nooksill_gpu::frame::FrameRenderer;
use nooksill_gpu::gpu::Gpu;
use nooksill_gpu::settings_win::{shell_with_frost, LOGICAL_SIZE};
use nooksill_settings::{Outcome, Page, SettingsState};

/// First frames build the font atlas and settle the entrance animations.
const WARMUP: usize = 5;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    nooksill_core::i18n::configure("", nooksill_platform::locale::ui_language_id());
    let frames: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let (lw, lh) = LOGICAL_SIZE;
    // 1.0 keeps the numbers comparable with the in-window measurements; the
    // real scale comes from `UiScale`/system DPI and multiplies every pixel
    // cost below.
    let ppp = 1.0_f32;
    let (w, h) = ((lw * ppp) as u32, (lh * ppp) as u32);

    let gpu = Gpu::new("")?;
    let mut frame = FrameRenderer::new(&gpu, w, h, ppp)?;
    println!(
        "{w}x{h} ppp {ppp} MSAA {}x  frames {frames}\n\
         {:<12} {:>8} {:>8} {:>8} {:>8}",
        frame.samples(),
        "page",
        "ui p50",
        "rest p50",
        "total",
        "max"
    );

    for page in Page::ALL {
        let mut cfg = Config::defaults();
        let mut state = SettingsState {
            page,
            config_dir: r"C:\Users\you\AppData\Roaming\NookSill".to_owned(),
            monitors: vec!["DISPLAY1".to_owned()],
            adapters: vec!["AMD Radeon(TM) 610M".to_owned()],
            active_adapter: "AMD Radeon(TM) 610M".to_owned(),
            // The items page is empty without a list, and an empty page would
            // understate both columns.
            items: (0..12)
                .map(|i| {
                    Item::new(
                        ItemKind::File,
                        format!("item{i}"),
                        r"C:\Windows\notepad.exe",
                    )
                })
                .collect(),
            ..SettingsState::default()
        };
        let mut out = Outcome::default();
        let mut ui = Vec::with_capacity(frames);
        let mut rest = Vec::with_capacity(frames);
        let mut worst = 0.0_f64;
        let mut time = 0.0_f64;

        for i in 0..frames + WARMUP {
            time += 0.016;
            let mut ui_ms = 0.0_f64;
            let started = Instant::now();
            let _ = frame.render_rgba(&gpu, raw_input((w, h), ppp, time), |ui_ui| {
                let at = Instant::now();
                shell_with_frost(ui_ui, &mut state, &mut cfg, &mut out, false);
                ui_ms = at.elapsed().as_secs_f64() * 1000.0;
            })?;
            let total = started.elapsed().as_secs_f64() * 1000.0;
            if i >= WARMUP {
                ui.push(ui_ms);
                rest.push(total - ui_ms);
                worst = worst.max(total);
            }
        }

        let p50 = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        let ui_ms = p50(&mut ui);
        let rest_ms = p50(&mut rest);
        println!(
            "{:<12} {ui_ms:>8.2} {rest_ms:>8.2} {:>8.2} {worst:>8.2}",
            format!("{page:?}"),
            ui_ms + rest_ms
        );
    }
    Ok(())
}

/// One frame of input at `time`, matching the settings window.
fn raw_input(size: (u32, u32), ppp: f32, time: f64) -> egui::RawInput {
    let (w, h) = size;
    let mut raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(w as f32 / ppp, h as f32 / ppp),
        )),
        time: Some(time),
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
