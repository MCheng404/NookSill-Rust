//! GPU half of the experimental "real blur plus perceptual cheat" scheme.
//!
//! Input is the real-blur base texture plus three `gw*gh` color fields
//! (base, prev, cur); output is the same base with the low-frequency color
//! delta added back. Alpha is carried through untouched, so the base stays
//! premultiplied.

use std::time::{Duration, Instant};

use egui_wgpu::wgpu;

use super::ShaderRuntime;
use crate::frost::{ColorField, GRID_H, GRID_W};

const WGSL: &str = include_str!("illusion.wgsl");
const N: usize = GRID_W * GRID_H;
const FIELD_BYTES: usize = N * 16;

/// Color transition duration. Spreads a desktop change into a continuous fade
/// instead of a single-frame jump.
pub const FADE: Duration = Duration::from_millis(90);

/// Fraction of the color-field delta applied to the final color.
const GAIN: f32 = 1.0;

pub struct Illusion {
    rt: ShaderRuntime,
    pipeline: wgpu::ComputePipeline,
    params: wgpu::Buffer,
    fields: wgpu::Buffer,
    out: Option<(wgpu::Texture, wgpu::TextureView)>,
    size: (u32, u32),
    /// Color field at each tile's last real blur, i.e. the compensation
    /// reference.
    base: ColorField,
    prev: ColorField,
    cur: ColorField,
    /// Columns covered by real blur this frame; the shader skips compensation
    /// there, so the same color change is never applied twice.
    cols: std::ops::Range<u32>,
    /// Interpolation progress, 0..1.
    t: f32,
    last: Instant,
    dirty: bool,
}

impl std::fmt::Debug for Illusion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Illusion")
            .field("size", &self.size)
            .field("t", &self.t)
            .finish_non_exhaustive()
    }
}

impl Illusion {
    #[must_use]
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let mut rt = ShaderRuntime::new(device, queue);
        let pipeline = rt.pipeline("illusion", WGSL, "reproject").clone();
        let params = rt.buffer(
            "illusion-params",
            32,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let fields = rt.buffer(
            "illusion-fields",
            (3 * FIELD_BYTES) as u64,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let empty = vec![[0.0; 3]; N];
        Self {
            rt,
            pipeline,
            params,
            fields,
            out: None,
            size: (0, 0),
            base: empty.clone(),
            prev: empty.clone(),
            cur: empty,
            cols: 0..0,
            t: 1.0,
            last: Instant::now(),
            dirty: true,
        }
    }

    #[must_use]
    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.out.as_ref().map(|(_, v)| v)
    }

    /// Sets the reference after a full real-blur rebuild: every tile's
    /// reference moves to the current field, the full width is marked covered
    /// and any fade is dropped.
    pub fn set_base(&mut self, field: &ColorField, w: u32) {
        self.base = field.clone();
        self.prev = field.clone();
        self.cur = field.clone();
        self.cols = 0..w;
        self.t = 1.0;
        self.last = Instant::now();
        self.dirty = true;
    }

    /// Handles a partial real-blur frame: columns in `cols` were just
    /// convolved, so their reference tiles refresh; the rest still get the
    /// delta, faded in.
    pub fn push(&mut self, field: &ColorField, cols: std::ops::Range<u32>, w: u32) {
        self.refresh_base_tiles(field, &cols, w);
        self.prev = blend(&self.prev, &self.cur, self.t);
        self.cur = field.clone();
        self.cols = cols;
        self.t = 0.0;
        self.last = Instant::now();
        self.dirty = true;
    }

    /// Refreshes the reference color field of the tiles covered by the real
    /// Gaussian. Tiles outside keep their old reference, so `cur - base` is
    /// exactly how much the color changed there since the last real blur.
    fn refresh_base_tiles(&mut self, field: &ColorField, cols: &std::ops::Range<u32>, w: u32) {
        if w == 0 || cols.start >= cols.end {
            return;
        }
        let width = w as usize;
        let gx0 = (cols.start as usize * GRID_W / width).min(GRID_W - 1);
        let gx1 = ((cols.end.min(w) as usize * GRID_W).div_ceil(width))
            .min(GRID_W)
            .max(gx0 + 1);
        for gy in 0..GRID_H {
            for gx in gx0..gx1 {
                let k = gy * GRID_W + gx;
                self.base[k] = field[k];
            }
        }
    }

    /// Advances the time interpolation. Returns whether another frame is
    /// needed (used to ask egui for a repaint).
    pub fn advance(&mut self) -> bool {
        if self.t >= 1.0 {
            return false;
        }
        let dt = self.last.elapsed().as_secs_f32() * 1000.0;
        self.last = Instant::now();
        self.t = (self.t + dt / FADE.as_millis() as f32).min(1.0);
        self.dirty = true;
        self.t < 1.0
    }

    #[must_use]
    pub const fn fading(&self) -> bool {
        self.t < 1.0
    }

    /// Takes the dirty flag for an unrendered color field or interpolation
    /// change.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Uploads the three color fields plus the parameters, runs one
    /// reprojection pass and returns the output texture view.
    pub fn render(
        &mut self,
        base: &wgpu::TextureView,
        w: u32,
        h: u32,
    ) -> Option<wgpu::TextureView> {
        if w == 0 || h == 0 {
            return None;
        }
        if self.size != (w, h) {
            self.out = Some(self.rt.output(w, h));
            self.size = (w, h);
            self.dirty = true;
        }
        let (_, view) = self.out.as_ref()?;

        let mut data = vec![0u8; 3 * FIELD_BYTES];
        for (k, field) in [&self.base, &self.prev, &self.cur].iter().enumerate() {
            for (i, c) in field.iter().take(N).enumerate() {
                let off = k * FIELD_BYTES + i * 16;
                data[off..off + 4].copy_from_slice(&c[0].to_ne_bytes());
                data[off + 4..off + 8].copy_from_slice(&c[1].to_ne_bytes());
                data[off + 8..off + 12].copy_from_slice(&c[2].to_ne_bytes());
            }
        }
        self.rt.queue.write_buffer(&self.fields, 0, &data);

        let mut uniform = [0u8; 32];
        uniform[0..4].copy_from_slice(&w.to_ne_bytes());
        uniform[4..8].copy_from_slice(&h.to_ne_bytes());
        uniform[8..12].copy_from_slice(&(GRID_W as u32).to_ne_bytes());
        uniform[12..16].copy_from_slice(&(GRID_H as u32).to_ne_bytes());
        uniform[16..20].copy_from_slice(&GAIN.to_ne_bytes());
        uniform[20..24].copy_from_slice(&self.t.to_ne_bytes());
        uniform[24..28].copy_from_slice(&self.cols.start.to_ne_bytes());
        uniform[28..32].copy_from_slice(&self.cols.end.to_ne_bytes());
        self.rt.queue.write_buffer(&self.params, 0, &uniform);

        let group = self
            .rt
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("illusion-reproject"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(base),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.fields.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                ],
            });
        self.rt.dispatch(&self.pipeline, &group, w * h);
        self.dirty = false;
        Some(view.clone())
    }
}

fn blend(a: &ColorField, b: &ColorField, t: f32) -> ColorField {
    a.iter()
        .zip(b)
        .map(|(p, q)| {
            [
                p[0] + (q[0] - p[0]) * t,
                p[1] + (q[1] - p[1]) * t,
                p[2] + (q[2] - p[2]) * t,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::Gpu;
    use crate::shaders::frost::GpuBlur;

    fn flat(w: usize, h: usize, v: u8) -> Vec<u8> {
        let mut out = vec![0u8; w * h * 4];
        for px in out.as_chunks_mut::<4>().0 {
            px[0] = v;
            px[1] = v;
            px[2] = v;
            px[3] = 255;
        }
        out
    }

    /// A color field equal to the base must reproduce the base; a brighter
    /// field must brighten the output. Also checks that the WGSL compiles, the
    /// bind group layout is legal and the dispatch does not error.
    #[test]
    #[ignore = "requires a GPU; run with --ignored"]
    fn reprojection_is_identity_then_tracks_colour() {
        let gpu = Gpu::new("").expect("D3D12 adapter");
        let (w, h) = (64u32, 48u32);
        let src = flat(w as usize, h as usize, 100);
        let mut blur = GpuBlur::new(&gpu.device, &gpu.queue).expect("pipeline");
        blur.blur(&src, w, h, 6, 0.0).expect("base blur");
        let base_view = blur.view().expect("view").clone();

        let mut ill = Illusion::new(&gpu.device, &gpu.queue);
        let field = crate::frost::color_field(&src, w as usize, h as usize);
        ill.set_base(&field, w);
        let out = ill.render(&base_view, w, h).expect("identity render");

        let rt = ShaderRuntime::new(&gpu.device, &gpu.queue);
        let base_px = rt
            .readback(base_view.texture(), w, h)
            .expect("base readback");
        let same = rt.readback(out.texture(), w, h).expect("identity readback");
        let center = ((h / 2) as usize * w as usize + (w / 2) as usize) * 4;
        for c in 0..4 {
            let got = i32::from(same[center + c]);
            let want = i32::from(base_px[center + c]);
            assert!(
                (got - want).abs() <= 1,
                "通道 {c}：重投影={got} 基底={want}"
            );
        }

        // With no column under the real blur, a brighter field must brighten
        // the whole frame.
        let brighter: ColorField = field.iter().map(|_| [160.0, 160.0, 160.0]).collect();
        ill.push(&brighter, 0..0, w);
        std::thread::sleep(FADE + Duration::from_millis(10));
        ill.advance();
        let out2 = ill.render(&base_view, w, h).expect("shifted render");
        let shifted = rt.readback(out2.texture(), w, h).expect("shifted readback");
        assert!(
            shifted[center] > base_px[center] + 30,
            "色场变亮后输出应跟着变亮：{} -> {}",
            base_px[center],
            shifted[center]
        );
    }

    /// Core of the combined mode: columns under the real Gaussian take the
    /// base, the rest get the color field compensation.
    #[test]
    #[ignore = "requires a GPU; run with --ignored"]
    fn true_blur_columns_skip_colour_projection() {
        let gpu = Gpu::new("").expect("D3D12 adapter");
        let (w, h) = (64u32, 48u32);
        let src = flat(w as usize, h as usize, 100);
        let mut blur = GpuBlur::new(&gpu.device, &gpu.queue).expect("pipeline");
        blur.blur(&src, w, h, 6, 0.0).expect("base blur");
        let base_view = blur.view().expect("view").clone();

        let mut ill = Illusion::new(&gpu.device, &gpu.queue);
        let field = crate::frost::color_field(&src, w as usize, h as usize);
        ill.set_base(&field, w);
        let brighter: ColorField = field.iter().map(|_| [160.0, 160.0, 160.0]).collect();
        // Left half runs the real blur, right half only the visual cheat.
        ill.push(&brighter, 0..w / 2, w);
        std::thread::sleep(FADE + Duration::from_millis(10));
        ill.advance();
        let out = ill.render(&base_view, w, h).expect("mixed render");

        let rt = ShaderRuntime::new(&gpu.device, &gpu.queue);
        let base_px = rt.readback(base_view.texture(), w, h).expect("base");
        let mixed = rt.readback(out.texture(), w, h).expect("mixed");
        let row = (h / 2) as usize * w as usize * 4;
        let left = row + (w / 4) as usize * 4;
        let right = row + (3 * w / 4) as usize * 4;
        assert!(
            (i32::from(mixed[left]) - i32::from(base_px[left])).abs() <= 1,
            "真模糊覆盖的左半不该再叠色场：{} vs {}",
            mixed[left],
            base_px[left]
        );
        assert!(
            mixed[right] > base_px[right] + 30,
            "右半应由色场补偿变亮：{} -> {}",
            base_px[right],
            mixed[right]
        );
    }
}
