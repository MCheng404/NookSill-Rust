//! Colour helpers, gradients and small geometry utilities.

use super::*;

/// Build a colour from `(a, r, g, b)`.
///
/// Colour constants are written alpha-first; this is the only place that maps
/// to epaint's r, g, b, a order.
pub(super) fn argbc(a: u8, r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Per-frame bar gradient parameters.
///
/// Three stops when `mid` is set; the angle is in screen degrees, 0 being
/// left-to-right and 90 top-down.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BarGradient {
    pub(super) from: Color32,
    pub(super) mid: Option<Color32>,
    pub(super) to: Color32,
    pub(super) angle_deg: f32,
}

impl BarGradient {
    /// Nearest `Direction` for the square-corner fallback.
    pub(super) fn roughly(self) -> Direction {
        let a = self.angle_deg.rem_euclid(360.0);
        if !(45.0..135.0).contains(&a) && (a < 315.0 || a >= 225.0) {
            if a < 45.0 || a >= 315.0 {
                Direction::LeftToRight
            } else {
                Direction::RightToLeft
            }
        } else if a < 225.0 {
            Direction::TopDown
        } else {
            Direction::BottomUp
        }
    }

    /// Colour at `t`, premultiplied; the mid stop sits exactly at 0.5.
    pub(super) fn at(self, t: f32) -> Color32 {
        let (a, b, u) = match self.mid {
            Some(mid) if t < 0.5 => (self.from, mid, t * 2.0),
            Some(mid) => (mid, self.to, (t - 0.5) * 2.0),
            None => (self.from, self.to, t),
        };
        let premul = |c: Color32| {
            let k = f32::from(c.a()) / 255.0;
            [
                (f32::from(c.r()) * k).round() as u8,
                (f32::from(c.g()) * k).round() as u8,
                (f32::from(c.b()) * k).round() as u8,
                c.a(),
            ]
        };
        let (pa, pb) = (premul(a), premul(b));
        let mix = |i: usize| -> u8 {
            (f32::from(pa[i]) + (f32::from(pb[i]) - f32::from(pa[i])) * u).round() as u8
        };
        Color32::from_rgba_premultiplied(mix(0), mix(1), mix(2), mix(3))
    }
}

/// Rounded rectangle filled with a linear gradient.
///
/// epaint has no shape that does both, so the mesh is built here: a triangle
/// fan from the centre plus a transparent ring that provides antialiasing.
pub(super) fn rounded_gradient(
    rect: EguiRect,
    radius: f32,
    grad: BarGradient,
) -> egui::epaint::Shape {
    use egui::epaint::Vertex;
    use std::f32::consts::PI;

    let (w, h) = (rect.width(), rect.height());
    // Check the size before clamping; a negative max panics inside `clamp`.
    // A negative rect from layout should just not draw.
    if w <= 0.0 || h <= 0.0 || !w.is_finite() || !h.is_finite() {
        return egui::epaint::Shape::Noop;
    }
    let r = radius.clamp(0.0, w.min(h) * 0.5);
    // Below this radius the corners are invisible anyway.
    if r < 0.5 {
        // Square fallback: `gradient_rect` saves vertices; the mid stop does
        // not show at this size.
        let dir = grad.roughly();
        let (a, b) = match dir {
            Direction::LeftToRight | Direction::TopDown => (grad.from, grad.to),
            _ => (grad.to, grad.from),
        };
        return egui::epaint::Shape::gradient_rect(rect, dir, [a, b]);
    }

    // Project onto the angle direction, normalised by the rect's span, so 0
    // and 90 degrees match the old `Direction` exactly.
    let (sin, cos) = grad.angle_deg.to_radians().sin_cos();
    let center = rect.center();
    let span = (cos.abs() * w + sin.abs() * h).max(1e-3);
    let param = |p: Pos2| -> f32 {
        let d = (p.x - center.x) * cos + (p.y - center.y) * sin;
        (d / span + 0.5).clamp(0.0, 1.0)
    };
    let shade = |p: Pos2| -> Color32 { grad.at(param(p)) };

    // Points per arc grow with the radius so the curve stays smooth.
    let seg = ((r * 1.2).ceil() as usize).clamp(3, 20);
    let mut outline: Vec<Pos2> = Vec::with_capacity(seg * 4 + 4);
    // Corner centres and start angles; screen y grows downward.
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
            outline.push(Pos2::new(center.x + r * a.cos(), center.y + r * a.sin()));
        }
    }

    // Feather width in logical points; tighter than epaint's default 1.0.
    const FEATHER: f32 = 0.75;
    let mid = rect.center();
    let inner = |p: Pos2| -> Pos2 {
        // Inset toward the centre; close enough to the normal on these arcs.
        let d = Pos2::new(mid.x - p.x, mid.y - p.y);
        let len = d.x.hypot(d.y);
        if len <= FEATHER {
            p
        } else {
            Pos2::new(p.x + d.x / len * FEATHER, p.y + d.y / len * FEATHER)
        }
    };

    let mut mesh = egui::epaint::Mesh::default();
    mesh.vertices.push(Vertex::untextured(mid, shade(mid)));
    for &p in &outline {
        mesh.vertices.push(Vertex::untextured(inner(p), shade(p)));
        // Same position, fully transparent: this is the antialias ring.
        mesh.vertices
            .push(Vertex::untextured(p, Color32::TRANSPARENT));
    }

    let n = outline.len();
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
    egui::epaint::Shape::mesh(mesh)
}

/// egui 0.37 `CornerRadius` stores `u8`, so sub-pixel radius is lost.
///
/// Acceptable: the acceptance threshold is 1 px.
pub(super) fn r8(v: f32) -> u8 {
    v.round().clamp(0.0, u8::MAX as f32) as u8
}

pub(super) fn it_visible(dock: &Dock, i: usize) -> bool {
    dock.items.get(i).is_some_and(|it| it.cur > 1.0)
}

/// `0xAARRGGBB` to f32 channels for blending.
pub(super) fn to_rgb(c: color::Argb) -> (f32, f32, f32) {
    (
        color::red(c) as f32,
        color::green(c) as f32,
        color::blue(c) as f32,
    )
}

/// Font size for one clock card row, in logical points.
///
/// The input is logical row height; multiplying `p.k` or using physical
/// `bar_h` would apply DPI twice.
#[must_use]
pub(super) fn clock_line_size(line_h: f32, is_date: bool) -> f32 {
    let ratio = if is_date { 0.44 } else { 0.70 };
    (line_h * ratio).max(9.0)
}
