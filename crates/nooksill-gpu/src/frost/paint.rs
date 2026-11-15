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

/// Separable gaussian blur over RGBA, with normalised edges.
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
/// Edge pixels use [`edge_taps`], which drops out-of-bounds taps instead of
/// repeating the border. The middle section has no branch and is the only part
/// that has to run fast.
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
            out[base + x * stride] = edge_taps(row, x, r, weights);
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
            out[base + x * stride] = edge_taps(row, x, r, weights);
        }
    } else {
        // The row is narrower than the kernel, so every pixel clamps and
        // there is no middle section.
        for x in 0..w {
            out[base + x * stride] = edge_taps(row, x, r, weights);
        }
    }
}

/// 1D convolution at the edges: out-of-bounds taps are dropped, not clamped.
///
/// Clamping repeats the border pixel. At a corner both passes clamp, so the
/// whole corner becomes one flat colour patch once the blur is upscaled.
/// Renormalising by the weights that were actually used keeps the average
/// brightness and continues the desktop texture into the corner.
#[inline]
pub(super) fn edge_taps(row: &[f32], x: usize, r: usize, weights: &[f32]) -> f32 {
    let mut acc = 0.0f32;
    let mut wsum = 0.0f32;
    for (k, wt) in weights.iter().enumerate() {
        let sx = (x + k) as isize - r as isize;
        if sx >= 0 && (sx as usize) < row.len() {
            acc += row[sx as usize] * wt;
            wsum += *wt;
        }
    }
    if wsum > 0.0 {
        acc / wsum
    } else {
        0.0
    }
}

/// One 8-bit step of deterministic noise before quantisation.
///
/// The blur texture is 8-bit and gets upscaled four times, so a smooth
/// gradient shows contour bands. Wrapping `u32` math matches `frost.wgsl`
/// exactly, which keeps the CPU/GPU comparison test meaningful.
pub(crate) fn apply_dither(rgba: &mut [u8], w: usize, h: usize) {
    if w == 0 || h == 0 {
        return;
    }
    let n = (w * h).min(rgba.len() / 4);
    for i in 0..n {
        let d = dither_offset((i % w) as u32, (i / w) as u32);
        for c in 0..3 {
            let v = f32::from(rgba[i * 4 + c]) + d;
            rgba[i * 4 + c] = v.round().clamp(0.0, 255.0) as u8;
        }
    }
}

fn dither_offset(x: u32, y: u32) -> f32 {
    let mut h = x
        .wrapping_mul(1_664_525)
        .wrapping_add(y.wrapping_mul(1_013_904_223));
    h = (h ^ (h >> 16)).wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(3_266_489_917);
    h ^= h >> 16;
    f32::from(((h >> 24) & 255) as u8) / 255.0 - 0.5
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
    // Dither before the rounded alpha: the corner mask then premultiplies the
    // noisy colour to zero, so the transparent corners stay clean.
    apply_dither(&mut rgba, w, h);
    let scale = f32::from(CAPTURE_PCT) / 100.0;
    apply_round_alpha(&mut rgba, w, h, spec.corner_pt * ppp * scale);
    Some(ColorImage::from_rgba_premultiplied([w, h], &rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uniform(taps: usize) -> Vec<f32> {
        vec![1.0 / taps as f32; taps]
    }

    #[test]
    fn edge_taps_renormalises_instead_of_repeating_the_border() {
        let row: Vec<f32> = (0..16).map(|v| v as f32).collect();
        let w = uniform(5);
        // x=0 keeps sources 0,1,2: normalised that is 1.0, clamping gave 0.6.
        let left = edge_taps(&row, 0, 2, &w);
        assert!((left - 1.0).abs() < 1e-5, "left edge got {left}");
        // The right edge drops the last two taps instead of repeating row[15].
        let right = edge_taps(&row, row.len() - 1, 2, &w);
        let want = (13.0 + 14.0 + 15.0) / 3.0;
        assert!((right - want).abs() < 1e-5, "right edge got {right}");
    }

    #[test]
    fn edge_taps_matches_a_full_convolution_when_all_taps_are_inside() {
        let row: Vec<f32> = (0..16).map(|v| v as f32).collect();
        let w = uniform(5);
        let x = 8;
        let want: f32 = w
            .iter()
            .enumerate()
            .map(|(k, wt)| row[x + k - 2] * wt)
            .sum();
        let got = edge_taps(&row, x, 2, &w);
        assert!((got - want).abs() < 1e-5);
    }

    #[test]
    fn compose_corners_are_transparent_and_premultiplied() {
        let (w, h) = (120usize, 80usize);
        let mut bits = vec![0u8; w * h * 4];
        for px in bits.as_chunks_mut::<4>().0.iter_mut() {
            px.copy_from_slice(&[90, 120, 200, 255]);
        }
        let spec = Spec {
            radius: 4,
            corner_pt: 18.0,
        };
        let img = compose(&bits, w as u32, h as u32, spec, 1.0).expect("compose");
        for idx in [0, w - 1, (h - 1) * w, h * w - 1] {
            let p = img.pixels[idx];
            assert_eq!(p.a(), 0, "corner {idx} must be transparent");
            assert_eq!(
                [p.r(), p.g(), p.b()],
                [0, 0, 0],
                "corner {idx} must be premultiplied"
            );
        }
        assert_eq!(img.pixels[(h / 2) * w + w / 2].a(), 255);
    }
}
