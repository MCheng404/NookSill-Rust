//! 实验性「视觉欺骗」模糊的 GPU 侧：时域色彩重投影。
//!
//! 原高斯（[`super::frost::GpuBlur`]）只在开窗时和结构大变时跑一次，
//! 产出一张带完整空间结构（边缘、轮廓、圆角）的基底纹理。之后每帧
//! 只上传一张 24×16 的低频色场，用一个无循环的 compute pass 把
//! 「桌面颜色相对基底变了多少」叠回基底，并在 90 ms 内插值过渡。
//!
//! 为什么看起来还是模糊：面板 tint 只透出约 22% 的模糊层，而模糊本身
//! 已把高频细节抹平；人眼对这层能察觉的主要是 **颜色与明暗**，不是结构。
//! 于是「结构缓存 + 颜色实时」在观感上接近重新卷积，成本却只有约 1/20。

use std::time::{Duration, Instant};

use egui_wgpu::wgpu;

use super::ShaderRuntime;
use crate::frost::{ColorField, GRID_H, GRID_W};

const WGSL: &str = include_str!("illusion.wgsl");
const N: usize = GRID_W * GRID_H;
const FIELD_BYTES: usize = N * 16;

/// 颜色过渡时长。把「桌面突然换了一张壁纸」摊成一段连续渐变，
/// 而不是一帧跳过去 —— 割裂感主要来自跳变而不是模糊本身。
pub const FADE: Duration = Duration::from_millis(90);

/// 色场差值作用到最终颜色的比例。
const GAIN: f32 = 1.0;

pub struct Illusion {
    rt: ShaderRuntime,
    pipeline: wgpu::ComputePipeline,
    params: wgpu::Buffer,
    fields: wgpu::Buffer,
    out: Option<(wgpu::Texture, wgpu::TextureView)>,
    size: (u32, u32),
    /// 生成基底纹理时的色场（参考点）。
    base: ColorField,
    prev: ColorField,
    cur: ColorField,
    /// 0..1 的插值进度。
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
            t: 1.0,
            last: Instant::now(),
            dirty: true,
        }
    }

    #[must_use]
    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.out.as_ref().map(|(_, v)| v)
    }

    /// 基底重建后调用：把当前色场钉为新的参考点，清掉未完成的过渡。
    pub fn set_base(&mut self, field: &ColorField) {
        self.base = field.clone();
        self.prev = field.clone();
        self.cur = field.clone();
        self.t = 1.0;
        self.last = Instant::now();
        self.dirty = true;
    }

    /// 新捕获到达：把当前插值结果固化为 prev，cur 换成新色场。
    pub fn push(&mut self, field: &ColorField) {
        self.prev = blend(&self.prev, &self.cur, self.t);
        self.cur = field.clone();
        self.t = 0.0;
        self.last = Instant::now();
        self.dirty = true;
    }

    /// 推进时间插值。返回是否还需要下一帧（用于向 egui 要重绘）。
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

    /// 有未呈现的色场/插值变化时取走脏标记。
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// 把三段色场 + 参数上传，跑一次重投影，返回输出纹理视图。
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

    /// 色场与基底一致时输出应等于基底；色场变亮后输出跟着变亮。
    /// 同时验证 WGSL 能编译、bind group 自动布局合法、dispatch 不报错。
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
        ill.set_base(&field);
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

        let brighter: ColorField = field.iter().map(|_| [160.0, 160.0, 160.0]).collect();
        ill.push(&brighter);
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
}
