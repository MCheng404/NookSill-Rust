//! Desktop capture sampling: colour field, dirty columns and tracing.

use super::*;

/// Low-frequency colour grid, used only to follow the desktop hue.
pub const GRID_W: usize = 24;

pub const GRID_H: usize = 16;

/// Average RGB per tile (0..255).
pub type ColorField = Vec<[f32; 3]>;

/// Reduces a capture to [`GRID_W`] x [`GRID_H`] average colours.
///
/// The field carries low frequencies only; the base texture keeps the high
/// frequency detail of a real gaussian blur. Every pixel is accumulated
/// rather than sparsely sampled, because a skipped tile would come out black
/// after the reprojection.
#[must_use]
pub fn color_field(bits: &[u8], w: usize, h: usize) -> ColorField {
    let mut sums = vec![[0f64; 3]; GRID_W * GRID_H];
    let mut counts = vec![0u32; GRID_W * GRID_H];
    if w == 0 || h == 0 || bits.len() < w.saturating_mul(h).saturating_mul(4) {
        return vec![[0.0; 3]; GRID_W * GRID_H];
    }
    for y in 0..h {
        let gy = (y * GRID_H / h).min(GRID_H - 1);
        for x in 0..w {
            let gx = (x * GRID_W / w).min(GRID_W - 1);
            let off = (y * w + x) * 4;
            let k = gy * GRID_W + gx;
            sums[k][0] += f64::from(bits[off]);
            sums[k][1] += f64::from(bits[off + 1]);
            sums[k][2] += f64::from(bits[off + 2]);
            counts[k] += 1;
        }
    }
    // An image narrower than the grid leaves empty tiles; fill them with the
    // global average rather than leaving black holes.
    let total: u32 = counts.iter().sum();
    let global = [
        sums.iter().map(|s| s[0]).sum::<f64>(),
        sums.iter().map(|s| s[1]).sum::<f64>(),
        sums.iter().map(|s| s[2]).sum::<f64>(),
    ]
    .map(|v| (v / f64::from(total.max(1))) as f32);
    sums.iter()
        .zip(&counts)
        .map(|(s, &n)| {
            if n == 0 {
                global
            } else {
                let k = 1.0 / f64::from(n);
                [(s[0] * k) as f32, (s[1] * k) as f32, (s[2] * k) as f32]
            }
        })
        .collect()
}

/// Returns the mean absolute difference of two colour fields (0..255), used
/// to decide whether a real blur has to run again.
#[must_use]
pub fn field_delta(a: &ColorField, b: &ColorField) -> f32 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let mut sum = 0f32;
    for (p, q) in a[..n].iter().zip(&b[..n]) {
        for c in 0..3 {
            sum += (p[c] - q[c]).abs();
        }
    }
    sum / (n * 3) as f32
}

/// Returns the column range `[x0, x1)` that changed between two frames.
///
/// A gaussian is a local operator: a change in source column x can only
/// affect output columns `[x-r, x+r]`. The caller widens the range by the
/// radius and dispatches only that strip; the rest of the texture stays.
pub(super) fn dirty_columns(prev: &[u8], cur: &[u8], w: usize, h: usize) -> (u32, u32) {
    let need = w.saturating_mul(h).saturating_mul(4);
    if w == 0 || h == 0 || prev.len() < need || cur.len() < need {
        return (0, w as u32);
    }
    let (mut x0, mut x1) = (w, 0usize);
    for y in 0..h {
        let base = y * w * 4;
        let a = &prev[base..base + w * 4];
        let b = &cur[base..base + w * 4];
        for (x, (p, q)) in a
            .as_chunks::<4>()
            .0
            .iter()
            .zip(b.as_chunks::<4>().0)
            .enumerate()
        {
            if p != q {
                x0 = x0.min(x);
                x1 = x1.max(x);
            }
        }
    }
    if x0 > x1 {
        return (0, w as u32);
    }
    (x0 as u32, (x1 as u32 + 1).min(w as u32))
}

/// `NOOKSILL_FROST_TRACE=1`: print one line of size, mean colour and timing
/// per produced frame.
///
/// Put a colour-changing window behind the panel: the mean colour has to
/// follow it. A mean that never moves means the capture still shows the
/// pre-open frame, or the panel captured itself.
pub(super) fn trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NOOKSILL_FROST_TRACE").is_ok_and(|v| v.trim() != "0"))
}

/// Counts the pixels that changed between two frames, for diagnostics only.
pub(super) fn count_diff(a: &[u8], b: &[u8]) -> usize {
    a.as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .filter(|(p, q)| p != q)
        .count()
}

/// Prints a frame summary, called only while [`trace`] is on.
pub(super) fn report(seq: u64, bits: &[u8], w: u32, h: u32, spent: Duration, diff: usize) {
    let (mut r, mut g, mut b) = (0u64, 0u64, 0u64);
    let px = bits.as_chunks::<4>().0;
    for p in px {
        r += u64::from(p[0]);
        g += u64::from(p[1]);
        b += u64::from(p[2]);
    }
    let n = px.len().max(1) as u64;
    eprintln!(
        "[frost] #{seq} {w}x{h} 平均色=({},{},{}) 变化像素={diff}/{} 抓+模糊={} ms",
        r / n,
        g / n,
        b / n,
        px.len(),
        spent.as_millis()
    );
}
