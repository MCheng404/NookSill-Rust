//! Fluid simulation: grid allocation, buoyancy, vorticity confinement, pressure projection, advection.

use super::*;

/// Per-frame input. Separate from [`Tuning`], which only changes on settings edits.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// Frame interval in seconds; the caller clamps it to [`MAX_DT`].
    pub dt: f32,
    /// Seconds already scaled by speed; drives texture drift, turbulence and the density target.
    pub time: f32,
    /// Cursor position in grid cell coordinates.
    pub mouse: (f32, f32),
    /// Cursor movement this frame, in cells.
    pub mouse_vel: (f32, f32),
}

/// Flow field, dye and bitmap of one bar.
///
/// All fields are tiled arrays indexed `i = x + y*w`; the `0`-suffixed ones hold
/// the previous frame. `Debug` prints sizes only.
///
pub struct Fluid {
    pub(super) w: usize,
    pub(super) h: usize,
    pub(super) seed: f32,
    /// Bar rect in grid cells: `(x0, y0, w, h)`.
    ///
    /// Positions the capsule mask and the dye source; the end radius is `h/2`.
    /// Updated every frame, so it is not part of the `ensure` identity check.
    pub(super) bar: (f32, f32, f32, f32),
    pub(super) u: Vec<f32>,
    pub(super) v: Vec<f32>,
    u0: Vec<f32>,
    v0: Vec<f32>,
    pub(super) d: Vec<f32>,
    d0: Vec<f32>,
    /// Vorticity and pressure share this buffer; vorticity is cleared before the solve.
    pub(super) tmp: Vec<f32>,
    pub(super) div: Vec<f32>,
    /// Displacement field: texture coordinates are dragged by the flow, which curls the wisps.
    pub(super) ox: Vec<f32>,
    pub(super) oy: Vec<f32>,
    ox0: Vec<f32>,
    oy0: Vec<f32>,
    /// Final alpha per cell: capsule mask times density.
    pub(super) al: Vec<f32>,
    /// Contour band weight 0..=1: 1 on the bar edge, fading 3 cells inward and `reach` outward.
    ///
    /// Built in `ensure` from geometry only and shared by `step` and `raster`, so
    /// emission and visibility use the same band.
    pub(super) edge: Vec<f32>,
    /// Thickness `edge` was built with; a change forces a rebuild.
    pub(super) thick: f32,
    /// Output bitmap, premultiplied RGBA.
    pub(super) px: Vec<u8>,
    pub(super) pw: usize,
    pub(super) ph: usize,
}

impl std::fmt::Debug for Fluid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fluid")
            .field("grid", &(self.w, self.h))
            .field("bitmap", &(self.pw, self.ph))
            .field("seed", &self.seed)
            .field("bar", &self.bar)
            .finish()
    }
}

impl Default for Fluid {
    fn default() -> Self {
        Self::new()
    }
}

impl Fluid {
    #[must_use]
    pub fn new() -> Self {
        Self {
            w: 0,
            h: 0,
            seed: 0.0,
            bar: (0.0, 0.0, 0.0, 0.0),
            u: Vec::new(),
            v: Vec::new(),
            u0: Vec::new(),
            v0: Vec::new(),
            d: Vec::new(),
            d0: Vec::new(),
            tmp: Vec::new(),
            div: Vec::new(),
            ox: Vec::new(),
            oy: Vec::new(),
            ox0: Vec::new(),
            oy0: Vec::new(),
            al: Vec::new(),
            edge: Vec::new(),
            thick: 0.0,
            px: Vec::new(),
            pw: 0,
            ph: 0,
        }
    }

    /// Rebuilds the grid and mask when the geometry changed; returns whether it did.
    ///
    /// A rebuild drops all animation state, so it must only run when size or seed
    /// really changed.
    pub fn ensure(
        &mut self,
        w: usize,
        h: usize,
        seed: f32,
        bar: (f32, f32, f32, f32),
        thickness: f32,
    ) -> bool {
        // Identity is grid size and seed only. `bar` must stay out: while the bar
        // magnifies it changes every frame, which would clear velocity and dye each
        // frame and make the smoke flicker.
        let rebuild =
            self.w != w.max(4) || self.h != h.max(4) || self.seed != seed || self.u.is_empty();
        if rebuild {
            self.w = w.max(4);
            self.h = h.max(4);
            self.seed = seed;
            let n = self.w * self.h;
            for f in [
                &mut self.u,
                &mut self.v,
                &mut self.u0,
                &mut self.v0,
                &mut self.d,
                &mut self.d0,
                &mut self.tmp,
                &mut self.div,
                &mut self.ox,
                &mut self.oy,
                &mut self.ox0,
                &mut self.oy0,
                &mut self.al,
            ] {
                f.clear();
                f.resize(n, 0.0);
            }
            self.px.clear();
            self.pw = 0;
            self.ph = 0;
        }
        // The mask follows the bar and is cheap: about 0.1 ms for ~10k cells,
        // two orders of magnitude below rebuilding the grid.
        self.thick = thickness;
        self.bar = bar;
        self.edge = build_edge(self.w, self.h, bar, thickness);
        rebuild
    }

    #[must_use]
    pub fn grid(&self) -> (usize, usize) {
        (self.w, self.h)
    }

    /// Advances one frame.
    ///
    /// Order is fixed: buoyancy, turbulence, cursor, damping, vorticity confinement,
    /// self advection, pressure projection, dye, displacement, source. Projection must
    /// follow self advection, otherwise the field is divergent again right away.
    pub fn step(&mut self, noise: &Noise, tune: &Tuning, f: Frame) {
        let (w, h) = (self.w, self.h);
        if w < 4 || h < 4 {
            return;
        }
        let dt = f.dt.clamp(0.0, MAX_DT);
        if dt <= 0.0 {
            return;
        }
        let (mx, my) = f.mouse;
        let mvx = f.mouse_vel.0.clamp(-8.0, 8.0);
        let mvy = f.mouse_vel.1.clamp(-8.0, 8.0);
        let mstr = tune.mouse;
        let rr = tune.reach * tune.reach;
        let t = f.time;
        let mean1 = noise.mean(1);
        let mean2 = noise.mean(2);

        // Body forces: buoyancy upward (y decreases), noise turbulence, cursor push.
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let i = x + y * w;
                self.v[i] -= dt * 7.0 * self.d[i];
                let n1 = noise.source(
                    1,
                    x as f32 * 1.6 + 17.0 + self.seed,
                    y as f32 * 1.6,
                    t * 0.35,
                ) - mean1;
                let n2 = noise.source(
                    2,
                    x as f32 * 1.4 + self.seed,
                    y as f32 * 1.4 + 53.0,
                    t * 0.3,
                ) - mean2;
                self.u[i] += dt * 26.0 * n1;
                self.v[i] += dt * 16.0 * n2;
                let ddx = x as f32 - mx;
                let ddy = y as f32 - my;
                let r2 = ddx * ddx + ddy * ddy;
                if mstr > 0.0 && r2 < 130.0 * mstr * rr {
                    let g = (-r2 / (26.0 * mstr * rr)).exp() * mstr;
                    self.u[i] += g * (mvx * 5.0 + ddx * 0.55);
                    self.v[i] += g * (mvy * 5.0 + ddy * 0.55);
                    // The center also thins the dye: the cursor parts the smoke, it does not erase it.
                    self.d[i] *= 1.0 - 0.16 * (-r2 / (22.0 * rr)).exp() * mstr;
                }
                self.u[i] *= 0.985;
                self.v[i] *= 0.985;
            }
        }

        // Vorticity confinement: numerical damping flattens small eddies, so restore them.
        // Without this the smoke turns into a smear within two seconds.
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let i = x + y * w;
                self.tmp[i] =
                    0.5 * ((self.v[i + 1] - self.v[i - 1]) - (self.u[i + w] - self.u[i - w]));
            }
        }
        for y in 2..h - 2 {
            for x in 2..w - 2 {
                let i = x + y * w;
                let gx = 0.5 * (self.tmp[i + 1].abs() - self.tmp[i - 1].abs());
                let gy = 0.5 * (self.tmp[i + w].abs() - self.tmp[i - w].abs());
                let len = (gx * gx + gy * gy).sqrt() + 1e-5;
                self.u[i] += dt * 2.2 * (gy / len) * self.tmp[i];
                self.v[i] -= dt * 2.2 * (gx / len) * self.tmp[i];
            }
        }

        // Self advection: copy the velocity into `u0`/`v0`, then trace back through the
        // copies. They are element-wise equal here, so this matches in-place advection
        // while avoiding the `&self.u` / `&mut self.u` borrow conflict.
        self.u0.copy_from_slice(&self.u);
        self.v0.copy_from_slice(&self.v);
        advect(&self.u0, &self.v0, &self.u0, &mut self.u, w, h, dt * 8.0);
        advect(&self.u0, &self.v0, &self.v0, &mut self.v, w, h, dt * 8.0);

        // Pressure projection: 10 Jacobi iterations make the field divergence-free;
        // fewer lets smoke leak out at the borders.
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let i = x + y * w;
                self.div[i] =
                    -0.5 * (self.u[i + 1] - self.u[i - 1] + self.v[i + w] - self.v[i - w]);
                self.tmp[i] = 0.0;
            }
        }
        for _ in 0..10 {
            for y in 1..h - 1 {
                for x in 1..w - 1 {
                    let i = x + y * w;
                    self.tmp[i] = (self.div[i]
                        + self.tmp[i - 1]
                        + self.tmp[i + 1]
                        + self.tmp[i - w]
                        + self.tmp[i + w])
                        * 0.25;
                }
            }
        }
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let i = x + y * w;
                self.u[i] -= 0.5 * (self.tmp[i + 1] - self.tmp[i - 1]);
                self.v[i] -= 0.5 * (self.tmp[i + w] - self.tmp[i - w]);
            }
        }

        // Advect the dye with the flow.
        self.d0.copy_from_slice(&self.d);
        advect(&self.u, &self.v, &self.d0, &mut self.d, w, h, dt * 8.0);

        // Displacement field: advect it as well, then accumulate velocity, so the
        // wisps curl in texture space and not only in the density field.
        self.ox0.copy_from_slice(&self.ox);
        self.oy0.copy_from_slice(&self.oy);
        advect(&self.u, &self.v, &self.ox0, &mut self.ox, w, h, dt * 8.0);
        advect(&self.u, &self.v, &self.oy0, &mut self.oy, w, h, dt * 8.0);
        for i in 0..self.ox.len() {
            self.ox[i] = ((self.ox[i] + self.u[i] * dt * 8.0) * 0.994).clamp(-10.0, 10.0);
            self.oy[i] = ((self.oy[i] + self.v[i] * dt * 8.0) * 0.994).clamp(-10.0, 10.0);
        }

        // Sources and dissipation: emit along the contour band with a noise-modulated
        // target, then decay by direction, slower upward.
        //
        // Emission covers only the band outside the contour. Emitting from the whole
        // interior would let buoyancy pile the smoke against the top edge, leaving a
        // thin visible line and a gap outside. The interior is hidden by the bar
        // anyway, only the outer ring has to be complete.
        let (ix0, iy0) = (self.bar.0, self.bar.1);
        let (ix1, iy1) = (self.bar.0 + self.bar.2, self.bar.1 + self.bar.3);
        let dk_top = tune.decay_top();
        let dk_side = tune.decay_side();
        for y in 0..h {
            for x in 0..w {
                let i = x + y * w;
                let (xf, yf) = (x as f32, y as f32);
                if xf >= ix0 && xf < ix1 && yf >= iy0 && yf < iy1 {
                    // Band weight; the draw stage reuses the same mask.
                    let band = self.edge[i];
                    let target = band
                        * (0.12 + 0.85 * noise.source(0, xf * 1.6 + self.seed * 2.0, yf * 1.6, t))
                        * tune.density;
                    // Cells with a zero target use the same convergence, so dye inside
                    // the bar fades out instead of silting up.
                    //
                    // 0.10 convergence rate: the flow sweeps the band in about one frame
                    // (|v| ~ 7 cells/frame); a lower rate stays half saturated and shows
                    // as a grey-violet smear.
                    self.d[i] += (target - self.d[i]) * 0.10;
                } else if yf < iy0 {
                    self.d[i] *= dk_top;
                } else {
                    self.d[i] *= dk_side;
                }
            }
        }
    }
}

/// Contour band mask: 1 near the bar edge, fading 3 cells inward and `reach` outward.
///
/// Shaped as a capsule from the distance to the axis, like the D2D rounded-rect strokes.
pub(super) fn build_edge(
    w: usize,
    h: usize,
    bar: (f32, f32, f32, f32),
    thickness: f32,
) -> Vec<f32> {
    // `bar` is already in cells and is the capsule rect. Deriving it from grid minus
    // margins would assume one centered rectangle and give right angles as soon as the
    // bar has rounded corners or a widget strip beside it.
    let (ix0, iy0) = (bar.0, bar.1);
    let ix1 = bar.0 + bar.2;
    let iy1 = bar.1 + bar.3;
    let ih = (iy1 - iy0).max(1.0);
    let rr = ih * 0.5;
    let cyc = iy0 + rr;
    // Inner falloff in cells; 3 cells = 15 px, about half the bar corner radius.
    // Wider pushes smoke into the icon area, narrower leaves a hard line at the edge.
    const INNER: f32 = 3.0;
    let mut out = vec![0.0_f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let (xf, yf) = (x as f32, y as f32);
            let qx = (ix0 + rr - xf).max(xf - (ix1 - rr)).max(0.0);
            let qy = (yf - cyc).abs();
            let sd = (qx * qx + qy * qy).sqrt() - rr;
            // 6 cells above, 3 below: smoke rises, this is smoke and not fog.
            let reach = if yf < cyc { 6.0 } else { 3.0 } * thickness;
            let t = if sd <= 0.0 {
                (1.0 + sd / INNER).clamp(0.0, 1.0)
            } else {
                (1.0 - sd / reach).clamp(0.0, 1.0)
            };
            out[y * w + x] = t * t * (3.0 - 2.0 * t);
        }
    }
    out
}

/// Bilinear sample; `cx` / `cy` are clamped to `[0, w-1.001]`, so callers need no check.
pub(super) fn bilinear(f: &[f32], w: usize, h: usize, cx: f32, cy: f32) -> f32 {
    // Bilinear reads x0+1 and y0+1, so a grid smaller than 2 would clamp to a
    // negative upper bound and panic.
    if w < 2 || h < 2 {
        return 0.0;
    }
    let cx = cx.clamp(0.0, w as f32 - 1.001);
    let cy = cy.clamp(0.0, h as f32 - 1.001);
    let x0 = cx as usize;
    let y0 = cy as usize;
    let j = x0 + y0 * w;
    let fx = cx - x0 as f32;
    let fy = cy - y0 as f32;
    (f[j] * (1.0 - fx) + f[j + 1] * fx) * (1.0 - fy)
        + (f[j + w] * (1.0 - fx) + f[j + w + 1] * fx) * fy
}

/// Semi-Lagrangian advection: trace back along `-dt * (u, v)` and sample bilinearly.
///
/// `src` and `dst` must not alias; `vel_u` / `vel_v` may be `src` itself or an
/// older frame.
pub(super) fn advect(
    vel_u: &[f32],
    vel_v: &[f32],
    src: &[f32],
    dst: &mut [f32],
    w: usize,
    h: usize,
    dt: f32,
) {
    // Same guard as `bilinear`: degenerate grids must not clamp to a negative bound.
    if w < 2 || h < 2 {
        return;
    }
    for y in 0..h {
        for x in 0..w {
            let i = x + y * w;
            let mut px = x as f32 - dt * vel_u[i];
            let mut py = y as f32 - dt * vel_v[i];
            px = px.clamp(0.0, w as f32 - 1.001);
            py = py.clamp(0.0, h as f32 - 1.001);
            let x0 = px as usize;
            let y0 = py as usize;
            let j = x0 + y0 * w;
            let fx = px - x0 as f32;
            let fy = py - y0 as f32;
            dst[i] = (src[j] * (1.0 - fx) + src[j + 1] * fx) * (1.0 - fy)
                + (src[j + w] * (1.0 - fx) + src[j + w + 1] * fx) * fy;
        }
    }
}
