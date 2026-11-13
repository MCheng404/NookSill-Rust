//! Renders the settings window offscreen to a BMP, for comparing against the
//! HTML design prototype kept outside the public repository.
//!
//! The window is layered, so it is only visible when actually running, and
//! whether a layout is right should not depend on a hand-taken screenshot.
//! This takes the same path as the real window (`FrameRenderer` +
//! `settings_win::shell`) and only skips `UpdateLayeredWindow`.
//!
//! ```text
//! cargo run -p nooksill-gpu --example settings_shot -- out.bmp [page] [dark]
//! ```
//!
//! `page` is a `Page::title()`, defaulting to Appearance. The rest of the
//! arguments are order-free flags: `dark`, `tip`, `focus`, `q=<text>`,
//! `accent=RRGGBB`, `feather=<px>`, `nodiscs`, `noroundrect`, `modal` (an
//! available update, so the popup is shown), `beta` (select the beta channel),
//! `gradient` (gradient bar material), `mirror` (icon reflection), `lib` (icon
//! library preview with sample tiles), `ready`
//! (a verified download, so the install popup is shown), `progress` (a
//! download in flight), and a bare number to scale the UI.
//!
//! Needs no window and no monitor, only a working DX12 adapter.

#[path = "common/bmp.rs"]
mod bmp;

use nooksill_core::config::Config;
use nooksill_core::config::Key;
use nooksill_gpu::frame::FrameRenderer;
use nooksill_gpu::gpu::Gpu;
use nooksill_gpu::settings_win::{shell_with_frost, LOGICAL_SIZE};
use nooksill_settings::{Outcome, Page, SettingsState};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    nooksill_core::i18n::configure("", nooksill_platform::locale::ui_language_id());
    let mut args = std::env::args().skip(1);
    let out_path = args.next().unwrap_or_else(|| "settings.bmp".to_owned());
    let page_name = args.next();
    // Flags, in any order: `dark` theme, `tip` for an open tooltip, `focus` for
    // the search field, `q=<text>` to prefill it, `accent=RRGGBB` and a bare
    // percentage to scale the UI.
    //
    // A tooltip is driven by hover or click, and an offscreen frame has no
    // pointer, so its state has to be set directly.
    let rest: Vec<String> = args.collect();
    let dark = rest.iter().any(|a| a == "dark");
    let tip = rest.iter().any(|a| a == "tip");
    let modal = rest.iter().any(|a| a == "modal");
    let beta = rest.iter().any(|a| a == "beta");
    let gradient = rest.iter().any(|a| a == "gradient");
    let mirror = rest.iter().any(|a| a == "mirror");
    let ready = rest.iter().any(|a| a == "ready");
    let progress = rest.iter().any(|a| a == "progress");
    let lib = rest.iter().any(|a| a == "lib");
    // Focus is normally granted by a click, so request it from memory instead.
    let focus = rest.iter().any(|a| a == "focus");
    // A bare number is a percentage (150 -> 1.5). Fonts and every element scale
    // together, so this exercises the UI scale key without changing the layout.
    let scale = rest
        .iter()
        .filter_map(|a| a.parse::<f32>().ok())
        .next()
        .map_or(1.0, |v| (v / 100.0).clamp(0.8, 3.0));

    let page = match page_name.as_deref() {
        None => Page::Appearance,
        Some(name) => Page::ALL
            .into_iter()
            .find(|p| p.title() == name)
            .ok_or_else(|| format!("没有叫「{name}」的页；可选：{:?}", titles()))?,
    };

    let (lw, lh) = LOGICAL_SIZE;
    let ppp = scale;
    let (w, h) = ((lw * ppp) as u32, (lh * ppp) as u32);

    let gpu = Gpu::new("")?;
    let mut frame = FrameRenderer::new(&gpu, w, h, ppp)?;
    frame.ctx().set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });

    let mut cfg = Config::defaults();
    let mut state = SettingsState {
        page,
        config_dir: r"C:\Users\you\AppData\Roaming\NookSill".to_owned(),
        monitors: vec!["DISPLAY1  1920×1080".to_owned()],
        adapters: vec!["AMD Radeon(TM) 610M".to_owned()],
        active_adapter: "AMD Radeon(TM) 610M".to_owned(),
        stats: "空闲 · 0 帧/秒 · 上一次出帧 42 ms 前".to_owned(),
        ..SettingsState::default()
    };
    // `q=<text>` prefills the search box, to check text against icons.
    if let Some(text) = rest.iter().find_map(|a| a.strip_prefix("q=")) {
        state.nav_filter = text.to_owned();
    }
    if beta {
        cfg.apply_i32(Key::UPD_CHANNEL, 1);
    }
    if gradient {
        cfg.apply_i32(Key::BgMode, 1);
    }
    if mirror {
        cfg.apply_i32(Key::Reflect, 1);
    }
    // An offered release makes `settings_ui` raise the update popup itself,
    // which is the same path the running app uses.
    if modal {
        state.update.available = true;
        state.update.version = "2.1.0".to_owned();
        state.update.notes = "Bug fixes and a faster update check.".to_owned();
    }
    if ready {
        state.update.ready = true;
    }
    if lib {
        state.icon_dir = r"C:\icons".to_owned();
        let colours: [[u8; 3]; 4] = [
            [72, 133, 232],
            [232, 118, 92],
            [104, 186, 140],
            [230, 182, 84],
        ];
        let mut rows = Vec::new();
        for (i, rgb) in colours.iter().enumerate() {
            let tex = frame.ctx().load_texture(
                format!("lib-sample-{i}"),
                egui::ColorImage::from_rgba_unmultiplied([1, 1], &[rgb[0], rgb[1], rgb[2], 255]),
                egui::TextureOptions::LINEAR,
            );
            rows.push(nooksill_settings::LibIcon {
                name: format!("App {i}"),
                path: format!(r"C:\icons\App {i}.png"),
                tex: Some(tex),
            });
        }
        rows.push(nooksill_settings::LibIcon {
            name: "Missing".to_owned(),
            path: String::new(),
            tex: None,
        });
        state.icon_library = rows;
    }
    if progress {
        state.update.busy = true;
        state.update.bytes_done = 18 * 1024 * 1024;
        state.update.bytes_total = 32 * 1024 * 1024;
        state.update.bytes_per_sec = 2_400_000;
        state.update.eta_secs = 6;
        state.update.status = nooksill_core::i18n::fmt(
            "Downloading from {0}…",
            &[nooksill_core::i18n::t("GitHub direct")],
        );
    }
    // `accent=RRGGBB`; -1 in the config means follow the system colour.
    if let Some(hex) = rest.iter().find_map(|a| a.strip_prefix("accent=")) {
        if let Ok(v) = u32::from_str_radix(hex, 16) {
            // COLORREF is 0x00BBGGRR, the argument is 0xRRGGBB.
            let r = (v >> 16) & 0xFF;
            let g = (v >> 8) & 0xFF;
            let b = v & 0xFF;
            cfg.apply_i32(Key::UI_ACCENT, (b << 16 | g << 8 | r) as i32);
        }
    }
    if tip {
        state.tip = Some(nooksill_settings::pages::Tip {
            id: egui::Id::unique("shot"),
            text: "根据 Windows 强调色生成 Material You 风格色调".to_owned(),
            anchor: egui::Rect::from_min_size(egui::pos2(560.0, 300.0), egui::vec2(120.0, 20.0)),
            pinned: true,
        });
    }
    let mut out = Outcome::default();

    if focus {
        let id = egui::Id::unique("nav-search");
        frame.ctx().memory_mut(|m| m.request_focus(id));
    }
    // Overrides egui's tessellation only for shots, to isolate which official
    // setting actually controls corner and dot quality.
    let feather = rest.iter().find_map(|a| a.strip_prefix("feather="));
    let no_discs = rest.iter().any(|a| a == "nodiscs");
    let no_round = rest.iter().any(|a| a == "noroundrect");
    if feather.is_some() || no_discs || no_round {
        frame.ctx().tessellation_options_mut(|opt| {
            if let Some(v) = feather.and_then(|x| x.parse::<f32>().ok()) {
                opt.feathering_size_in_pixels = v;
            }
            if no_discs {
                opt.prerasterized_discs = false;
            }
            if no_round {
                opt.round_rects_to_pixels = false;
            }
        });
    }

    // `NOOKSILL_SHOT_FROST=1` composites the panel over a fake wallpaper. Frost
    // legibility only shows up against an unknown background, never on a solid
    // one.
    let frost = std::env::var("NOOKSILL_SHOT_FROST").is_ok();

    // Time has to move: egui fades and slides overlays in using
    // `RawInput::time`, so a frozen clock leaves the tooltip at a few percent
    // opacity and halfway through its slide.
    for t in [0.0_f64, 0.4] {
        let _ = frame.render_rgba(&gpu, raw_input((w, h), ppp, t), |ui| {
            shell_with_frost(ui, &mut state, &mut cfg, &mut out, frost)
        })?;
    }
    // The third frame has the font atlas built and the animations settled.
    let (pixels, _) = frame.render_rgba(&gpu, raw_input((w, h), ppp, 10.0), |ui| {
        shell_with_frost(ui, &mut state, &mut cfg, &mut out, frost)
    })?;

    // A bright-to-dark diagonal: a real desktop can be either, and the same
    // panel has to stay readable at both ends.
    let wallpaper = |x: u32, y: u32| -> [u8; 3] {
        let fx = x as f32 / w.max(1) as f32;
        let fy = y as f32 / h.max(1) as f32;
        let t = (fx * 0.75 + fy * 0.25).clamp(0.0, 1.0);
        let lerp = |a: f32, b: f32| (a + (b - a) * t).round() as u8;
        if dark {
            [lerp(236.0, 26.0), lerp(232.0, 32.0), lerp(214.0, 46.0)]
        } else {
            [lerp(250.0, 22.0), lerp(238.0, 30.0), lerp(210.0, 44.0)]
        }
    };
    let base = if dark { [24, 25, 30] } else { [246, 247, 250] };
    bmp::write_bmp(&out_path, w, h, &pixels, |x, y| {
        if frost {
            wallpaper(x, y)
        } else {
            base
        }
    })?;
    println!(
        "已写出 {out_path}（{}×{}，{:?}，{}，MSAA {}x）",
        w,
        h,
        page,
        if dark { "深色" } else { "浅色" },
        frame.samples()
    );
    Ok(())
}

fn titles() -> Vec<&'static str> {
    Page::ALL.iter().map(|p| p.title()).collect()
}

/// One frame of input, matching `bin/dock.rs`.
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
