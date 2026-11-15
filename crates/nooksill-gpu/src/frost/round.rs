//! Rounded-rect meshes shared by the frost sheet, panels and dock cards.
//!
//! The corner lives in geometry, not in the capture texture: the blur runs at
//! 25% and is upscaled four times, so an alpha mask baked there is a handful
//! of texels and turns into a staircase. A vector outline plus a feathered
//! ring stays smooth at any scale.

use egui::epaint::{Mesh, Vertex};
use egui::{Color32, Pos2, Rect, Shape, TextureId};

use std::f32::consts::PI;

/// Feather width in logical points; tighter than epaint's default 1.0.
const FEATHER: f32 = 0.75;

/// Boundary points of a rounded rect, four arcs starting at the top left.
///
/// Screen y grows downward, so the first arc runs from `PI` to `1.5 * PI`.
pub(crate) fn outline(rect: Rect, r: f32) -> Vec<Pos2> {
    // Points per arc grow with the radius so the curve stays smooth.
    let seg = ((r * 1.2).ceil() as usize).clamp(3, 20);
    let mut points: Vec<Pos2> = Vec::with_capacity(seg * 4 + 4);
    let corners = [
        (Pos2::new(rect.left() + r, rect.top() + r), PI, 1.5 * PI),
        (
            Pos2::new(rect.right() - r, rect.top() + r),
            1.5 * PI,
            2.0 * PI,
        ),
        (
            Pos2::new(rect.right() - r, rect.bottom() - r),
            0.0,
            0.5 * PI,
        ),
        (Pos2::new(rect.left() + r, rect.bottom() - r), 0.5 * PI, PI),
    ];
    for (center, a0, a1) in corners {
        for i in 0..=seg {
            let a = a0 + (a1 - a0) * (i as f32 / seg as f32);
            points.push(Pos2::new(center.x + r * a.cos(), center.y + r * a.sin()));
        }
    }
    points
}

/// Triangle fan over a rounded outline plus a transparent ring that provides
/// the antialiasing.
///
/// epaint has no shape that both rounds a rect and fills it with a gradient or
/// a texture, so the gradient and the frost backdrop share this mesh. `fill`
/// returns the UV and colour of one point.
pub(crate) fn fan(
    rect: Rect,
    points: &[Pos2],
    mut fill: impl FnMut(Pos2) -> (Pos2, Color32),
) -> Mesh {
    let mid = rect.center();
    let mut mesh = Mesh::default();
    let (uv, color) = fill(mid);
    mesh.vertices.push(Vertex {
        pos: mid,
        uv,
        color,
    });
    for &p in points {
        // Inset toward the centre; close enough to the normal on these arcs.
        let d = Pos2::new(mid.x - p.x, mid.y - p.y);
        let len = d.x.hypot(d.y);
        let inner = if len <= FEATHER {
            p
        } else {
            Pos2::new(p.x + d.x / len * FEATHER, p.y + d.y / len * FEATHER)
        };
        let (uv, color) = fill(inner);
        mesh.vertices.push(Vertex {
            pos: inner,
            uv,
            color,
        });
        // Same position, fully transparent: this is the antialias ring. The UV
        // stays the boundary's own, so the feather does not smear the sample.
        let (uv, _) = fill(p);
        mesh.vertices.push(Vertex {
            pos: p,
            uv,
            color: Color32::TRANSPARENT,
        });
    }

    let n = points.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let (ii, jj) = (1 + 2 * i, 1 + 2 * j);
        // Inner fan.
        mesh.indices.extend_from_slice(&[0, ii as u32, jj as u32]);
        // Feather ring.
        mesh.indices.extend_from_slice(&[
            ii as u32,
            (ii + 1) as u32,
            (jj + 1) as u32,
            ii as u32,
            (jj + 1) as u32,
            jj as u32,
        ]);
    }
    mesh
}

/// A texture drawn as a rounded rect. `win` is the window size in logical
/// points, so the UV of a point is its window fraction: every piece samples
/// the slice of the texture that sits behind itself.
pub fn image(rect: Rect, radius: f32, tex: TextureId, win: [f32; 2]) -> Shape {
    let (w, h) = (rect.width(), rect.height());
    if w <= 0.0 || h <= 0.0 || !w.is_finite() || !h.is_finite() {
        return Shape::Noop;
    }
    let r = radius.clamp(0.0, w.min(h) * 0.5);
    let win_w = win[0].max(1.0);
    let win_h = win[1].max(1.0);
    let uv = |p: Pos2| -> Pos2 {
        Pos2::new((p.x / win_w).clamp(0.0, 1.0), (p.y / win_h).clamp(0.0, 1.0))
    };
    if r < 0.5 {
        let quad = Rect::from_min_max(uv(rect.left_top()), uv(rect.right_bottom()));
        return Shape::image(tex, rect, quad, Color32::WHITE);
    }
    let points = outline(rect, r);
    let mut mesh = fan(rect, &points, |p| (uv(p), Color32::WHITE));
    mesh.texture_id = tex;
    Shape::mesh(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_stays_inside_the_rect() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(200.0, 120.0));
        for p in outline(rect, 18.0) {
            assert!(rect.contains(p), "{p:?} escaped {rect:?}");
        }
    }

    #[test]
    fn image_mesh_covers_the_rect_and_uses_the_texture() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(200.0, 120.0));
        let tex = TextureId::Managed(7);
        let Shape::Mesh(mesh) = image(rect, 18.0, tex, [200.0, 120.0]) else {
            panic!("expected a mesh");
        };
        assert_eq!(mesh.texture_id, tex);
        assert_eq!(mesh.vertices.len(), 1 + 2 * outline(rect, 18.0).len());
        assert!(mesh.vertices.iter().all(|v| rect.contains(v.pos)));
    }

    /// End to end: the sheet is clipped by geometry, so the four corners of the
    /// rendered window must be fully transparent while the centre stays filled.
    #[test]
    #[ignore = "requires a GPU and a desktop session; run with --ignored"]
    fn rounded_sheet_renders_transparent_corners() {
        use crate::frame::FrameRenderer;
        use crate::gpu::Gpu;

        let gpu = Gpu::new("").expect("D3D12 adapter");
        let (w, h) = (64u32, 48u32);
        let mut f = FrameRenderer::new(&gpu, w, h, 1.0).expect("renderer");
        let bits = vec![200u8; (w * h * 4) as usize];
        let spec = crate::frost::Spec {
            radius: 2,
            corner_pt: 12.0,
        };
        f.set_frost(crate::frost::compose(&bits, w, h, spec, 1.0).expect("compose"));
        f.set_frost_corner(12.0);
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(w as f32, h as f32),
            )),
            ..Default::default()
        };
        let (pixels, _) = f.render_rgba(&gpu, raw, |_| {}).expect("frame");
        let alpha = |x: u32, y: u32| pixels[((y * w + x) * 4 + 3) as usize];
        assert_eq!(alpha(0, 0), 0, "top-left must be clipped");
        assert_eq!(alpha(w - 1, 0), 0, "top-right must be clipped");
        assert_eq!(alpha(0, h - 1), 0, "bottom-left must be clipped");
        assert_eq!(alpha(w - 1, h - 1), 0, "bottom-right must be clipped");
        assert!(alpha(w / 2, h / 2) > 0, "the centre must stay filled");
    }

    #[test]
    fn zero_radius_draws_the_whole_rect() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(100.0, 50.0));
        let shape = image(rect, 0.0, TextureId::Managed(1), [100.0, 50.0]);
        assert!(!matches!(shape, Shape::Noop));
        let bounds = shape.visual_bounding_rect();
        assert!((bounds.width() - rect.width()).abs() < 0.5);
        assert!((bounds.height() - rect.height()).abs() < 0.5);
    }
}
