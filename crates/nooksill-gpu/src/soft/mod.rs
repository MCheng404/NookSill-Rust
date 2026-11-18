//! CPU rasterizer for egui frames, writing premultiplied RGBA8 with real alpha.
//!
//! This is the second rendering path behind the GPU-acceleration toggle. It
//! exists to keep the driver and shader stack out of the resident process: a
//! dock that draws through here never creates a `wgpu` device, which measured
//! 23.8 MiB working set against 207 MiB for the GPU dock (examples/mem_nogpu).
//!
//! Alpha is the reason this is not an off-the-shelf crate. The dock is a
//! layered window whose corners show the wallpaper through, so the surface
//! must carry coverage in its alpha channel; rasterizers that write an opaque
//! framebuffer cannot feed `UpdateLayeredWindow` at all.
//!
//! Pixel arithmetic follows `egui-wgpu`'s `fs_main_linear_framebuffer` exactly
//! -- gamma-space sampling and modulation, linear-space blending -- so turning
//! the toggle does not shift the picture. See `color` for the transfer tables.

mod color;
mod tri;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use color::Transfer;
use tri::{Clip, Tri};

/// One uploaded texture: gamma premultiplied RGBA8, row-major.
struct Texture {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    nearest: bool,
}

impl Texture {
    fn from_image(image: &egui::ColorImage, nearest: bool) -> Self {
        let mut pixels = Vec::with_capacity(image.pixels.len() * 4);
        for p in &image.pixels {
            pixels.extend_from_slice(&[p.r(), p.g(), p.b(), p.a()]);
        }
        Self {
            width: image.width(),
            height: image.height(),
            pixels,
            nearest,
        }
    }

    /// Copy a sub-rectangle, which is how the glyph atlas grows.
    fn update(&mut self, pos: [usize; 2], image: &egui::ColorImage) {
        for y in 0..image.height() {
            let dst_row = (pos[1] + y) * self.width + pos[0];
            for x in 0..image.width() {
                let p = image.pixels[y * image.width() + x];
                let idx = (dst_row + x) * 4;
                if idx + 4 <= self.pixels.len() {
                    self.pixels[idx..idx + 4].copy_from_slice(&[p.r(), p.g(), p.b(), p.a()]);
                }
            }
        }
    }

    fn texel(&self, x: i64, y: i64) -> [u8; 4] {
        let x = x.clamp(0, self.width as i64 - 1) as usize;
        let y = y.clamp(0, self.height as i64 - 1) as usize;
        let idx = (y * self.width + x) * 4;
        [
            self.pixels[idx],
            self.pixels[idx + 1],
            self.pixels[idx + 2],
            self.pixels[idx + 3],
        ]
    }

    /// Bilinear on gamma bytes, matching the shader's manual four-tap path.
    ///
    /// Clamp-to-edge clamps each tap index, not the coordinate: at uv 0 the
    /// sample sits at -0.5 and both taps must land on texel 0. Clamping the
    /// coordinate instead keeps a 0.5 weight on texel 1, which halves the
    /// white pixel egui uses for every untextured shape.
    fn sample(&self, uv: [f32; 2]) -> [u8; 4] {
        // egui routes every untextured shape through a 1x1 white pixel, so this
        // is the hottest sample in the frame; all four taps clamp to texel 0.
        if self.width == 1 && self.height == 1 {
            return self.texel(0, 0);
        }
        let fx = uv[0].clamp(0.0, 1.0) * self.width as f32 - 0.5;
        let fy = uv[1].clamp(0.0, 1.0) * self.height as f32 - 0.5;
        let x0 = fx.floor();
        let y0 = fy.floor();
        if self.nearest {
            return self.texel(fx.round() as i64, fy.round() as i64);
        }
        let tx = fx - x0;
        let ty = fy - y0;
        let (x0, y0) = (x0 as i64, y0 as i64);
        let tl = self.texel(x0, y0);
        let tr = self.texel(x0 + 1, y0);
        let bl = self.texel(x0, y0 + 1);
        let br = self.texel(x0 + 1, y0 + 1);
        let mut out = [0_u8; 4];
        for (c, slot) in out.iter_mut().enumerate() {
            let top = f32::from(tl[c]) * (1.0 - tx) + f32::from(tr[c]) * tx;
            let bottom = f32::from(bl[c]) * (1.0 - tx) + f32::from(br[c]) * tx;
            *slot = (top * (1.0 - ty) + bottom * ty).round().clamp(0.0, 255.0) as u8;
        }
        out
    }
}

/// A software rendering target that owns its pixels and its texture copies.
pub struct SoftRenderer {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    textures: HashMap<egui::TextureId, Texture>,
    transfer: Transfer,
}

impl SoftRenderer {
    /// Allocate a transparent surface. Nothing is drawn until `paint`.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0_u8; width as usize * height as usize * 4],
            textures: HashMap::new(),
            transfer: Transfer::new(),
        }
    }

    /// Resize, discarding contents. Textures survive: the atlas is expensive.
    pub fn resize(&mut self, width: u32, height: u32) {
        if (width, height) == (self.width, self.height) {
            return;
        }
        self.width = width;
        self.height = height;
        self.pixels.clear();
        self.pixels.resize(width as usize * height as usize * 4, 0);
    }

    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Premultiplied RGBA8, top row first -- what `upload_rgba_strided` takes.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    #[must_use]
    pub const fn stride(&self) -> usize {
        self.width as usize * 4
    }

    /// Mirror egui's texture delta, including partial atlas updates and frees.
    pub fn apply_textures(&mut self, delta: &egui::TexturesDelta) {
        for id in &delta.free {
            self.textures.remove(id);
        }
        for (id, deltas) in &delta.set {
            for image in deltas {
                let egui::ImageData::Color(color) = &image.image;
                let nearest = image.options.magnification == egui::TextureFilter::Nearest;
                match (image.pos, self.textures.get_mut(id)) {
                    (Some(pos), Some(existing)) => existing.update(pos, color.as_ref()),
                    _ => {
                        self.textures
                            .insert(*id, Texture::from_image(color.as_ref(), nearest));
                    }
                }
            }
        }
    }

    /// Clear to transparent black. The caller decides when a frame starts.
    pub fn clear(&mut self) {
        self.pixels.fill(0);
    }

    /// Draw tessellated egui output.
    ///
    /// Both vertex positions and `clip_rect` are in **logical points**, not
    /// pixels: egui-wgpu feeds the shader `screen_size_in_points` and lets the
    /// viewport supply the physical resolution, while `ScissorRect::new` scales
    /// the clip separately. A software surface has no viewport, so both must be
    /// scaled here. Skipping it draws the whole UI at 1/ppp size in the top-left
    /// corner, which is invisible at ppp 1 and wrong on every scaled monitor.
    pub fn paint(&mut self, primitives: &[egui::ClippedPrimitive], pixels_per_point: f32) {
        let (width, height) = (self.width, self.height);
        let stride = self.stride();
        for primitive in primitives {
            let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive else {
                // Paint callbacks are GPU-only effects; the CPU path has its
                // own equivalents and must not silently draw nothing here.
                continue;
            };
            let clip = Clip {
                x0: (primitive.clip_rect.min.x * pixels_per_point).round() as i32,
                y0: (primitive.clip_rect.min.y * pixels_per_point).round() as i32,
                x1: (primitive.clip_rect.max.x * pixels_per_point).round() as i32,
                y1: (primitive.clip_rect.max.y * pixels_per_point).round() as i32,
            };
            let texture = self.textures.get(&mesh.texture_id);
            for tri in mesh.indices.as_chunks::<3>().0 {
                let (a, b, c) = (
                    &mesh.vertices[tri[0] as usize],
                    &mesh.vertices[tri[1] as usize],
                    &mesh.vertices[tri[2] as usize],
                );
                let shape = Tri {
                    pos: [
                        [a.pos.x * pixels_per_point, a.pos.y * pixels_per_point],
                        [b.pos.x * pixels_per_point, b.pos.y * pixels_per_point],
                        [c.pos.x * pixels_per_point, c.pos.y * pixels_per_point],
                    ],
                    uv: [[a.uv.x, a.uv.y], [b.uv.x, b.uv.y], [c.uv.x, c.uv.y]],
                    color: [rgba(a.color), rgba(b.color), rgba(c.color)],
                };
                let mut target = tri::Target {
                    pixels: &mut self.pixels,
                    stride,
                    width,
                    height,
                };
                tri::rasterize(
                    &shape,
                    clip,
                    &mut target,
                    &self.transfer,
                    |uv, vertex| match texture {
                        Some(tex) => color::modulate(tex.sample(uv), vertex),
                        None => vertex,
                    },
                );
            }
        }
    }
}

const fn rgba(c: egui::Color32) -> [u8; 4] {
    [c.r(), c.g(), c.b(), c.a()]
}

impl std::fmt::Debug for SoftRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SoftRenderer")
            .field("size", &(self.width, self.height))
            .field("textures", &self.textures.len())
            .finish_non_exhaustive()
    }
}
