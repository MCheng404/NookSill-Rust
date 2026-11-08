//! Tileable smoke texture: hash, value noise, fractal sum, domain warp.

use super::*;

/// Grid noise with cubic smoothing.
fn hash(x: i32, y: i32, seed: i32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(374_761_393)
        .wrapping_add((y as u32).wrapping_mul(668_265_263))
        .wrapping_add((seed as u32).wrapping_mul(2_246_822_519));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0x00FF_FFFF) as f32 / 16_777_215.0
}

fn vnoise(x: f32, y: f32, period: i32, seed: i32) -> f32 {
    let xi = x.floor();
    let yi = y.floor();
    let xf = x - xi;
    let yf = y - yi;
    // `3t^2 - 2t^3` has a discontinuous first derivative at the lattice points,
    // but 5 octaves hide it and it costs half of Perlin's quintic.
    let u = xf * xf * (3.0 - 2.0 * xf);
    let v = yf * yf * (3.0 - 2.0 * yf);
    let (xi, yi) = (xi as i32, yi as i32);
    let wrap = |q: i32| ((q % period) + period) % period;
    let (x0, y0) = (wrap(xi), wrap(yi));
    let (x1, y1) = (wrap(xi + 1), wrap(yi + 1));
    let a = hash(x0, y0, seed);
    let b = hash(x1, y0, seed);
    let c = hash(x0, y1, seed);
    let d = hash(x1, y1, seed);
    a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v
}

/// Five octaves of fractal noise.
fn fbm(x: f32, y: f32, period: i32, seed: i32) -> f32 {
    let mut total = 0.0;
    let mut amp = 0.5;
    for octave in 0..5 {
        let fi = 1 << octave;
        total += amp * vnoise(x * fi as f32, y * fi as f32, period * fi, seed + octave);
        amp *= 0.5;
    }
    total
}

/// Three tileable smoke layers, each `n x n` of alpha.
///
/// Built by domain warping: one FBM layer offsets the sample position of the
/// third, turning straight noise bands into curls.
#[derive(Clone)]
pub struct Noise {
    pub(super) n: usize,
    pub(super) alpha: [Vec<f32>; 3],
    pub(super) mean: [f32; 3],
}

impl std::fmt::Debug for Noise {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Noise")
            .field("n", &self.n)
            .field("mean", &self.mean)
            .finish()
    }
}

impl Noise {
    /// Builds with the default edge length; about 40 ms in release, once at startup.
    #[must_use]
    pub fn build() -> Self {
        Self::build_sized(NOISE_N)
    }

    /// Builds with an explicit edge length, for tests: tiling is independent of it
    /// and small sizes keep tests fast.
    #[must_use]
    pub fn build_sized(n: usize) -> Self {
        const BASIS: [i32; 3] = [4, 3, 6];
        const SEED: [i32; 3] = [11, 53, 97];
        let n = n.max(8);
        let mut alpha: [Vec<f32>; 3] = std::array::from_fn(|_| Vec::new());
        let mut mean = [0.0_f32; 3];
        for (k, buf) in alpha.iter_mut().enumerate() {
            buf.resize(n * n, 0.0);
            let bs = BASIS[k];
            let sd = SEED[k];
            let mut sum = 0.0_f64;
            for j in 0..n {
                for i in 0..n {
                    let x = i as f32 / n as f32 * bs as f32;
                    let y = j as f32 / n as f32 * bs as f32;
                    let qx = fbm(x, y, bs, sd);
                    let qy = fbm(x + 5.2, y + 1.3, bs, sd + 7);
                    let w = fbm(x + 3.2 * qx, y + 3.2 * qy, bs, sd + 13);
                    let mut al = ((w - 0.32) / 0.5).clamp(0.0, 1.0);
                    // Smoothstep turns the linear threshold into a soft edge, otherwise
                    // the wisp border is a contour line that shows as color banding.
                    al = al * al * (3.0 - 2.0 * al);
                    buf[j * n + i] = al;
                    sum += f64::from(al);
                }
            }
            mean[k] = (sum / (n * n) as f64) as f32;
        }
        Self { n, alpha, mean }
    }

    /// Mean of layer `k`. Turbulence must subtract it, or every frame adds the
    /// same bias and the smoke drifts in one direction.
    #[must_use]
    pub fn mean(&self, k: usize) -> f32 {
        self.mean[k]
    }

    #[must_use]
    pub fn size(&self) -> usize {
        self.n
    }

    /// Three tiled noise layers, used by the GPU raster shader.
    #[must_use]
    pub fn layers(&self) -> &[Vec<f32>; 3] {
        &self.alpha
    }

    /// Samples layer `k` at grid cell coordinates; `t` is speed-scaled seconds.
    /// The 0.55/0.7 and 1.2/2.0 factors make the texture drift diagonally.
    ///
    /// Turbulence and the density target both read from here.
    #[must_use]
    pub fn source(&self, k: usize, x: f32, y: f32, t: f32) -> f32 {
        self.tap(k, x * 0.55 + t * 1.2, y * 0.70 + t * 2.0)
    }

    /// Bilinear sample in texture coordinates with wrapping, no drift transform.
    ///
    /// The raster stage already offsets coordinates by the displacement field, so
    /// applying the drift again would double it.
    #[must_use]
    pub fn tap(&self, k: usize, u: f32, v: f32) -> f32 {
        let n = self.n;
        let nf = n as f32;
        let mut fx = u - (u / nf).floor() * nf;
        let mut fy = v - (v / nf).floor() * nf;
        // Rounding can push `fx` to `n` on exact division; wrap it once more.
        if fx >= nf {
            fx -= nf;
        }
        if fy >= nf {
            fy -= nf;
        }
        let x0 = fx as usize;
        let y0 = fy as usize;
        let x1 = (x0 + 1) % n;
        let y1 = (y0 + 1) % n;
        let ax = fx - x0 as f32;
        let ay = fy - y0 as f32;
        let tex = &self.alpha[k];
        let at = |xx: usize, yy: usize| tex[yy * n + xx];
        (at(x0, y0) * (1.0 - ax) + at(x1, y0) * ax) * (1.0 - ay)
            + (at(x0, y1) * (1.0 - ax) + at(x1, y1) * ax) * ay
    }
}
