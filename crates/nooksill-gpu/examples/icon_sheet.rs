//! Sheet of every dock and control-center vector icon, rendered offscreen.
//!
//! These glyphs sit behind hover magnification and the popup layer in a real
//! window, so checking a shape change by clicking is slow. Same `FrameRenderer`
//! as the real window, output written to a BMP instead.
//!
//! ```text
//! cargo run -p nooksill-gpu --example icon_sheet -- out.bmp [dark] [accent=3b7ddd] [dpi=125]
//! ```
//!
//! `dpi` follows Windows fractional scaling so stroke widths and pixel-grid
//! alignment can be checked at non-integer scale factors.

#[path = "common/bmp.rs"]
mod bmp;

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect};
use nooksill_core::control::Tile;
use nooksill_gpu::frame::FrameRenderer;
use nooksill_gpu::glyph::{self, Builtin, IconState, IconStyle};
use nooksill_gpu::gpu::Gpu;

const W: u32 = 780;
const H: u32 = 520;
const CELL: f32 = 108.0;
const ROW_H: f32 = 112.0;
const COLS: usize = 7;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "icons.bmp".to_owned());
    let rest: Vec<String> = args.collect();
    let dark = rest.iter().any(|a| a == "dark");
    // The logical canvas is fixed; physical pixels grow with the scale factor.
    let ppp = rest
        .iter()
        .find_map(|a| a.strip_prefix("dpi="))
        .and_then(|v| v.parse::<f32>().ok())
        .map_or(1.0, |v| (v / 100.0).clamp(0.8, 3.0));
    let (pw, ph) = (
        (W as f32 * ppp).round() as u32,
        (H as f32 * ppp).round() as u32,
    );
    let accent = rest
        .iter()
        .find_map(|a| a.strip_prefix("accent="))
        .and_then(|h| u32::from_str_radix(h, 16).ok())
        .map_or_else(
            || Color32::from_rgb(46, 123, 246),
            |v| {
                Color32::from_rgb(
                    ((v >> 16) & 0xFF) as u8,
                    ((v >> 8) & 0xFF) as u8,
                    (v & 0xFF) as u8,
                )
            },
        );

    let gpu = Gpu::new("")?;
    let mut frame = FrameRenderer::new(&gpu, pw, ph, ppp)?;
    frame.ctx().set_theme(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });

    let mut pixels = Vec::new();
    for t in [0.0_f64, 0.4, 10.0] {
        let (px, _) = frame.render_rgba(&gpu, raw_input(t, ppp), |ui| {
            sheet(ui, dark, accent);
        })?;
        // Font atlas on the first frame, then animation, then a settled frame.
        pixels = px;
    }
    let bg = if dark { [24, 25, 30] } else { [246, 247, 250] };
    bmp::write_bmp(&out, pw, ph, &pixels, move |_, _| bg)?;
    println!(
        "已写出 {out}（逻辑 {W}×{H} / 物理 {pw}×{ph}，{}，{}% DPI）",
        if dark { "深色" } else { "浅色" },
        (ppp * 100.0).round() as u32
    );
    Ok(())
}

/// One cell: tonal background, glyph, English label.
///
/// Labels must stay ASCII: the bundled font has no CJK glyphs.
fn sheet(ui: &mut egui::Ui, dark: bool, accent: Color32) {
    let p = ui.painter();
    let bg = if dark {
        Color32::from_rgb(24, 25, 30)
    } else {
        Color32::from_rgb(246, 247, 250)
    };
    p.rect_filled(ui.max_rect(), CornerRadius::ZERO, bg);
    let fg = if dark {
        Color32::from_rgb(238, 240, 246)
    } else {
        Color32::from_rgb(28, 30, 38)
    };
    p.text(
        Pos2::new(16.0, 14.0),
        Align2::LEFT_TOP,
        "Dock + Control Center · icon sheet",
        FontId::proportional(15.0),
        fg,
    );

    let tiles: [(Tile, &str); 9] = [
        (Tile::Wifi, "Wifi"),
        (Tile::Bluetooth, "Bluetooth"),
        (Tile::DarkMode, "DarkMode"),
        (Tile::Location, "Location"),
        (Tile::Mute, "Mute"),
        (Tile::Settings, "Settings"),
        (Tile::Energy, "Energy"),
        (Tile::Hotspot, "Hotspot"),
        (Tile::Ethernet, "Ethernet"),
    ];

    let mut n = 0;
    let cell =
        |p: &egui::Painter, n: &mut usize, label: &str, draw: &dyn Fn(&egui::Painter, Rect)| {
            let col = *n % COLS;
            let row = *n / COLS;
            let x = 12.0 + col as f32 * CELL;
            let y = 44.0 + row as f32 * ROW_H;
            let box_r = Rect::from_min_size(Pos2::new(x, y), egui::vec2(CELL - 8.0, ROW_H - 10.0));
            let tonal = if dark {
                Color32::from_rgba_unmultiplied(255, 255, 255, 22)
            } else {
                Color32::from_rgba_unmultiplied(23, 26, 35, 14)
            };
            p.rect_filled(box_r, CornerRadius::same(12), tonal);
            draw(
                p,
                Rect::from_center_size(
                    Pos2::new(box_r.center().x, y + 38.0),
                    egui::vec2(60.0, 60.0),
                ),
            );
            p.text(
                Pos2::new(box_r.center().x, y + 80.0),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(11.0),
                fg,
            );
            *n += 1;
        };

    let style = |color: Color32| IconStyle::plain(color);
    let dock = |a: Color32| IconStyle::new(Color32::WHITE, a, IconState::default());
    let st = |state: IconState| IconStyle::new(fg, accent, state);

    // Dock builtins, monogram, sliders, speaker, and an accent variant.
    cell(p, &mut n, "Start", &|p, r| {
        glyph::paint_tile(p, r, Builtin::Start, dock(accent), dark)
    });
    cell(p, &mut n, "Launchpad", &|p, r| {
        glyph::paint_tile(p, r, Builtin::Launchpad, dock(accent), dark)
    });
    cell(p, &mut n, "Monogram", &|p, r| {
        glyph::paint_monogram(p, r, 'M', dock(accent), dark)
    });
    cell(p, &mut n, "Sliders", &|p, r| {
        glyph::sliders(p, r, style(fg))
    });
    cell(p, &mut n, "Speaker", &|p, r| {
        glyph::speaker(p, r, style(fg))
    });
    cell(p, &mut n, "Muted", &|p, r| {
        glyph::speaker(
            p,
            r,
            IconStyle::new(
                fg,
                fg,
                IconState {
                    muted: true,
                    ..IconState::default()
                },
            ),
        )
    });
    cell(p, &mut n, "Start · pink", &|p, r| {
        glyph::paint_tile(
            p,
            r,
            Builtin::Start,
            dock(Color32::from_rgb(224, 86, 138)),
            dark,
        )
    });

    // The nine control-center tiles.
    for (tile, label) in tiles {
        cell(p, &mut n, label, &|p, r| {
            glyph::control_tile(p, r, tile, style(fg))
        });
    }

    // 24 px sizes, which is where stroke widths start to fall apart.
    cell(p, &mut n, "Wifi · 24px", &|p, r| {
        let small = Rect::from_center_size(r.center(), egui::vec2(24.0, 24.0));
        p.rect_filled(small, CornerRadius::same(4), tonal(dark));
        glyph::control_tile(p, small, Tile::Wifi, style(fg))
    });
    cell(p, &mut n, "Settings · 24px", &|p, r| {
        let small = Rect::from_center_size(r.center(), egui::vec2(24.0, 24.0));
        p.rect_filled(small, CornerRadius::same(4), tonal(dark));
        glyph::control_tile(p, small, Tile::Settings, style(fg))
    });
    cell(p, &mut n, "Start · 24px", &|p, r| {
        let small = Rect::from_center_size(r.center(), egui::vec2(24.0, 24.0));
        glyph::paint_tile(p, small, Builtin::Start, dock(accent), dark)
    });
    cell(p, &mut n, "Wifi · ON", &|p, r| {
        let chip = Rect::from_center_size(r.center(), egui::vec2(48.0, 48.0));
        p.rect_filled(chip, CornerRadius::same(14), accent);
        glyph::control_tile(p, chip, Tile::Wifi, style(Color32::WHITE))
    });
    cell(p, &mut n, "Gear · ON", &|p, r| {
        let chip = Rect::from_center_size(r.center(), egui::vec2(48.0, 48.0));
        p.rect_filled(chip, CornerRadius::same(14), accent);
        glyph::control_tile(p, chip, Tile::Settings, style(Color32::WHITE))
    });

    // `level` and `phase` are the only inputs an animated glyph will need.
    cell(p, &mut n, "Wifi · lv .33", &|p, r| {
        glyph::control_tile(p, r, Tile::Wifi, st(IconState::default().with_level(0.33)))
    });
    cell(p, &mut n, "Speaker · lv .5", &|p, r| {
        glyph::speaker(p, r, st(IconState::default().with_level(0.5)))
    });
    cell(p, &mut n, "Energy · lv .4", &|p, r| {
        glyph::control_tile(p, r, Tile::Energy, st(IconState::default().with_level(0.4)))
    });
    cell(p, &mut n, "Launch · lv .55", &|p, r| {
        glyph::paint_tile(
            p,
            r,
            Builtin::Launchpad,
            IconStyle::new(
                Color32::WHITE,
                accent,
                IconState::default().with_level(0.55),
            ),
            dark,
        )
    });
    cell(p, &mut n, "Gear · phase", &|p, r| {
        glyph::control_tile(
            p,
            r,
            Tile::Settings,
            st(IconState::default().with_phase(0.5)),
        )
    });
    cell(p, &mut n, "Hotspot · phase", &|p, r| {
        glyph::control_tile(
            p,
            r,
            Tile::Hotspot,
            st(IconState::default().with_phase(0.25)),
        )
    });
    cell(p, &mut n, "accent off / on", &|p, r| {
        let left = Rect::from_center_size(
            Pos2::new(r.center().x - 15.0, r.center().y),
            egui::vec2(26.0, 26.0),
        );
        let right = left.translate(egui::vec2(30.0, 0.0));
        // Off uses the foreground colour, on uses the accent; only that differs.
        glyph::control_tile(p, left, Tile::Wifi, style(fg));
        glyph::control_tile(p, right, Tile::Wifi, style(accent));
    });
}

fn tonal(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, 40)
    } else {
        Color32::from_rgba_unmultiplied(255, 255, 255, 200)
    }
}

fn raw_input(time: f64, ppp: f32) -> egui::RawInput {
    let mut raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(W as f32, H as f32),
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
