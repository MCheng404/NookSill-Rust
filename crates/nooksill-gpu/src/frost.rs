//! Frost background: GDI capture -> GPU separable gaussian -> egui texture.
//!
//! `glass_backdrop` supplies the capture (BitBlt), the gaussian weights and
//! the rounded-rect SDF. The blur runs in `blur_gpu` (WGSL compute) and the
//! result is registered with egui directly, so there is no GPU->CPU readback.
//! `blur_rgba` and `compose` here are the CPU fallback and the test reference.
//!
//! Capture resolution is `CAPTURE_PCT` of the client area. A visible panel is
//! excluded from screen capture, otherwise GDI would capture the panel itself.
//!
//! `Live` polls fast while the desktop moves and backs off on a static
//! desktop; a cheap pixel probe detects change during the backoff, so the
//! reaction time stays at probe latency instead of a full idle interval.
//! Captured bytes are shared with the UI thread via `Arc`, and each frame
//! carries its changed column range so the convolution only recomputes the
//! columns that actually changed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use egui::{Color32, ColorImage, TextureId};
use glass_backdrop::blur::{clamp_radius, normalized_gaussian_weights, sigma_for_radius};
use glass_backdrop::capture::DesktopCapture;
use glass_backdrop::glass::glass_sdf;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{ClientToScreen, GetDC, GetPixel, ReleaseDC, HDC};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
};

use crate::frame::FrameRenderer;
use crate::worker::Waker;

/// Capture resolution as a percentage of the client area; 25% is visually
/// indistinguishable after the blur but costs 16 times fewer pixels.
pub const CAPTURE_PCT: u8 = 25;

/// Capture interval while the desktop is changing (20 Hz).
///
/// The probe catches the first change; this interval governs the continuous
/// follow-up. Capture itself is the expensive part, so going faster pays off
/// little.
const LIVE_ACTIVE: Duration = Duration::from_millis(50);

/// Initial interval after a capture saw no change (10 Hz); doubles on every
/// further idle round.
const LIVE_INTERVAL: Duration = Duration::from_millis(100);

/// Idle backoff ceiling. [`PROBE_INTERVAL`] keeps it from being a perceived
/// delay: the next probe tick wakes a full capture as soon as the desktop
/// changes.
const LIVE_IDLE_MAX: Duration = Duration::from_millis(800);

/// Pixel probe period; reading 20 points costs well under 0.1 ms.
const PROBE_INTERVAL: Duration = Duration::from_millis(25);

/// Minimum gap between full captures, so a busy desktop cannot turn every
/// probe tick into a capture.
const MIN_CAPTURE_GAP: Duration = Duration::from_millis(40);

/// Fallback for probes that cannot see changes (some remote sessions): force
/// one full capture at this interval.
const PROBE_FORCE_GAP: Duration = Duration::from_millis(300);

/// Experimental "visual illusion" blur switch. Process-wide: the main loop
/// syncs it from config every frame.
static ILLUSION: AtomicBool = AtomicBool::new(false);

/// Switch the experimental blur path. The gaussian path stays available.
pub fn set_illusion(on: bool) {
    ILLUSION.store(on, Ordering::Relaxed);
}

/// Whether the experimental blur path is on.
#[must_use]
pub fn illusion() -> bool {
    ILLUSION.load(Ordering::Relaxed)
}

/// Frost parameters for one window open. `None` = the feature is off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    /// Gaussian radius in capture-image texels; from `Blur`.
    pub radius: u32,
    /// Panel corner radius in logical points; the launchpad is square, pass 0.
    ///
    /// The dock passes 0 as well: it rounds each piece of its geometry itself,
    /// see [`Backdrop`].
    pub corner_pt: f32,
}

/// A registered blur texture plus the window it covers.
///
/// The texture always spans the whole window, so a piece of geometry can
/// sample the slice of the blur behind itself: the dock draws one backdrop per
/// bar-shaped rect instead of covering the window, which would also frost the
/// zoom overhang and the reflection margin. UV of a point is its position as a
/// fraction of `win`, so the mapping survives a resize or a DPI change.
#[derive(Clone, Copy, Debug)]
pub struct Backdrop {
    pub tex: TextureId,
    /// Window size in logical points.
    pub win: [f32; 2],
}

impl Spec {
    /// Capture once before the panel is shown. Falls back to CPU blur when the
    /// GPU pipeline is unavailable.
    pub fn apply(self, renderer: &mut FrameRenderer, hwnd: HWND, ppp: f32) {
        let mut cap = match DesktopCapture::new(true) {
            Ok(c) => c,
            Err(_) => {
                renderer.clear_frost();
                return;
            }
        };
        let captured = cap
            .capture_behind_pct(hwnd.0 as isize, CAPTURE_PCT)
            .ok()
            .flatten();
        let Some((bits, w, h)) = captured else {
            renderer.clear_frost();
            return;
        };
        if illusion() && renderer.set_frost_illusion(bits, w, h, self, ppp) {
            return;
        }
        if renderer.set_frost_gpu(bits, w, h, self, ppp) {
            return;
        }
        match compose(bits, w, h, self, ppp) {
            Some(image) => renderer.set_frost(image),
            None => renderer.clear_frost(),
        }
    }
}

/// Exclude the window from screen capture.
///
/// Must run before `ShowWindow`: afterwards that frame captures the panel on
/// top of the desktop, and the mirrored copy shows up in the blur.
pub fn exclude_from_capture(hwnd: HWND) {
    // Only fails on systems older than Win10 2004, which lack the flag; the
    // caller decides whether to enable frost anyway.
    unsafe {
        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
    }
}

/// Let the window be captured again (called while the panel is hidden).
pub fn allow_capture(hwnd: HWND) {
    unsafe {
        let _ = SetWindowDisplayAffinity(hwnd, WDA_NONE);
    }
}

mod capture;
mod live;
mod paint;

pub use self::capture::{color_field, field_delta, ColorField, GRID_H, GRID_W};
pub use self::live::{FrostFrame, Live};
#[cfg(test)]
pub(crate) use self::paint::apply_round_alpha;
pub(crate) use self::paint::compose;
pub use self::paint::{blur_rgba, panel_bg, rounded_coverage};

#[cfg(test)]
mod tests {
    use super::capture::{color_field, dirty_columns, field_delta, GRID_H, GRID_W};
    use super::live::{merge_frames, FrostFrame};
    use super::paint::{apply_round_alpha, blur_rgba, panel_bg, rounded_coverage};
    use super::*;

    /// A constant image stays constant (the weights are normalized to 1).
    #[test]
    fn constant_image_stays_constant() {
        let src = vec![90u8; 5 * 5 * 4];
        let out = blur_rgba(&src, 5, 5, 4);
        assert_eq!(out.len(), src.len());
        assert_eq!(out[0], 90);
        assert_eq!(out[1], 90);
        assert_eq!(out[2], 90);
        assert_eq!(out[3], 255);
    }

    /// A single point spreads out and conserves energy.
    #[test]
    fn impulse_spreads_and_conserves_energy() {
        let w = 9;
        let h = 9;
        let mut src = vec![0u8; w * h * 4];
        for px in src.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        src[(4 * w + 4) * 4] = 255;
        let out = blur_rgba(&src, w, h, 3);
        let sum: u32 = out.as_chunks::<4>().0.iter().map(|p| u32::from(p[0])).sum();
        assert!((220..290).contains(&sum), "能量应接近 255，实际 {sum}");
        assert!(out[(4 * w + 4) * 4] < 255, "中心应当被摊薄");
        assert!(out[(4 * w + 3) * 4] > 0, "核半径内的邻居该分到能量");
        assert_eq!(out[0], 0, "核半径外不该有能量");
    }

    /// Non-square images blur correctly: the transposed write must not mix up
    /// the axes.
    #[test]
    fn non_square_keeps_axes_straight() {
        let (w, h) = (24usize, 8usize);
        let mut src = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                src[(y * w + x) * 4] = if y == h / 2 { 200 } else { 0 };
            }
        }
        let out = blur_rgba(&src, w, h, 2);
        let row = |y: usize| u32::from(out[(y * w + w / 2) * 4]);
        assert!(row(h / 2) > 100, "亮带应留在原来的行上");
        assert!(row(0) < 20 && row(h - 1) < 20, "角落不该被点亮");
    }

    /// Matches a naive 2D convolution pixel by pixel (+/-1 rounding).
    ///
    /// This pins the separable, planar, transposed implementation against the
    /// plain `sum row[y][x] * w * w` reference over a non-square image and an
    /// odd radius.
    #[test]
    fn separable_blur_matches_naive_2d() {
        let (w, h, radius) = (13usize, 9usize, 2u32);
        // Deterministic pseudo-random content (no external crate, no time).
        let mut seed = 0x1234_5678u32;
        let mut src = vec![0u8; w * h * 4];
        for (i, px) in src.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let mut next = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 24) as u8
            };
            px[0] = next();
            px[1] = next();
            px[2] = next();
            px[3] = 255;
            let _ = i;
        }

        let fast = blur_rgba(&src, w, h, radius);
        let weights = normalized_gaussian_weights(radius, sigma_for_radius(radius));
        let r = clamp_radius(radius) as i32;
        let taps = weights.len() as i32;
        assert_eq!(taps, 2 * r + 1, "权重个数必须是 2r+1");

        for y in 0..h {
            for x in 0..w {
                for c in 0..3 {
                    // Real 2D convolution: the kernel is the outer product of
                    // the two 1D weights, not a diagonal sum.
                    let mut acc = 0.0f32;
                    for (j, wy) in weights.iter().enumerate() {
                        let sy = (y as i32 + j as i32 - r).clamp(0, h as i32 - 1) as usize;
                        for (k, wx) in weights.iter().enumerate() {
                            let sx = (x as i32 + k as i32 - r).clamp(0, w as i32 - 1) as usize;
                            acc += f32::from(src[(sy * w + sx) * 4 + c]) * wy * wx;
                        }
                    }
                    let want = acc.round().clamp(0.0, 255.0) as i32;
                    let got = i32::from(fast[(y * w + x) * 4 + c]);
                    assert!(
                        (got - want).abs() <= 1,
                        "({x},{y}) 通道 {c}：可分离={got} 朴素={want}"
                    );
                }
            }
        }
    }

    /// Illegal sizes return early.
    #[test]
    fn malformed_buffers_do_not_panic() {
        assert!(blur_rgba(&[], 0, 0, 4).is_empty());
        assert!(blur_rgba(&[0u8; 4], 10, 10, 4).is_empty());
    }

    /// Rounded mask: centre and interior opaque, corners transparent, edge is
    /// a gradient.
    #[test]
    fn rounded_mask_keeps_center_and_cuts_corners() {
        let (w, h, r) = (100.0f32, 100.0f32, 12.0f32);
        assert_eq!(rounded_coverage(50.0, 50.0, w, h, r), 255);
        assert_eq!(rounded_coverage(0.0, 0.0, w, h, r), 0);
        assert_eq!(rounded_coverage(99.0, 0.0, w, h, r), 0);
        // Square corners (radius 0) must skip the SDF: it is constantly
        // -radius inside, so radius 0 would pin the panel at 50% coverage.
        assert_eq!(rounded_coverage(0.0, 0.0, w, h, 0.0), 255);
    }

    /// Corner-only alpha must equal a full scan: that is the performance
    /// premise of the live path. Too small a corner block would leave a bright
    /// rim outside the rounded edge.
    #[test]
    fn corner_only_alpha_matches_full_scan() {
        for (w, h, corner) in [(120usize, 80usize, 12.0f32), (200, 150, 7.3), (60, 60, 4.0)] {
            // Mimics `blur_rgba` output: RGB is content, alpha already 255.
            let start: Vec<u8> = (0..w * h).flat_map(|_| [200u8, 200, 200, 255]).collect();
            let mut fast = start.clone();
            apply_round_alpha(&mut fast, w, h, corner);

            // Reference: full-image scan, always premultiplied.
            let mut slow = start;
            for y in 0..h {
                for x in 0..w {
                    let a = rounded_coverage(x as f32, y as f32, w as f32, h as f32, corner);
                    let off = (y * w + x) * 4;
                    slow[off + 3] = a;
                    let k = f32::from(a) / 255.0;
                    for c in 0..3 {
                        slow[off + c] = (200.0f32 * k).round() as u8;
                    }
                }
            }
            assert_eq!(
                fast, slow,
                "{w}x{h} corner={corner} 的四角裁剪应等价于整幅扫描"
            );
        }
    }

    /// A square panel must not touch a single byte (the blur already wrote
    /// 255) and must not premultiply.
    #[test]
    fn square_panel_leaves_alpha_alone() {
        let start: Vec<u8> = (0..8 * 8).flat_map(|_| [180u8, 90, 30, 255]).collect();
        let mut buf = start.clone();
        apply_round_alpha(&mut buf, 8, 8, 0.0);
        assert_eq!(buf, start);
    }

    /// The frosted background really is more transparent than the solid one.
    #[test]
    fn frost_background_is_more_transparent() {
        assert!(panel_bg(true, true).a() < panel_bg(true, false).a());
        assert_eq!(panel_bg(true, false).a(), 250);
    }

    fn frame_with_cols(x0: u32, x1: u32, w: u32) -> FrostFrame {
        FrostFrame {
            bits: Arc::new(vec![0u8; 4]),
            w,
            h: 1,
            x0,
            x1,
            field: Arc::new(vec![[0.0; 3]; GRID_W * GRID_H]),
        }
    }

    /// Dropped frames must union their dirty columns, otherwise the band
    /// changed by the middle frame lingers forever.
    #[test]
    fn dropped_frames_union_their_dirty_columns() {
        let merged = merge_frames(frame_with_cols(10, 20, 100), frame_with_cols(30, 40, 100));
        assert_eq!((merged.x0, merged.x1), (10, 40));
    }

    /// If any frame is full, the merge is full.
    #[test]
    fn a_full_frame_keeps_the_merge_full() {
        let merged = merge_frames(frame_with_cols(10, 20, 100), frame_with_cols(0, 100, 100));
        assert_eq!((merged.x0, merged.x1), (0, 100));
        let merged = merge_frames(frame_with_cols(0, 100, 100), frame_with_cols(30, 40, 100));
        assert_eq!((merged.x0, merged.x1), (0, 100));
    }

    /// The colour field keeps low frequencies only: a constant image is
    /// uniform everywhere, a left/right split must stay split.
    #[test]
    fn color_field_keeps_low_frequency_colour() {
        let (w, h) = (48usize, 32usize);
        let mut flat = vec![0u8; w * h * 4];
        for px in flat.as_chunks_mut::<4>().0 {
            px[0] = 200;
            px[1] = 100;
            px[2] = 50;
            px[3] = 255;
        }
        let f = color_field(&flat, w, h);
        assert_eq!(f.len(), GRID_W * GRID_H);
        assert!(f.iter().all(|c| (c[0] - 200.0).abs() < 0.01
            && (c[1] - 100.0).abs() < 0.01
            && (c[2] - 50.0).abs() < 0.01));

        let mut split = flat;
        for y in 0..h {
            for x in w / 2..w {
                let i = (y * w + x) * 4;
                split[i] = 10;
                split[i + 1] = 20;
                split[i + 2] = 230;
            }
        }
        let g = color_field(&split, w, h);
        assert!(g[0][0] > 190.0, "左半应保持红色");
        assert!(g[GRID_W - 1][2] > 220.0, "右半应变成蓝色");
        assert!(field_delta(&f, &g) > 20.0);
        assert_eq!(field_delta(&f, &f), 0.0);
    }

    /// Dirty-column detection reports only changed columns; no change and bad
    /// input both fall back to full.
    #[test]
    fn dirty_columns_finds_the_changed_band() {
        let (w, h) = (8usize, 4usize);
        let mut a = vec![0u8; w * h * 4];
        let mut b = a.clone();
        for y in 0..h {
            for x in [3usize, 5] {
                b[(y * w + x) * 4] = 9;
            }
        }
        assert_eq!(dirty_columns(&a, &b, w, h), (3, 6));
        assert_eq!(dirty_columns(&a, &a, w, h), (0, w as u32));
        assert_eq!(dirty_columns(&[], &b, w, h), (0, w as u32));
        // Different sizes (for example right after a resize) cannot be
        // compared column by column.
        assert_eq!(dirty_columns(&a, &b, w + 1, h), (0, (w + 1) as u32));
        a.truncate(4);
        assert_eq!(dirty_columns(&a, &b, w, h), (0, w as u32));
    }
}
