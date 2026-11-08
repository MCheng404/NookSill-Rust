//! Rasterization: grid to premultiplied RGBA bitmap, plus color post-processing.

use super::fluid::bilinear;
use super::*;

impl Fluid {
    /// Renders the grid into a premultiplied RGBA bitmap.
    ///
    /// Returns `(width, height)` in pixels; the bitmap covers `grid * CELL`.
    /// Three steps: per-cell alpha, bilinear upscale, three-layer opacity blend.
    ///
    pub fn raster(&mut self, noise: &Noise, base: Rgb, t: f32) -> (usize, usize) {
        let (w, h) = (self.w, self.h);
        if w < 4 || h < 4 {
            return (0, 0);
        }
        // Per-cell alpha: density times the contour band built in `ensure`.
        for i in 0..self.al.len() {
            let a = self.d[i].min(1.0);
            self.al[i] = (a * 2.2).min(1.0) * self.edge[i];
        }

        let pw = (w as f32 * CELL / RES as f32).ceil() as usize;
        let ph = (h as f32 * CELL / RES as f32).ceil() as usize;
        if self.pw != pw || self.ph != ph {
            self.pw = pw;
            self.ph = ph;
            self.px.clear();
            self.px.resize(pw * ph * 4, 0);
        }
        // Light and dark variants: light leans white at the same hue, dark sits at
        // half brightness. Density alone reads as flat tinted glass.
        let light = [
            (base[0] * 1.55 + 30.0).min(255.0),
            (base[1] * 1.5 + 18.0).min(255.0),
            (base[2] * 1.55 + 30.0).min(255.0),
        ];
        let dark = [base[0] * 0.5, base[1] * 0.5, base[2] * 0.5];
        // Scale, drift speed and opacity per layer; changing any of them flattens
        // the wisps.
        const LAYERS: [(usize, f32, f32, f32, f32); 3] = [
            (0, 1.55, 3.0, -6.0, 0.95),
            (1, 1.08, -4.0, -4.0, 0.75),
            (2, 2.16, 2.0, -8.0, 0.60),
        ];
        let (al, ox, oy) = (&self.al, &self.ox, &self.oy);
        let px = &mut self.px;
        for py in 0..ph {
            for pxx in 0..pw {
                let out = (py * pw + pxx) * 4;
                let cx = (pxx as f32 + 0.5) * RES as f32 / CELL - 0.5;
                let cy = (py as f32 + 0.5) * RES as f32 / CELL - 0.5;
                let a0 = bilinear(al, w, h, cx, cy);
                if a0 < 0.004 {
                    px[out..out + 4].fill(0);
                    continue;
                }
                let dx = bilinear(ox, w, h, cx, cy) * CELL;
                let dy = bilinear(oy, w, h, cx, cy) * CELL;
                let wx = pxx as f32 * RES as f32 - dx;
                let wy = py as f32 * RES as f32 - dy;
                let mut rest = 1.0;
                let mut top = 0.0;
                for &(k, scale, vx, vy, op) in &LAYERS {
                    let pr = noise.tap(k, wx / scale - vx * t, wy / scale - vy * t);
                    rest *= 1.0 - op * pr;
                    if k == 0 {
                        top = pr;
                    }
                }
                let dens = 1.0 - rest;
                let a = (dens * a0 * 0.95).min(0.85);
                let m = (dens * 1.1 + top * 0.2).min(1.0);
                let r = (dark[0] + (light[0] - dark[0]) * m) * a;
                let g = (dark[1] + (light[1] - dark[1]) * m) * a;
                let b = (dark[2] + (light[2] - dark[2]) * m) * a;
                // Premultiplied: components never exceed alpha, so scale by `a` before rounding.
                px[out] = (r + 0.5) as u8;
                px[out + 1] = (g + 0.5) as u8;
                px[out + 2] = (b + 0.5) as u8;
                px[out + 3] = (a * 255.0 + 0.5) as u8;
            }
        }
        (pw, ph)
    }

    /// Secondary color (left-to-right gradient) and the alpha gain above 100%.
    ///
    /// Post-processing on the bitmap: both only read the final alpha, which is
    /// cheaper here than inside the sampling loop.
    ///
    /// `bar` is `(left, width)` in bitmap coordinates; a `gain` of 1 or less does
    /// nothing, since lower opacities are applied by the caller's tint alpha.
    pub fn post(&mut self, base: Rgb, second: Option<Rgb>, bar: (f32, f32), gain: f32) {
        let (pw, ph) = (self.pw, self.ph);
        if pw == 0 || ph == 0 {
            return;
        }
        let two = second.is_some();
        if !two && gain <= 1.0 {
            return;
        }
        // Luminance base: keep per-pixel brightness when recoloring, otherwise
        // darker second-color areas sink.
        let lum = |c: Rgb| (0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]).max(1.0);
        let lum1 = lum(base);
        let c2 = second.unwrap_or([0.0; 3]);
        let gain = gain.max(1.0);
        for py in 0..ph {
            for pxx in 0..pw {
                let out = (py * pw + pxx) * 4;
                let a = self.px[out + 3];
                if a == 0 {
                    continue;
                }
                let mut r = f32::from(self.px[out]);
                let mut g = f32::from(self.px[out + 1]);
                let mut b = f32::from(self.px[out + 2]);
                if two {
                    // Relative position of this pixel on the bar, clamped outside it.
                    let k = ((pxx as f32 - bar.0) / bar.1.max(1.0)).clamp(0.0, 1.0);
                    if k > 0.004 {
                        let inv = 255.0 / f32::from(a);
                        let (ur, ug, ub) = (r * inv, g * inv, b * inv);
                        let sl = (0.3 * ur + 0.59 * ug + 0.11 * ub) / lum1;
                        let af = f32::from(a) / 255.0;
                        r = (ur + ((c2[0] * sl).min(255.0) - ur) * k) * af;
                        g = (ug + ((c2[1] * sl).min(255.0) - ug) * k) * af;
                        b = (ub + ((c2[2] * sl).min(255.0) - ub) * k) * af;
                    }
                }
                let mut na = f32::from(a);
                if gain > 1.0 {
                    na = (na * gain).min(255.0);
                    let sc = na / f32::from(a);
                    r *= sc;
                    g *= sc;
                    b *= sc;
                }
                self.px[out] = r.min(na) as u8;
                self.px[out + 1] = g.min(na) as u8;
                self.px[out + 2] = b.min(na) as u8;
                self.px[out + 3] = na as u8;
            }
        }
    }

    /// Field statistics for diagnostics: (max dye, mean dye, max speed, max alpha).
    ///
    /// The three things that explain invisible smoke: dye never built up, velocity
    /// blew up, or the mask cut it off.
    #[must_use]
    pub fn stats(&self) -> (f32, f32, f32, f32) {
        let max = |f: &Vec<f32>| f.iter().copied().fold(0.0_f32, f32::max);
        let mean = if self.d.is_empty() {
            0.0
        } else {
            self.d.iter().sum::<f32>() / self.d.len() as f32
        };
        let speed = self
            .u
            .iter()
            .zip(self.v.iter())
            .fold(0.0_f32, |m, (u, v)| m.max((u * u + v * v).sqrt()));
        (max(&self.d), mean, speed, max(&self.al))
    }

    /// Pre-multiplied RGBA bitmap (`pw * ph * 4` bytes). CPU reference path.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.px
    }

    #[must_use]
    pub fn bitmap_size(&self) -> (usize, usize) {
        let pw = (self.w as f32 * CELL / RES as f32).ceil() as usize;
        let ph = (self.h as f32 * CELL / RES as f32).ceil() as usize;
        (pw, ph)
    }

    /// Compute per-cell alpha and expose `al | ox | oy` for the GPU shader.
    /// This is the only O(grid) CPU work left before rasterization.
    #[must_use]
    pub fn gpu_fields(&mut self) -> (&[f32], &[f32], &[f32]) {
        for i in 0..self.al.len() {
            let a = self.d[i].min(1.0);
            self.al[i] = (a * 2.2).min(1.0) * self.edge[i];
        }
        (&self.al, &self.ox, &self.oy)
    }
}
