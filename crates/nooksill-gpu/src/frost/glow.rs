//! Content-driven soft-light overlay for the frosted backdrop.
//!
//! The W3C soft-light formula is a non-linear contrast operator whose neutral
//! source is 0.5. Sampling a 4x4 low-frequency luminance field from the
//! captured desktop lets bright areas darken slightly and dark areas lift,
//! compressing the dynamic range under text without a flat tint. A 5x5 vertex
//! mesh turns that field into one cheap egui shape per frame.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Color32, Rect};

/// Side of the luminance grid; 4x4 is coarse enough to stay low-frequency.
pub const GRID: usize = 4;

/// Average luminance (0..1) of each cell of the captured desktop.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Field {
    pub luma: [[f32; GRID]; GRID],
}

impl Default for Field {
    fn default() -> Self {
        Self {
            luma: [[0.5; GRID]; GRID],
        }
    }
}

impl Field {
    /// Rec. 601 luminance per grid cell; a flat image yields a flat field.
    #[must_use]
    pub fn sample(bits: &[u8], w: usize, h: usize) -> Self {
        if w == 0 || h == 0 || bits.len() < w.saturating_mul(h).saturating_mul(4) {
            return Self::default();
        }
        let mut sum = [[0.0f64; GRID]; GRID];
        let mut count = [[0u32; GRID]; GRID];
        for y in 0..h {
            let gy = (y * GRID / h).min(GRID - 1);
            for x in 0..w {
                let gx = (x * GRID / w).min(GRID - 1);
                let i = (y * w + x) * 4;
                let l = f64::from(bits[i]) * 0.299
                    + f64::from(bits[i + 1]) * 0.587
                    + f64::from(bits[i + 2]) * 0.114;
                sum[gy][gx] += l / 255.0;
                count[gy][gx] += 1;
            }
        }
        let mut out = Self::default();
        for gy in 0..GRID {
            for gx in 0..GRID {
                if count[gy][gx] > 0 {
                    out.luma[gy][gx] = (sum[gy][gx] / f64::from(count[gy][gx])) as f32;
                }
            }
        }
        out
    }

    /// Bilinear lookup; grid nodes sit at 0, 1/3, 2/3, 1.
    #[must_use]
    pub fn at(self, u: f32, v: f32) -> f32 {
        let (fx, fy) = ((u * 3.0).clamp(0.0, 3.0), (v * 3.0).clamp(0.0, 3.0));
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(GRID - 1), (y0 + 1).min(GRID - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let a = self.luma[y0][x0] * (1.0 - tx) + self.luma[y0][x1] * tx;
        let b = self.luma[y1][x0] * (1.0 - tx) + self.luma[y1][x1] * tx;
        a * (1.0 - ty) + b * ty
    }
}

/// W3C Compositing and Blending Level 1, soft-light.
///
/// `cb` is the backdrop, `cs` the source; 0.5 is the neutral source, so a
/// small deviation around it is enough to tune the effect without clipping.
#[must_use]
pub fn soft_light(cb: f32, cs: f32) -> f32 {
    let cb = cb.clamp(0.0, 1.0);
    let cs = cs.clamp(0.0, 1.0);
    if cs <= 0.5 {
        cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
    } else {
        let d = if cb <= 0.25 {
            ((16.0 * cb - 12.0) * cb + 4.0) * cb
        } else {
            cb.sqrt()
        };
        (cb + (2.0 * cs - 1.0) * (d - cb)).clamp(0.0, 1.0)
    }
}

/// Bright areas darken, dark areas lift: the same rule covers light and dark
/// themes because it follows the content, not a theme flag.
#[must_use]
pub fn source(luma: f32, strength: f32) -> f32 {
    (0.5 + (0.5 - luma) * strength).clamp(0.0, 1.0)
}

/// Converts the soft-light delta into a source-over colour.
///
/// egui meshes composite with premultiplied alpha, so a positive delta becomes
/// white with the alpha that lands exactly on `B(cb, cs)`, and a negative one
/// becomes black. This reproduces the formula for a grey backdrop through a
/// normal alpha blend.
#[must_use]
pub fn overlay(luma: f32, strength: f32) -> Color32 {
    let cb = luma.clamp(0.0, 1.0);
    let b = soft_light(cb, source(cb, strength));
    let d = b - cb;
    let alpha = if d >= 0.0 {
        d / (1.0 - cb).max(0.001)
    } else {
        -d / cb.max(0.001)
    };
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    if d >= 0.0 {
        Color32::from_rgba_premultiplied(a, a, a, a)
    } else {
        Color32::from_rgba_premultiplied(0, 0, 0, a)
    }
}

/// A 5x5 vertex grid over `rect`, one draw call, no per-pixel texture upload.
#[must_use]
pub fn mesh(rect: Rect, field: Field, strength: f32) -> Mesh {
    const N: usize = 5;
    let mut out = Mesh::default();
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return out;
    }
    for j in 0..N {
        for i in 0..N {
            let (u, v) = (i as f32 / (N - 1) as f32, j as f32 / (N - 1) as f32);
            out.vertices.push(Vertex {
                pos: egui::pos2(
                    rect.left() + rect.width() * u,
                    rect.top() + rect.height() * v,
                ),
                uv: WHITE_UV,
                color: overlay(field.at(u, v), strength),
            });
        }
    }
    for j in 0..N - 1 {
        for i in 0..N - 1 {
            let a = (j * N + i) as u32;
            let b = a + 1;
            let c = a + N as u32;
            let d = c + 1;
            out.indices.extend_from_slice(&[a, b, c, b, d, c]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_light_is_neutral_at_half_and_stays_in_range() {
        for cb in [0.0, 0.1, 0.25, 0.5, 0.8, 1.0] {
            assert!((soft_light(cb, 0.5) - cb).abs() < 1e-6, "cb={cb}");
            for cs in [0.0, 0.25, 0.75, 1.0] {
                let b = soft_light(cb, cs);
                assert!((0.0..=1.0).contains(&b), "cb={cb} cs={cs} b={b}");
            }
        }
        assert!(soft_light(0.4, 1.0) > 0.4);
        assert!(soft_light(0.4, 0.0) < 0.4);
    }

    #[test]
    fn source_pulls_extremes_toward_the_middle() {
        assert!(source(0.1, 0.35) > 0.5);
        assert!(source(0.9, 0.35) < 0.5);
        assert!((source(0.5, 0.35) - 0.5).abs() < 1e-6);
        assert!(source(0.1, 0.0) == 0.5);
    }

    #[test]
    fn overlay_is_moderate_and_uses_white_for_lift_black_for_dim() {
        let lift = overlay(0.1, 0.35);
        let dim = overlay(0.9, 0.35);
        assert!(lift.a() > 0 && lift.a() < 128, "lift alpha={}", lift.a());
        assert!(dim.a() > 0 && dim.a() < 128, "dim alpha={}", dim.a());
        assert!(lift.r() > 0, "lift must be a white overlay");
        assert_eq!(dim.r(), 0, "dim must be a black overlay");
        assert_eq!(overlay(0.5, 0.0).a(), 0, "zero strength draws nothing");
    }

    #[test]
    fn field_is_flat_for_a_flat_image_and_tracks_a_bright_corner() {
        let (w, h) = (16usize, 12usize);
        let mut flat = vec![0u8; w * h * 4];
        for px in flat.as_chunks_mut::<4>().0.iter_mut() {
            px.copy_from_slice(&[100, 100, 100, 255]);
        }
        let f = Field::sample(&flat, w, h);
        let first = f.luma[0][0];
        assert!(f.luma.iter().flatten().all(|v| (v - first).abs() < 1e-6));

        for y in 0..h / 3 {
            for x in 0..w / 3 {
                let i = (y * w + x) * 4;
                flat[i] = 255;
                flat[i + 1] = 255;
                flat[i + 2] = 255;
            }
        }
        let f = Field::sample(&flat, w, h);
        assert!(f.luma[0][0] > f.luma[3][3] + 0.1);
        assert!(f.at(0.0, 0.0) > f.at(1.0, 1.0));
    }

    #[test]
    fn mesh_tracks_content_direction() {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 100.0));
        let mut field = Field {
            luma: [[0.95; GRID]; GRID],
        };
        field.luma[3][3] = 0.05;
        let m = mesh(rect, field, 0.5);
        // Bright top-left dims with black; dark bottom-right lifts with white.
        assert_eq!(m.vertices[0].color.r(), 0);
        assert!(m.vertices[0].color.a() > 0);
        assert!(m.vertices[24].color.r() > 0);
        assert!(m.vertices[24].color.a() > 0);
    }

    /// The mesh must be accepted by egui's renderer with the default white
    /// texture, not just be structurally valid on paper.
    #[test]
    #[ignore = "requires a GPU and a desktop session; run with --ignored"]
    fn glow_mesh_renders_a_frame() {
        use crate::frame::FrameRenderer;
        use crate::gpu::Gpu;

        let gpu = Gpu::new("").expect("D3D12 adapter");
        let (w, h) = (64u32, 48u32);
        let bits = vec![128u8; (w * h * 4) as usize];
        let spec = crate::frost::Spec {
            radius: 2,
            corner_pt: 0.0,
        };
        let mut f = FrameRenderer::new(&gpu, w, h, 1.0).expect("renderer");
        f.set_frost(crate::frost::compose(&bits, w, h, spec, 1.0).expect("compose"));
        f.set_frost_glow(Field::default());
        crate::frost::set_glow_strength(50);
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(w as f32, h as f32),
            )),
            ..Default::default()
        };
        let (pixels, _) = f.render_rgba(&gpu, raw, |_| {}).expect("frame");
        assert_eq!(pixels.len(), (w * h * 4) as usize);
    }

    #[test]
    fn mesh_covers_the_rect_with_bounded_indices() {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 100.0));
        let m = mesh(rect, Field::default(), 0.35);
        assert_eq!(m.vertices.len(), 25);
        assert_eq!(m.indices.len(), 4 * 4 * 6);
        assert!(m.indices.iter().all(|i| *i < 25));
        assert!(m.vertices.iter().all(|v| rect.contains(v.pos)));
    }
}
