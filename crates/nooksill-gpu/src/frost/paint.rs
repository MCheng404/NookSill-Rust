//! Frost painting: panel background, blur and composition.

use super::*;

/// Returns the panel background colour.
///
/// With frost on it drops to about 75% opacity, so the blurred desktop shows
/// through while the text keeps enough contrast.
#[must_use]
pub fn panel_bg(dark: bool, frost: bool) -> Color32 {
    let (r, g, b) = if dark { (24, 25, 30) } else { (246, 247, 250) };
    // 200/190 lets at most 22% of the wallpaper through: the blur stays
    // visible but the text contrast is predictable, like the Win11 acrylic
    // tint.
    let a = if frost {
        if dark {
            200
        } else {
            190
        }
    } else {
        250
    };
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Pixel coverage of a rounded rect (0..=255).
///
/// `corner` is a radius in image pixels, not logical points. The SDF is
/// negative inside the shape, so the coverage uses `0.5 - sdf` for a one
/// pixel feather; a binary `sdf < 0` test would give jagged corners.
#[must_use]
pub fn rounded_coverage(x: f32, y: f32, w: f32, h: f32, corner: f32) -> u8 {
    // `glass_sdf` is constant at `-radius` inside the shape, so a zero
    // radius would give a constant coverage of 50%. Square panels need no
    // SDF, they are fully opaque.
    if corner <= 0.0 {
        return 255;
    }
    let half = [w * 0.5, h * 0.5];
    let p = [x + 0.5 - half[0], y + 0.5 - half[1]];
    let sdf = glass_sdf(p, half, corner);
    let cov = (0.5 - sdf).clamp(0.0, 1.0);
    (cov * 255.0).round() as u8
}

/// Separable gaussian blur over RGBA, with clamped edges.
///
/// Two 1D passes cut the cost from `O(r^2)` to `O(2r)`. The weights come
/// from `glass_backdrop::blur::normalized_gaussian_weights`, the same
/// definition as the WGSL convolution kernel upstream.
///
/// Splitting into planes keeps the inner loop a dot product over contiguous
/// slices; in interleaved RGBA the same channel sits 4 bytes apart and the
/// compiler cannot vectorise. The second pass writes transposed, so neither
/// pass needs extra transpose memory.
#[must_use]
pub fn blur_rgba(src: &[u8], w: usize, h: usize, radius: u32) -> Vec<u8> {
    let need = w.saturating_mul(h).saturating_mul(4);
    if w == 0 || h == 0 || src.len() < need {
        return Vec::new();
    }
    let r = clamp_radius(radius) as usize;
    let weights = normalized_gaussian_weights(radius, sigma_for_radius(radius));
    let n = w * h;

    let mut plane = [vec![0.0f32; n], vec![0.0f32; n], vec![0.0f32; n]];
    for i in 0..n {
        plane[0][i] = f32::from(src[i * 4]);
        plane[1][i] = f32::from(src[i * 4 + 1]);
        plane[2][i] = f32::from(src[i * 4 + 2]);
    }

    // Horizontal: written transposed, so `mid[x * h + y]` holds the y-th
    // value of source column x.
    let mut mid = [vec![0.0f32; n], vec![0.0f32; n], vec![0.0f32; n]];
    for c in 0..3 {
        for y in 0..h {
            conv_row(
                &plane[c][y * w..(y + 1) * w],
                &mut mid[c],
                y,
                h,
                &weights,
                r,
            );
        }
    }

    // Vertical: walk the columns as rows, also written transposed, which
    // restores the normal w x h layout.
    let mut out = [vec![0.0f32; n], vec![0.0f32; n], vec![0.0f32; n]];
    for c in 0..3 {
        for x in 0..w {
            conv_row(&mid[c][x * h..(x + 1) * h], &mut out[c], x, w, &weights, r);
        }
    }

    let mut rgba = vec![0u8; need];
    for i in 0..n {
        rgba[i * 4] = out[0][i].round().clamp(0.0, 255.0) as u8;
        rgba[i * 4 + 1] = out[1][i].round().clamp(0.0, 255.0) as u8;
        rgba[i * 4 + 2] = out[2][i].round().clamp(0.0, 255.0) as u8;
        rgba[i * 4 + 3] = 255;
    }
    rgba
}

/// One 1D convolution row: `out[base + x * stride] = sum row[x - r + k] * weights[k]`.
///
/// The `r` pixels on each side are out of bounds and clamp to the edge,
/// extending the border pixels. The middle section has neither clamp nor
/// branch, and is the only part that has to run fast.
pub(super) fn conv_row(
    row: &[f32],
    out: &mut [f32],
    base: usize,
    stride: usize,
    weights: &[f32],
    r: usize,
) {
    let w = row.len();
    let taps = weights.len();
    if w >= taps {
        for x in 0..r {
            out[base + x * stride] = clamp_taps(row, x, r, weights);
        }
        for x in r..w - r {
            let win = &row[x - r..x - r + taps];
            let mut acc = 0.0f32;
            for (a, b) in win.iter().zip(weights) {
                acc += a * b;
            }
            out[base + x * stride] = acc;
        }
        for x in w - r..w {
            out[base + x * stride] = clamp_taps(row, x, r, weights);
        }
    } else {
        // The row is narrower than the kernel, so every pixel clamps and
        // there is no middle section.
        for x in 0..w {
            out[base + x * stride] = clamp_taps(row, x, r, weights);
        }
    }
}

/// 1D convolution with clamped taps, used only for the edges and narrow rows.
#[inline]
pub(super) fn clamp_taps(row: &[f32], x: usize, r: usize, weights: &[f32]) -> f32 {
    let last = row.len() - 1;
    let mut acc = 0.0f32;
    for (k, wt) in weights.iter().enumerate() {
        let sx = (x + k).saturating_sub(r).min(last);
        acc += row[sx] * wt;
    }
    acc
}

/// Writes the rounded alpha and the premultiplied colour into the blur result.
///
/// Only the four corners need an SDF: outside the corner radius the coverage
/// is always 255. Running the SDF over the whole image would be wasted work
/// on the live path, which runs ten times per second.
pub(crate) fn apply_round_alpha(rgba: &mut [u8], w: usize, h: usize, corner: f32) {
    if corner <= 0.0 {
        return; // `blur_rgba` already wrote alpha 255
    }
    let (fw, fh) = (w as f32, h as f32);
    let n = (corner.ceil() as usize + 1).min(w).min(h);
    if 2 * n > w || 2 * n > h {
        // The panel is small enough that the corners overlap: fall back to a
        // full scan, so no pixel gets its alpha applied twice.
        for y in 0..h {
            for x in 0..w {
                let a = rounded_coverage(x as f32, y as f32, fw, fh, corner);
                put_alpha(rgba, (y * w + x) * 4, a);
            }
        }
        return;
    }
    for y in 0..n {
        for x in 0..n {
            for (px, py) in [
                (x, y),
                (w - 1 - x, y),
                (x, h - 1 - y),
                (w - 1 - x, h - 1 - y),
            ] {
                let a = rounded_coverage(px as f32, py as f32, fw, fh, corner);
                put_alpha(rgba, (py * w + px) * 4, a);
            }
        }
    }
}

/// Writes one pixel's alpha and multiplies it into the RGB.
///
/// egui's `ColorImage` reads the result as premultiplied: with alpha 0
/// outside the corner, leftover bright RGB would bleed into the corner
/// during linear sampling.
pub(super) fn put_alpha(rgba: &mut [u8], off: usize, a: u8) {
    rgba[off + 3] = a;
    if a < 255 {
        let k = f32::from(a) / 255.0;
        for c in 0..3 {
            rgba[off + c] = (f32::from(rgba[off + c]) * k).round() as u8;
        }
    }
}

/// CPU fallback: captured bytes -> blur -> rounded alpha -> egui image.
/// Also the golden reference for the GPU compute path in tests.
pub(crate) fn compose(bits: &[u8], w: u32, h: u32, spec: Spec, ppp: f32) -> Option<ColorImage> {
    let (w, h) = (w as usize, h as usize);
    let need = w.checked_mul(h)?.checked_mul(4)?;
    if w == 0 || h == 0 || bits.len() < need {
        return None;
    }
    let mut rgba = blur_rgba(&bits[..need], w, h, spec.radius);
    if rgba.is_empty() {
        return None;
    }
    let scale = f32::from(CAPTURE_PCT) / 100.0;
    apply_round_alpha(&mut rgba, w, h, spec.corner_pt * ppp * scale);
    Some(ColorImage::from_rgba_premultiplied([w, h], &rgba))
}
