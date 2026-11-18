//! Triangle coverage and interpolation.
//!
//! Coverage uses the top-left rule so two triangles sharing an edge never both
//! blend the pixel: egui tessellates every rounded rect and glyph into fans
//! whose internal edges would otherwise show as bright seams.

/// Integer half-open scissor, already clamped to the surface.
#[derive(Clone, Copy)]
pub struct Clip {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Clip {
    /// Clip intersected with the surface bounds.
    #[must_use]
    pub fn clamped(self, width: u32, height: u32) -> Self {
        Self {
            x0: self.x0.max(0),
            y0: self.y0.max(0),
            x1: self.x1.min(width as i32),
            y1: self.y1.min(height as i32),
        }
    }

    const fn empty(self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }
}

/// One triangle: positions in physical pixels, normalized UV, gamma
/// premultiplied vertex colors.
pub struct Tri {
    pub pos: [[f32; 2]; 3],
    pub uv: [[f32; 2]; 3],
    pub color: [[u8; 4]; 3],
}

/// Twice the signed area; positive means the winding we rasterize.
fn edge(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

/// Whether a pixel landing exactly on this edge belongs to this triangle.
fn top_left(a: [f32; 2], b: [f32; 2]) -> bool {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    (dy == 0.0 && dx > 0.0) || dy < 0.0
}

/// The surface being drawn into: bytes, row pitch and bounds.
pub struct Target<'a> {
    pub pixels: &'a mut [u8],
    pub stride: usize,
    pub width: u32,
    pub height: u32,
}

/// Rasterize one triangle, calling `shade(uv, color)` per covered pixel.
pub fn rasterize(
    tri: &Tri,
    clip: Clip,
    target: &mut Target<'_>,
    transfer: &super::color::Transfer,
    mut shade: impl FnMut([f32; 2], [u8; 4]) -> [u8; 4],
) {
    let (width, height, stride) = (target.width, target.height, target.stride);
    let pixels = &mut *target.pixels;
    let clip = clip.clamped(width, height);
    if clip.empty() {
        return;
    }
    let area = edge(tri.pos[0], tri.pos[1], tri.pos[2]);
    if area == 0.0 || !area.is_finite() {
        return;
    }
    // Normalize winding so the inside test is always "all edges >= 0".
    let (p, uv, col) = if area < 0.0 {
        (
            [tri.pos[0], tri.pos[2], tri.pos[1]],
            [tri.uv[0], tri.uv[2], tri.uv[1]],
            [tri.color[0], tri.color[2], tri.color[1]],
        )
    } else {
        (tri.pos, tri.uv, tri.color)
    };
    let area = area.abs();

    let min_x = p[0][0].min(p[1][0]).min(p[2][0]).floor() as i32;
    let max_x = p[0][0].max(p[1][0]).max(p[2][0]).ceil() as i32;
    let min_y = p[0][1].min(p[1][1]).min(p[2][1]).floor() as i32;
    let max_y = p[0][1].max(p[1][1]).max(p[2][1]).ceil() as i32;
    let x0 = min_x.max(clip.x0);
    let x1 = max_x.min(clip.x1);
    let y0 = min_y.max(clip.y0);
    let y1 = max_y.min(clip.y1);
    if x1 <= x0 || y1 <= y0 {
        return;
    }

    let tl0 = top_left(p[1], p[2]);
    let tl1 = top_left(p[2], p[0]);
    let tl2 = top_left(p[0], p[1]);
    let inv = 1.0 / area;
    // Flat fills are the common case: interpolating four identical channels
    // costs more than the blend itself and rounds back to the same bytes.
    let flat = col[0] == col[1] && col[1] == col[2];

    // Edge i runs a->b and w_i(p) = A*(p.y-a.y) - B*(p.x-a.x). Solving w_i >= 0
    // for x gives the row's span, so a centre-fan triangle stops paying for the
    // empty half of its bounding box. The span is widened by a pixel and the
    // exact test below still decides, which keeps coverage bit-identical.
    let ends = [[p[1], p[2]], [p[2], p[0]], [p[0], p[1]]];
    let delta: [[f32; 2]; 3] =
        std::array::from_fn(|i| [ends[i][1][0] - ends[i][0][0], ends[i][1][1] - ends[i][0][1]]);

    for y in y0..y1 {
        let py = y as f32 + 0.5;
        let row = y as usize * stride;
        let (mut sx, mut ex) = (x0, x1);
        for i in 0..3 {
            let a = ends[i][0];
            let d = delta[i];
            let k = d[0] * (py - a[1]) + d[1] * a[0];
            if d[1] == 0.0 {
                if k < 0.0 {
                    ex = sx;
                    break;
                }
            } else {
                let bound = k / d[1] - 0.5;
                // A near-horizontal edge divides by almost zero and saturates
                // the cast, so the margin must not be plain arithmetic.
                if bound.is_finite() {
                    if d[1] > 0.0 {
                        ex = ex.min((bound.floor() as i32).saturating_add(2));
                    } else {
                        sx = sx.max((bound.ceil() as i32).saturating_sub(1));
                    }
                }
            }
        }
        for x in sx..ex {
            let pt = [x as f32 + 0.5, py];
            let w0 = edge(p[1], p[2], pt);
            let w1 = edge(p[2], p[0], pt);
            let w2 = edge(p[0], p[1], pt);
            let in0 = w0 > 0.0 || (w0 == 0.0 && tl0);
            let in1 = w1 > 0.0 || (w1 == 0.0 && tl1);
            let in2 = w2 > 0.0 || (w2 == 0.0 && tl2);
            if !(in0 && in1 && in2) {
                continue;
            }
            let b0 = w0 * inv;
            let b1 = w1 * inv;
            let b2 = w2 * inv;
            let u = uv[0][0] * b0 + uv[1][0] * b1 + uv[2][0] * b2;
            let v = uv[0][1] * b0 + uv[1][1] * b1 + uv[2][1] * b2;
            let mut color = col[0];
            if !flat {
                for (c, slot) in color.iter_mut().enumerate() {
                    let value = f32::from(col[0][c]) * b0
                        + f32::from(col[1][c]) * b1
                        + f32::from(col[2][c]) * b2;
                    *slot = value.round().clamp(0.0, 255.0) as u8;
                }
            }
            let idx = row + x as usize * 4;
            let src = shade([u, v], color);
            let mut dst = [
                pixels[idx],
                pixels[idx + 1],
                pixels[idx + 2],
                pixels[idx + 3],
            ];
            crate::soft::color::Transfer::blend(transfer, &mut dst, src);
            pixels[idx..idx + 4].copy_from_slice(&dst);
        }
    }
}
