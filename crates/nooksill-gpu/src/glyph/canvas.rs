//! The scaled drawing canvas and stroke helpers.

use super::*;

/// Normalized drawing surface: origin at the centre, units scaled by the side.
pub(super) struct Canvas<'a> {
    p: &'a Painter,
    c: Pos2,
    s: f32,
    style: IconStyle,
}

impl<'a> Canvas<'a> {
    pub(super) fn from_rect(p: &'a Painter, rect: EguiRect, style: IconStyle) -> Self {
        Self {
            p,
            c: rect.center(),
            s: rect.width().min(rect.height()),
            style,
        }
    }

    pub(super) fn pt(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(self.c.x + x * self.s, self.c.y + y * self.s)
    }

    pub(super) fn sw(&self) -> f32 {
        stroke_width(self.s)
    }

    /// Draws one continuous polyline: a single `Shape::line` plus a round join
    /// per vertex, so corners stay seamless instead of stacking segment caps.
    pub(super) fn polyline_with(&self, pts: &[(f32, f32)], ink: Color32) {
        if pts.len() < 2 {
            return;
        }
        let st = Stroke::new(self.sw(), ink);
        let screen: Vec<Pos2> = pts.iter().map(|(x, y)| self.pt(*x, *y)).collect();
        self.p.add(egui::Shape::line(screen.clone(), st));
        let r = st.width * 0.5;
        let mut prev: Option<Pos2> = None;
        for q in screen {
            // A closed path repeats its first point; do not join it twice.
            if prev.is_none_or(|p| (p - q).length() > 0.01) {
                self.p.circle_filled(q, r, ink);
            }
            prev = Some(q);
        }
    }

    /// Draws a smooth stroke: one `Shape::line` inset by `sw/2` at both ends
    /// so the butt caps land on the original endpoints.
    /// `cap = None` keeps a sharp tip.
    pub(super) fn smooth_line(&self, norm: &[(f32, f32)], ink: Color32, cap: Option<f32>) {
        if norm.len() < 2 {
            return;
        }
        let st = Stroke::new(self.sw(), ink);
        let mut screen: Vec<Pos2> = norm.iter().map(|(x, y)| self.pt(*x, *y)).collect();
        let n = screen.len();
        let ends = [screen[0], screen[n - 1]];
        let d = st.width * 0.5;
        let v0 = (screen[1] - screen[0]).normalized();
        let vn = (screen[n - 1] - screen[n - 2]).normalized();
        screen[0] += v0 * d;
        screen[n - 1] -= vn * d;
        self.p.add(egui::Shape::line(screen, st));
        if let Some(r) = cap {
            for e in ends {
                self.p.circle_filled(e, r, ink);
            }
        }
    }

    /// Draws a crescent arc: sharp tip, no end caps.
    pub(super) fn arc_line(&self, norm: &[(f32, f32)], ink: Color32) {
        self.smooth_line(norm, ink, None);
    }

    pub(super) fn polyline(&self, pts: &[(f32, f32)]) {
        self.polyline_with(pts, self.style.color);
    }

    /// Samples an arc so consecutive points sit about `0.6 * sw` apart, which
    /// keeps large arcs free of visible polygon corners; clamped to 12..64.
    pub(super) fn arc_steps(&self, radius: f32, sweep: f32) -> usize {
        let len_px = radius * sweep.abs() * self.s;
        ((len_px / (self.sw() * 0.6)).ceil() as usize).clamp(12, 64)
    }

    /// Draws an arc with adaptive sampling and round end caps.
    pub(super) fn arc(&self, cx: f32, cy: f32, r: f32, from: f32, to: f32, ink: Color32) {
        let steps = self.arc_steps(r, to - from);
        let pts: Vec<(f32, f32)> = (0..=steps)
            .map(|i| {
                let t = from + (to - from) * i as f32 / steps as f32;
                (cx + r * t.cos(), cy + r * t.sin())
            })
            .collect();
        self.smooth_line(&pts, ink, Some(self.sw() * 0.5));
    }

    pub(super) fn circle_stroke(&self, x: f32, y: f32, r: f32) {
        self.p.circle_stroke(
            self.pt(x, y),
            r * self.s,
            Stroke::new(self.sw(), self.style.color),
        );
    }

    pub(super) fn circle_filled(&self, x: f32, y: f32, r: f32, color: Color32) {
        self.p.circle_filled(self.pt(x, y), r * self.s, color);
    }

    pub(super) fn dot(&self, x: f32, y: f32, r: f32) {
        self.circle_filled(x, y, r, self.style.color);
    }

    /// Returns the corner radius shared by every filled rect.
    pub(super) fn radius(&self) -> CornerRadius {
        CornerRadius::same(pix(self.s * 0.10).max(2))
    }

    pub(super) fn box_filled(&self, x: f32, y: f32, w: f32, h: f32, color: Color32) {
        self.p.rect_filled(
            EguiRect::from_min_size(self.pt(x, y), egui::vec2(w * self.s, h * self.s)),
            self.radius(),
            color,
        );
    }

    pub(super) fn box_stroke(&self, x: f32, y: f32, w: f32, h: f32) {
        self.p.rect_stroke(
            EguiRect::from_min_size(self.pt(x, y), egui::vec2(w * self.s, h * self.s)),
            self.radius(),
            Stroke::new(self.sw(), self.style.color),
            egui::StrokeKind::Inside,
        );
    }

    /// Returns the dimmed ink for unlit segments in level-driven icons.
    pub(super) fn faint(&self) -> Color32 {
        self.style.color.linear_multiply(0.30)
    }
}

/// Rounds a radius to whole pixels to avoid half-pixel blur.
pub(super) fn pix(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

/// Returns the stroke width for an icon size, following the IBM Carbon tiers
/// (16/20/24/32 px). Above 32 the width grows sublinearly, capped at 3.0.
/// The result is in logical points, scaled by pixels_per_point at draw time.
pub(super) fn stroke_width(size: f32) -> f32 {
    const TABLE: [(f32, f32); 4] = [(16.0, 1.0), (20.0, 1.25), (24.0, 1.5), (32.0, 2.0)];
    if size <= TABLE[0].0 {
        return TABLE[0].1;
    }
    for w in TABLE.windows(2) {
        if size <= w[1].0 {
            let t = (size - w[0].0) / (w[1].0 - w[0].0);
            return w[0].1 + (w[1].1 - w[0].1) * t;
        }
    }
    (TABLE[3].1 * (size / TABLE[3].0).sqrt()).min(3.0)
}
