//! Composite one smoke frame onto a dark background and write a BMP, so the
//! effect can be judged without a GPU.
//!
//! Usage:
//! ```text
//! cargo run -q -p nooksill-core --example smoke_dump -- [seconds] [out.bmp]
//! ```
//! Defaults to 4 seconds and `smoke.bmp` in the current directory.

use std::time::Instant;

use nooksill_core::smoke::{Fluid, Frame, Noise, Tuning, CELL, PALETTE};

const BAR_W: usize = 720;
const BAR_H: usize = 48;

/// Bar size in physical pixels; matches a typical config (icon 72, padding 12).
fn env_usize(k: &str, d: usize) -> usize {
    std::env::var(k)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(d)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let secs: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(4.0);
    let out = args.next().unwrap_or_else(|| "smoke.bmp".to_owned());

    let t0 = Instant::now();
    let noise = Noise::build();
    let gen_ms = t0.elapsed().as_secs_f32() * 1000.0;

    let (bar_w, bar_h) = (
        env_usize("SMOKE_BAR_W", BAR_W),
        env_usize("SMOKE_BAR_H", BAR_H),
    );
    let mut tune = Tuning {
        enabled: true,
        ..Tuning::default()
    };
    if let Some(op) = std::env::var("SMOKE_OP")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
    {
        tune.opacity = op;
    }
    if let Some(t) = std::env::var("SMOKE_TONE")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
    {
        tune.glow = t > 0.5;
    }
    if let Some(v) = std::env::var("SMOKE_GLOW")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
    {
        tune.glow = v > 0.5;
    }
    let (mx, my) = (tune.margin_x(), tune.margin_y());
    let w = ((bar_w as f32 + 2.0 * mx) / CELL).ceil() as usize;
    let h = ((bar_h as f32 + 2.0 * my) / CELL).ceil() as usize;
    let mut fl = Fluid::new();
    // The bar rectangle in grid cells: the grid is "bar + margin", so it is
    // derived from the margin.
    fl.ensure(
        w,
        h,
        0.0,
        (
            mx / CELL,
            my / CELL,
            bar_w as f32 / CELL,
            bar_h as f32 / CELL,
        ),
        tune.thickness,
    );

    let fps: f32 = std::env::var("SMOKE_FPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60.0);
    let frames = (secs * fps) as usize;
    let t1 = Instant::now();
    for i in 0..frames {
        fl.step(
            &noise,
            &tune,
            Frame {
                dt: 1.0 / fps,
                time: i as f32 / fps,
                // The cursor rests slightly left of the bar center, as if the
                // mouse hovered there.
                mouse: (bar_w as f32 * 0.35 / CELL, (bar_h as f32 / 2.0 + my) / CELL),
                mouse_vel: (0.0, 0.0),
            },
        );
    }
    let step_ms = t1.elapsed().as_secs_f32() * 1000.0 / frames as f32;

    let t2 = Instant::now();
    let (pw, ph) = fl.raster(&noise, PALETTE[0], secs);
    fl.post(PALETTE[0], None, (mx, bar_w as f32), tune.opacity);
    let raster_ms = t2.elapsed().as_secs_f32() * 1000.0;

    // Composite: dark base + bar (rounded corners approximated by a capsule
    // distance field) + premultiplied smoke.
    //
    // Iterate in screen pixels and sample the half-resolution bitmap
    // bilinearly, matching the way egui scales it 2x with `LINEAR`. Compositing
    // at the bitmap resolution would put the bar at half its size.
    let (sw, sh) = (
        pw * nooksill_core::smoke::RES,
        ph * nooksill_core::smoke::RES,
    );
    let bar = (mx, my, bar_w as f32, bar_h as f32);
    let radius = bar_h as f32 / 2.0;
    let mut rgb = vec![0u8; sw * sh * 3];
    for sy in 0..sh {
        for sx in 0..sw {
            let x = sx as f32 + 0.5;
            let y = sy as f32 + 0.5;
            let mut c = [26.0_f32, 27.0, 31.0];
            // The bar is drawn on top of the smoke, matching the real z order
            // (the smoke is a background glow that the icons and bar cover).
            // Compositing the smoke first and testing the bar afterwards
            // avoids painting the covered part.
            let inside = x >= bar.0 && x < bar.0 + bar.2 && y >= bar.1 && y < bar.1 + bar.3;
            let opaque = if inside {
                let rx = (bar.0 + radius - x)
                    .max(x - (bar.0 + bar.2 - radius))
                    .max(0.0);
                let ry = y - (bar.1 + radius);
                rx * rx + ry * ry <= radius * radius
            } else {
                false
            };
            if !opaque {
                let (r, g, b, a) = sample(fl.pixels(), pw, ph, sx, sy);
                let a = f32::from(a) / 255.0;
                for (k, v) in [r, g, b].iter().enumerate() {
                    c[k] = f32::from(*v) + c[k] * (1.0 - a);
                }
            } else {
                c = [42.0, 44.0, 51.0];
            }
            let o = (sy * sw + sx) * 3;
            rgb[o] = c[0].min(255.0) as u8;
            rgb[o + 1] = c[1].min(255.0) as u8;
            rgb[o + 2] = c[2].min(255.0) as u8;
        }
    }
    let maxa = fl
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| p[3])
        .max()
        .unwrap_or(0);
    let (dm, da, sv, av) = fl.stats();
    println!("MAXA={maxa} d={dm:.2}/{da:.2} |v|={sv:.1} al={av:.2}");
    let path = std::path::Path::new(out.as_str());
    if let Err(e) = write_bmp(path, &rgb, sw, sh) {
        eprintln!("写 {} 失败：{e}", path.display());
        std::process::exit(1);
    }
    println!(
        "网格 {w}x{h} / 位图 {pw}x{ph} / 画面 {sw}x{sh} / 噪声 {gen_ms:.1} ms / 每帧 {step_ms:.2} ms / 光栅 {raster_ms:.1} ms → {}",
        path.display()
    );
}

/// Half-resolution bitmap to screen pixels (bilinear).
fn sample(px: &[u8], pw: usize, ph: usize, sx: usize, sy: usize) -> (u8, u8, u8, u8) {
    let res = nooksill_core::smoke::RES;
    let fx = (sx as f32 + 0.5) / res as f32 - 0.5;
    let fy = (sy as f32 + 0.5) / res as f32 - 0.5;
    let x0 = (fx.floor().max(0.0) as usize).min(pw - 1);
    let y0 = (fy.floor().max(0.0) as usize).min(ph - 1);
    let x1 = (x0 + 1).min(pw - 1);
    let y1 = (y0 + 1).min(ph - 1);
    let ax = (fx - x0 as f32).clamp(0.0, 1.0);
    let ay = (fy - y0 as f32).clamp(0.0, 1.0);
    let mut out = [0.0_f32; 4];
    for (k, o) in out.iter_mut().enumerate() {
        let at = |xx: usize, yy: usize| f32::from(px[(yy * pw + xx) * 4 + k]);
        *o = (at(x0, y0) * (1.0 - ax) + at(x1, y0) * ax) * (1.0 - ay)
            + (at(x0, y1) * (1.0 - ax) + at(x1, y1) * ax) * ay;
    }
    (out[0] as u8, out[1] as u8, out[2] as u8, out[3] as u8)
}

/// 24-bit BMP (bottom-up, rows padded to 4 bytes). No image crate: this is the
/// only use.
fn write_bmp(path: &std::path::Path, rgb: &[u8], w: usize, h: usize) -> std::io::Result<()> {
    let stride = (w * 3).div_ceil(4) * 4;
    let body = stride * h;
    let mut buf = Vec::with_capacity(54 + body);
    buf.extend_from_slice(b"BM");
    buf.extend_from_slice(&((54 + body) as u32).to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&54u32.to_le_bytes());
    buf.extend_from_slice(&40u32.to_le_bytes());
    buf.extend_from_slice(&(w as i32).to_le_bytes());
    buf.extend_from_slice(&(h as i32).to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&24u16.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&(body as u32).to_le_bytes());
    buf.extend_from_slice(&[0u8; 16]);
    for y in (0..h).rev() {
        for x in 0..w {
            let o = (y * w + x) * 3;
            buf.extend_from_slice(&[rgb[o + 2], rgb[o + 1], rgb[o]]);
        }
        buf.extend(std::iter::repeat_n(0u8, stride - w * 3));
    }
    std::fs::write(path, buf)
}
