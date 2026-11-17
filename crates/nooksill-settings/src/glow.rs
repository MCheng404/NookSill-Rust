//! W3C soft-light overlay shared by frosted backdrops and settings surfaces.
//!
//! The neutral source is 0.5: bright backdrops dim slightly and dark ones lift,
//! compressing the range under text without a flat tint. A vertex mesh draws
//! the field in one shape, with no per-pixel texture upload.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Color32, Pos2, Rect};

/// Side of the luminance grid; 4x4 is coarse enough to stay low-frequency.
pub const GRID: usize = 4;

/// Average luminance (0..1) of each cell of the backdrop.
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
    /// Fixed low-frequency field for surfaces without a live capture.
    ///
    /// Screenshot and non-frost modes still get the soft-light modelling;
    /// a flat 0.5 field would be neutral and draw nothing.
    #[must_use]
    pub const fn ambient() -> Self {
        Self {
            luma: [
                [0.62, 0.55, 0.48, 0.44],
                [0.56, 0.47, 0.42, 0.50],
                [0.46, 0.40, 0.52, 0.61],
                [0.43, 0.49, 0.60, 0.66],
            ],
        }
    }

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

/// Bright areas darken, dark areas lift: the rule follows content, not theme.
#[must_use]
pub fn source(luma: f32, strength: f32) -> f32 {
    (0.5 + (0.5 - luma) * strength).clamp(0.0, 1.0)
}

/// Converts the soft-light delta into a source-over colour.
///
/// egui meshes composite with premultiplied alpha, so a positive delta becomes
/// white with the alpha that lands exactly on `B(cb, cs)`, and a negative one
/// becomes black.
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

/// Paints one panel's soft-light overlay; zero strength draws nothing.
pub fn paint(ui: &egui::Ui, rect: Rect, radius: f32, field: Option<Field>, percent: i32) {
    if percent <= 0 || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let field = field.unwrap_or_else(Field::ambient);
    let mesh = mesh_rounded(rect, radius, field, percent as f32 / 100.0);
    ui.painter().add(egui::epaint::Shape::mesh(mesh));
}

/// A 5x5 vertex grid over `rect`, one draw call, no texture upload.
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
            out.vertices.push(vertex(rect, u, v, field, strength));
        }
    }
    grid_indices(&mut out, N);
    out
}

/// Same field, clipped to a rounded rect by a centre fan.
///
/// Panels use this instead of `mesh`: a square mesh would repaint their
/// antialiased corners and break the outline against the blurred desktop.
#[must_use]
pub fn mesh_rounded(rect: Rect, radius: f32, field: Field, strength: f32) -> Mesh {
    let mut out = Mesh::default();
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return out;
    }
    let rim = outline(rect, radius);
    let n = rim.len();
    out.vertices.push(vertex(rect, 0.5, 0.5, field, strength));
    for p in rim {
        let u = (p.x - rect.left()) / rect.width();
        let v = (p.y - rect.top()) / rect.height();
        out.vertices
            .push(tinted(p, overlay(field.at(u, v), strength)));
    }
    for i in 0..n {
        out.indices
            .extend_from_slice(&[0, 1 + i as u32, 1 + ((i + 1) % n) as u32]);
    }
    out
}

fn vertex(rect: Rect, u: f32, v: f32, field: Field, strength: f32) -> Vertex {
    tinted(
        egui::pos2(
            rect.left() + rect.width() * u,
            rect.top() + rect.height() * v,
        ),
        overlay(field.at(u, v), strength),
    )
}

fn tinted(pos: Pos2, color: Color32) -> Vertex {
    Vertex {
        pos,
        uv: WHITE_UV,
        color,
    }
}

fn grid_indices(mesh: &mut Mesh, n: usize) {
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let a = (j * n + i) as u32;
            let b = a + 1;
            let c = a + n as u32;
            let d = c + 1;
            mesh.indices.extend_from_slice(&[a, b, c, b, d, c]);
        }
    }
}

/// Clockwise outline; each corner contributes five points, including ends.
fn outline(rect: Rect, radius: f32) -> Vec<Pos2> {
    let r = radius.min(rect.width() * 0.5).min(rect.height() * 0.5);
    if r <= 0.5 {
        return vec![
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ];
    }
    let corners = [
        (egui::pos2(rect.left() + r, rect.top() + r), 180.0),
        (egui::pos2(rect.right() - r, rect.top() + r), 270.0),
        (egui::pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (egui::pos2(rect.left() + r, rect.bottom() - r), 90.0),
    ];
    let mut out = Vec::with_capacity(20);
    for (c, start) in corners {
        for k in 0..=4 {
            let a = (start + k as f32 * 22.5).to_radians();
            out.push(egui::pos2(c.x + r * a.cos(), c.y + r * a.sin()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambient_field_is_low_frequency_but_not_flat() {
        let f = Field::ambient();
        let first = f.luma[0][0];
        assert!(f.luma.iter().flatten().any(|v| (v - first).abs() > 0.1));
        let m = mesh_rounded(
            Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 100.0)),
            18.0,
            f,
            0.5,
        );
        assert!(m.vertices.iter().any(|v| v.color.a() > 0));
    }

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
        assert_eq!(source(0.1, 0.0), 0.5);
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
        assert_eq!(m.vertices[0].color.r(), 0);
        assert!(m.vertices[0].color.a() > 0);
        assert!(m.vertices[24].color.r() > 0);
        assert!(m.vertices[24].color.a() > 0);
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

    #[test]
    fn rounded_mesh_stays_inside_the_panel() {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 100.0));
        let m = mesh_rounded(rect, 18.0, Field::default(), 0.35);
        assert_eq!(m.vertices.len(), 21);
        assert_eq!(m.indices.len(), 20 * 3);
        assert!(m.indices.iter().all(|i| *i < 21));
        assert!(m.vertices.iter().all(|v| rect.contains(v.pos)));
    }
}
