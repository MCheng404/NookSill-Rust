//! Smoke effect: CPU fluid simulation, GPU raster shader, egui drawing.
//!
//! `SmokeFx::with_gpu` uses `shaders::smoke` (WGSL) and registers the output
//! as a native egui texture, so no per-frame bitmap upload happens. `new()`
//! keeps the CPU reference path for tests and as a fallback.
//!
//! Drawn before the bar; the glow must stay behind the icons.

use egui::{Color32, ColorImage, TextureHandle, TextureOptions};
use nooksill_core::smoke::{Fluid, Frame, Noise, Rgb, Tuning, CELL, RES};

use crate::gpu::Gpu;
use crate::shaders::puff::PuffShader;
use crate::shaders::smoke::{SharedLayer, SmokeShader};

/// Grid size is rounded up to a multiple of this many cells.
///
/// Hover widens the window (1496 -> 1720 px); bucketing keeps one zoom animation
/// inside a single grid, avoiding a rebuild that would clear the fluid and make
/// the smoke flicker.
const GRID_BUCKET: usize = 128;

/// Physical pixel size to cell count, rounded up to a `GRID_BUCKET` multiple.
fn bucket(px: f32) -> usize {
    let cells = (px / CELL).ceil().max(4.0) as usize;
    cells.div_ceil(GRID_BUCKET) * GRID_BUCKET
}

/// Rising puff: a dark blob off the bar, ringed by a colored glow.
///
/// The config calls it `clouds`; here it is named for its shape.
#[derive(Clone, Copy, Debug)]
struct Puff {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
    max: f32,
}

/// Adequate PRNG (xorshift32).
///
/// No `rand` crate: only plausible randomness is needed, and it must be
/// reproducible so two renders of the same frame match.
#[derive(Debug)]
struct Rng(u32);

impl Rng {
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// `[0, 1)`.
    fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16_777_216.0
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

/// Where the effect draws this frame.
///
/// Packed into a struct rather than four more arguments: `paint` already takes
/// `ui`, `ppp` and `tune`, and the call sites would become anonymous tuples.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// The bar's physical rect `(x, y, w, h)` in the window, without the hide
    /// offset.
    pub bar: (f32, f32, f32, f32),
    /// Hide-animation offset, in physical pixels.
    pub offset: (f32, f32),
    /// Cursor in window physical coordinates; `None` when off the dock.
    pub cursor: Option<(f32, f32)>,
    /// Monotonic clock in seconds.
    pub now_s: f64,
}

/// GPU side of the effect. `SmokeLayer` bridges the texture to FrameRenderer.
struct SmokeGpu {
    shader: SmokeShader,
    puff: PuffShader,
    layer: SharedLayer,
    fields: Vec<f32>,
    noise_ready: bool,
}

/// Smoke runtime state, one per dock window.
pub struct SmokeFx {
    /// Noise table: ~40 ms to build, so it is created on the first real frame.
    /// Users with smoke disabled never pay for it.
    noise: Option<Noise>,
    fluid: Fluid,
    /// CPU reference path only; the GPU path registers a native texture.
    tex: Option<TextureHandle>,
    gpu: Option<SmokeGpu>,
    /// Previous frame time in seconds (monotonic). `None` means the first frame
    /// and the simulation does not advance.
    last_s: Option<f64>,
    /// Previous cursor position in window physical coordinates, used for the
    /// push direction.
    last_mouse: Option<(f32, f32)>,
    /// Puff sprite: one 64x64 texture, reused.
    puff_tex: Option<TextureHandle>,
    puffs: Vec<Puff>,
    /// Spawn accumulator: one puff per whole unit.
    spawn_acc: f32,
    rng: Rng,
    /// Whether the last frame drew. Detects a return from hidden, which needs a
    /// fresh time base.
    was_on: bool,
    /// Diagnostic frame counter (`NOOKSILL_SMOKE_TRACE=1`).
    trace_n: u32,
}

/// Diagnostic switch, same convention as `NOOKSILL_GEOM` / `NOOKSILL_TRACE`.
fn trace_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NOOKSILL_SMOKE_TRACE").is_ok_and(|v| v.trim() != "0"))
}

impl std::fmt::Debug for SmokeFx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmokeFx")
            .field("grid", &self.fluid.grid())
            .field("puffs", &self.puffs.len())
            .field("noise", &self.noise.is_some())
            .finish()
    }
}

impl Default for SmokeFx {
    fn default() -> Self {
        Self::new()
    }
}

impl SmokeFx {
    #[must_use]
    pub fn new() -> Self {
        Self {
            noise: None,
            fluid: Fluid::new(),
            tex: None,
            gpu: None,
            last_s: None,
            last_mouse: None,
            puff_tex: None,
            puffs: Vec::new(),
            spawn_acc: 0.0,
            rng: Rng(0x9E37_79B9),
            was_on: false,
            trace_n: 0,
        }
    }

    /// Enable the GPU raster path. Called once at startup with the shared device.
    #[must_use]
    pub fn with_gpu(gpu: &Gpu) -> Self {
        let mut fx = Self::new();
        let shader = SmokeShader::new(&gpu.device, &gpu.queue);
        fx.gpu = Some(SmokeGpu {
            layer: shader.layer(),
            shader,
            puff: PuffShader::new(&gpu.device, &gpu.queue),
            fields: Vec::new(),
            noise_ready: false,
        });
        fx
    }

    /// Shared layer handle, passed to `FrameRenderer::set_smoke_layer`.
    #[must_use]
    pub fn layer(&self) -> Option<SharedLayer> {
        self.gpu.as_ref().map(|g| g.layer.clone())
    }

    /// Whether more frames are needed; the main loop sizes its wait on this.
    ///
    /// Distinct from `Tuning::enabled`: this answers "did the last frame draw",
    /// so disabling smoke stops waking the loop.
    #[must_use]
    pub fn active(&self) -> bool {
        self.was_on
    }

    /// Draw one frame. `view` carries this frame's coordinates and timestamp.
    pub fn paint(&mut self, ui: &egui::Ui, ppp: f32, tune: &Tuning, view: View) {
        let View {
            bar,
            offset,
            cursor,
            now_s,
        } = view;
        if !tune.enabled {
            self.was_on = false;
            self.last_s = None;
            return;
        }
        self.was_on = true;
        let noise = self.noise.get_or_insert_with(Noise::build);
        // Frame interval. First frame, or back after a pause: dt = 0, which
        // rasterizes without advancing the simulation.
        let dt = match self.last_s {
            Some(prev) => ((now_s - prev) as f32).clamp(0.0, nooksill_core::smoke::MAX_DT),
            None => 0.0,
        };
        self.last_s = Some(now_s);
        // Time for the texture drift. The modulo keeps `f32` fractional precision
        // after days of uptime.
        let t = ((now_s * f64::from(tune.speed)) % 3600.0) as f32;

        // The simulation covers the whole window, not bar plus margin: the latter
        // changes size every hover frame (icons grow), and `ensure()` rebuilds on
        // any size change, clearing velocity and dye.
        //
        // Hover also widens the window itself, hence the `GRID_BUCKET` snap so a
        // zoom animation stays inside one grid. Extra cells are masked to 0 and
        // stay invisible.
        let (bx, by, bw, bh) = bar;
        let (gx0, gy0) = (0.0, 0.0);
        let win = ui.max_rect();
        let w = bucket(win.width() * ppp);
        let h = bucket(win.height() * ppp);
        self.fluid.ensure(
            w,
            h,
            0.0,
            (bx / CELL, by / CELL, bw / CELL, bh / CELL),
            tune.thickness,
        );

        // Cursor in grid cells; velocity is the frame-to-frame delta normalized
        // to 1/60 s, so low frame rates do not fling the smoke around.
        let mouse = cursor.map_or((-999.0, -999.0), |(x, y)| {
            ((x - gx0) / CELL, (y - gy0) / CELL)
        });
        let norm = (dt * 60.0).min(2.0);
        let vel = match (cursor, self.last_mouse) {
            (Some((x, y)), Some((px, py))) => ((x - px) / CELL * norm, (y - py) / CELL * norm),
            _ => (0.0, 0.0),
        };
        self.last_mouse = cursor;

        self.fluid.step(
            noise,
            tune,
            Frame {
                dt: dt * tune.speed,
                time: t,
                mouse,
                mouse_vel: vel,
            },
        );

        // Shade one frame. GPU path writes a native texture (zero bitmap
        // upload); CPU path stays as the reference/fallback.
        let (base, second) = tune.colors((now_s % 86400.0) as f32);
        let (gw, gh) = self.fluid.grid();
        let (pw, ph) = self.fluid.bitmap_size();
        if pw == 0 || ph == 0 {
            return;
        }
        let k = 1.0 / ppp;
        let rect = egui::Rect::from_min_size(
            egui::pos2((gx0 + offset.0) * k, (gy0 + offset.1) * k),
            egui::vec2((pw * RES) as f32 * k, (ph * RES) as f32 * k),
        );
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        let alpha = (tune.opacity.min(1.0) * 255.0).round() as u8;
        let tint = Color32::from_rgba_unmultiplied(255, 255, 255, alpha);

        if trace_on() {
            self.trace_n = self.trace_n.wrapping_add(1);
            if self.trace_n % 30 == 1 {
                eprintln!(
                    "[smoke] grid={gw}x{gh} bmp={pw}x{ph} bar=({bx},{by},{bw},{bh}) off={offset:?} op={:.2} gpu={} cursor={cursor:?}",
                    tune.opacity,
                    self.gpu.is_some()
                );
            }
        }

        if let Some(g) = self.gpu.as_mut() {
            if !g.noise_ready {
                let table: Vec<f32> = noise.layers().iter().flatten().copied().collect();
                g.shader.set_noise(&table);
                g.noise_ready = true;
            }
            let (al, ox, oy) = self.fluid.gpu_fields();
            let cells = gw * gh;
            g.fields.clear();
            g.fields.reserve(cells * 3);
            g.fields.extend_from_slice(al);
            g.fields.extend_from_slice(ox);
            g.fields.extend_from_slice(oy);

            let c2 = second.unwrap_or([0.0; 3]);
            let mut p = [0.0f32; 20];
            p[0] = gw as f32;
            p[1] = gh as f32;
            p[2] = pw as f32;
            p[3] = ph as f32;
            p[4] = noise.size() as f32;
            p[5] = CELL;
            p[6] = RES as f32;
            p[7] = t;
            p[8] = base[0];
            p[9] = base[1];
            p[10] = base[2];
            p[11] = c2[0];
            p[12] = c2[1];
            p[13] = c2[2];
            p[14] = if second.is_some() { 1.0 } else { 0.0 };
            p[15] = (bx - gx0) / RES as f32;
            p[16] = bw / RES as f32;
            p[17] = tune.opacity;

            let SmokeGpu {
                shader,
                fields,
                layer,
                ..
            } = g;
            let _ = shader.raster(fields.as_slice(), &p);
            let id = layer.lock().ok().and_then(|l| l.id);
            if let Some(id) = id {
                ui.painter().image(id, rect, uv, tint);
            }
        } else {
            let (cpw, cph) = self.fluid.raster(noise, base, t);
            if cpw == 0 || cph == 0 {
                return;
            }
            self.fluid.post(
                base,
                second,
                ((bx - gx0) / RES as f32, bw / RES as f32),
                tune.opacity,
            );
            let image = ColorImage::from_rgba_premultiplied([cpw, cph], self.fluid.pixels());
            let tex = match &mut self.tex {
                Some(t) => {
                    t.set(image, TextureOptions::LINEAR);
                    t.clone()
                }
                None => ui
                    .ctx()
                    .load_texture("nooksill-smoke", image, TextureOptions::LINEAR),
            };
            self.tex = Some(tex.clone());
            ui.painter().image(tex.id(), rect, uv, tint);
        }

        if tune.clouds {
            self.paint_puffs(ui, ppp, tune, view, base, dt);
        }
    }

    /// Puffs: spawned at the bar's top edge, drifting up, swaying and growing.
    ///
    /// Two stacked draws of one prebuilt soft sprite (glow plus core): cheaper
    /// than a dozen circles, and no seams where circles meet.
    fn paint_puffs(
        &mut self,
        ui: &egui::Ui,
        ppp: f32,
        tune: &Tuning,
        view: View,
        base: Rgb,
        dt: f32,
    ) {
        let (bx, by, bw, _bh) = view.bar;
        let offset = view.offset;
        // Spawn rate: wider bar, more puffs.
        self.spawn_acc += dt * (1.2 + bw / 900.0);
        while self.spawn_acc >= 1.0 {
            self.spawn_acc -= 1.0;
            let x = bx + self.rng.range(0.0, bw.max(1.0));
            let y = by + self.rng.range(0.0, 8.0);
            let vx = self.rng.range(-7.0, 7.0);
            let vy = -self.rng.range(10.0, 24.0);
            let max = self.rng.range(2.6, 4.0);
            self.puffs.push(Puff {
                x,
                y,
                vx,
                vy,
                life: 0.0,
                max,
            });
        }
        // Advance and retire: a puff dies when its life ends or it leaves the top.
        let top = by - tune.margin_y();
        self.puffs.retain_mut(|p| {
            p.life += dt;
            p.x += (p.vx + 6.0 * (p.life * 3.0 + p.x * 0.02).sin()) * dt;
            p.y += p.vy * dt;
            p.life < p.max && p.y > top
        });

        if self.puffs.is_empty() {
            return;
        }
        let tex = self.puff_texture(ui.ctx());
        let k = 1.0 / ppp;
        let glow = [
            (base[0] * 1.55 + 40.0).min(255.0) / 255.0,
            (base[1] * 1.55 + 40.0).min(255.0) / 255.0,
            (base[2] * 1.55 + 40.0).min(255.0) / 255.0,
        ];
        for p in &self.puffs {
            let u = (p.life / p.max).clamp(0.0, 1.0);
            // Sine envelope: faint at birth and death, solid between. The extra
            // fade near the bar edge stops particles from popping into existence.
            let a =
                (std::f32::consts::PI * u).sin().powf(0.8) * ((p.y - top) / 18.0).clamp(0.0, 1.0);
            if a < 0.02 {
                continue;
            }
            let r = 3.2 + p.max * 1.5 * (0.6 + 0.5 * u);
            let c = egui::pos2((p.x + offset.0) * k, (p.y + offset.1) * k);
            let halo = egui::vec2(r * 3.2, r * 3.2) * k;
            let core = egui::vec2(r * 2.0, r * 1.8) * k;
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(
                tex.id(),
                egui::Rect::from_center_size(c, halo),
                uv,
                Color32::from_rgba_unmultiplied(
                    (glow[0] * 255.0) as u8,
                    (glow[1] * 255.0) as u8,
                    (glow[2] * 255.0) as u8,
                    (38.0 * a) as u8,
                ),
            );
            ui.painter().image(
                tex.id(),
                egui::Rect::from_center_size(c, core),
                uv,
                Color32::from_rgba_unmultiplied(8, 5, 12, (200.0 * a) as u8),
            );
        }
    }

    /// Soft puff sprite. GPU compute when available, CPU as reference/fallback.
    fn puff_texture(&mut self, ctx: &egui::Context) -> TextureHandle {
        if let Some(t) = &self.puff_tex {
            return t.clone();
        }
        let rgba = match &self.gpu {
            Some(g) => g.puff.build().unwrap_or_else(Self::puff_rgba_cpu),
            None => Self::puff_rgba_cpu(),
        };
        let size = crate::shaders::puff::SIZE as usize;
        let tex = ctx.load_texture(
            "nooksill-smoke-puff",
            ColorImage::from_rgba_premultiplied([size, size], &rgba),
            TextureOptions::LINEAR,
        );
        self.puff_tex = Some(tex.clone());
        tex
    }

    /// CPU reference for the sprite; also the no-GPU fallback.
    fn puff_rgba_cpu() -> Vec<u8> {
        let s = crate::shaders::puff::SIZE as usize;
        let mut rgba = vec![0u8; s * s * 4];
        let mut circles = vec![(0.0_f32, 0.0_f32, 0.62_f32, 0.60_f32)];
        for k in 0..6 {
            let ang = std::f32::consts::TAU * k as f32 / 6.0;
            let d = 0.62 + 0.08 * ((k * 5) % 4) as f32;
            let r = 0.36 + 0.07 * ((k * 3) % 4) as f32;
            circles.push((ang.cos() * d, ang.sin() * d * 0.86, r, r));
        }
        for y in 0..s {
            for x in 0..s {
                let u = (x as f32 + 0.5) / s as f32 * 2.2 - 1.1;
                let v = (y as f32 + 0.5) / s as f32 * 2.2 - 1.1;
                let mut best = 0.0_f32;
                for &(cx, cy, rx, ry) in &circles {
                    let dx = (u - cx) / rx;
                    let dy = (v - cy) / ry;
                    best = best.max((1.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0));
                }
                let a = best * best * (3.0 - 2.0 * best);
                let b = (a * 255.0) as u8;
                let o = (y * s + x) * 4;
                rgba[o..o + 4].copy_from_slice(&[b, b, b, b]);
            }
        }
        rgba
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end GPU path: compute output -> native texture -> rendered frame.
    #[test]
    #[ignore = "requires a GPU and a desktop session; run with --ignored"]
    fn gpu_smoke_reaches_the_frame() {
        let gpu = Gpu::new("").expect("D3D12 adapter");
        let (w, h) = (256u32, 128u32);
        let mut frame = crate::frame::FrameRenderer::new(&gpu, w, h, 1.0).expect("renderer");
        let mut fx = SmokeFx::with_gpu(&gpu);
        frame.set_smoke_layer(fx.layer().expect("layer"));
        let tune = Tuning {
            enabled: true,
            clouds: false,
            ..Tuning::default()
        };
        let bar = (40.0, 60.0, 176.0, 30.0);
        let mut lit = 0usize;
        for i in 0..6 {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(w as f32, h as f32),
                )),
                ..Default::default()
            };
            let view = View {
                bar,
                offset: (0.0, 0.0),
                cursor: None,
                now_s: f64::from(i) * 0.033,
            };
            let (pixels, _) = frame
                .render_rgba(&gpu, raw, |ui| fx.paint(ui, 1.0, &tune, view))
                .expect("frame");
            lit = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] > 8)
                .count();
        }
        assert!(lit > 100, "smoke never appeared, lit={lit}");
    }

    /// Runs a few frames end to end: simulate, rasterize, upload as an egui
    /// texture.
    ///
    /// "Smoke invisible" has three layers (disabled, transparent, never
    /// uploaded) and a screenshot only shows the last one. Inspecting
    /// `textures_delta` pins the middle layer.
    #[test]
    fn smoke_reaches_egui_as_non_empty_texture() {
        let ctx = egui::Context::default();
        let mut fx = SmokeFx::new();
        let tune = Tuning {
            enabled: true,
            ..Tuning::default()
        };
        // Bar 200x60 inside an 800x300 canvas, leaving room for the glow.
        let bar = (100.0, 90.0, 200.0, 60.0);

        let mut last: Option<Vec<u8>> = None;
        for i in 0..40 {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    // The canvas must be large: the grid snaps and buckets per
                    // window, and 400x200 would give a 320x320 square that
                    // defeats the aspect filter below.
                    egui::vec2(800.0, 300.0),
                )),
                ..Default::default()
            };
            let view = View {
                bar,
                offset: (0.0, 0.0),
                cursor: None,
                now_s: f64::from(i) * 0.033,
            };
            let mut out = ctx.run_ui(raw, |ui| {
                fx.paint(ui, 1.0, &tune, view);
            });
            for (id, deltas) in &out.textures_delta.set {
                for d in deltas {
                    let [w, h] = d.image.size();
                    // Only the smoke bitmap: it is the wide one. The puff sprite
                    // is 64x64 and the glyph atlas is square.
                    if w > 60 && h > 30 && w != h {
                        let egui::epaint::ImageData::Color(img) = &d.image;
                        last = Some(img.pixels.iter().map(|c| c.a()).collect());
                        let _ = id;
                    }
                }
            }
            out.textures_delta.clear();
        }
        let alpha = last.expect("烟雾位图从未进入 textures_delta");
        let max = alpha.iter().copied().max().unwrap_or(0);
        assert!(max > 20, "上传的位图几乎全透明，max={max}");
        // The emitter is a thin outline band whose area depends only on the bar
        // perimeter, so the lit count is absolute, not a ratio: bucketing makes
        // the ratio drift with canvas size. ~1000 is steady state; 400 proves
        // smoke was drawn without being sensitive to flow variation.
        let lit = alpha.iter().filter(|a| **a > 8).count();
        assert!(lit > 400, "只有 {lit}/{} 个像素有内容", alpha.len());
    }
}
