//! Icon fitting: which part of a bitmap to draw, at what size, whether to
//! round the corners.
//!
//! | `fit` | Meaning | Behavior |
//! |---|---|---|
//! | 0 | Auto | Trim the alpha bounding box; if the image is fully opaque, trim the flat plate and round the corners |
//! | 1 | Full | Whole image at 94%, no crop |
//! | 2 | Square | Center square at 100% with rounded corners |
//!
//! Geometry and masking only, no resampling: the draw stage turns `bbox` into a
//! UV sub-rect and lets the GPU scale it, which tracks the grow animation
//! every frame.
//!
//! The second trim pass (`solid_bbox`) exists because an opaque icon such as
//! the Win11 Calculator has alpha 255 everywhere. An alpha-only bbox returns
//! the whole image and the dock shows a dark block; trimming the same-colored
//! plate leaves just the central glyph.

/// A rectangle. `x1` / `y1` are exclusive bounds.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BBox {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl BBox {
    #[must_use]
    pub fn width(self) -> u32 {
        self.x1.saturating_sub(self.x0)
    }

    #[must_use]
    pub fn height(self) -> u32 {
        self.y1.saturating_sub(self.y0)
    }

    #[must_use]
    pub fn is_full(self, w: u32, h: u32) -> bool {
        self.x0 == 0 && self.y0 == 0 && self.x1 == w && self.y1 == h
    }

    #[must_use]
    pub fn full(w: u32, h: u32) -> Self {
        Self {
            x0: 0,
            y0: 0,
            x1: w,
            y1: h,
        }
    }
}

/// Fit mode. The value is the `fit` field stored in the ini.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum FitMode {
    /// Trim edges and a flat plate.
    #[default]
    Auto = 0,
    /// Keep the whole image.
    Full = 1,
    /// Center crop to fill.
    Square = 2,
}

impl FitMode {
    /// Unknown codes fall back to [`FitMode::Auto`]. The original `FitBitmap`
    /// routes them to the no-crop branch; auto is the better default here.
    #[must_use]
    pub const fn from_code(v: i32) -> Self {
        match v {
            1 => Self::Full,
            2 => Self::Square,
            _ => Self::Auto,
        }
    }
}

/// Fit result.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct IconFit {
    /// Source region to draw.
    pub bbox: BBox,
    /// `Some(radius)` means the content is one opaque block needing a rounded
    /// mask. Radius is in source pixels.
    pub rounded: Option<f32>,
    /// Fraction of the target size the content fills.
    pub fill: f32,
}

/// `FitBitmap` fill parameter: 0.94 for all modes except center crop, which
/// uses 1.0.
const FILL: f32 = 0.94;
/// Corner radius as a fraction of the short edge.
const CORNER: f32 = 0.22;
/// Alpha threshold (`AlphaBBox(..., 40)`).
const ALPHA_THR: u8 = 40;

/// Compute the fit region.
#[must_use]
pub fn fit_icon(rgba: &[u8], w: u32, h: u32, mode: FitMode) -> IconFit {
    if w == 0 || h == 0 || rgba.len() < (w as usize) * (h as usize) * 4 {
        return IconFit {
            bbox: BBox::full(w, h),
            rounded: None,
            fill: FILL,
        };
    }

    match mode {
        FitMode::Full => IconFit {
            bbox: BBox::full(w, h),
            rounded: None,
            fill: FILL,
        },
        FitMode::Square => {
            // Center square on the short edge, fill = 1.0.
            let side = w.min(h);
            let bbox = BBox {
                x0: (w - side) / 2,
                y0: (h - side) / 2,
                x1: (w - side) / 2 + side,
                y1: (h - side) / 2 + side,
            };
            IconFit {
                bbox,
                rounded: Some(side as f32 * CORNER),
                fill: 1.0,
            }
        }
        FitMode::Auto => {
            let alpha = alpha_bbox(rgba, w, h, ALPHA_THR);
            if !alpha.is_full(w, h) {
                // Transparent edges: crop alpha only, no rounding.
                return IconFit {
                    bbox: alpha,
                    rounded: None,
                    fill: FILL,
                };
            }
            // Fully opaque: try trimming the flat plate and round the corners.
            match solid_bbox(rgba, w, h) {
                Some(b) => IconFit {
                    bbox: b,
                    rounded: Some(b.width().min(b.height()) as f32 * CORNER),
                    fill: FILL,
                },
                None => IconFit {
                    bbox: alpha,
                    rounded: Some(w.min(h) as f32 * CORNER),
                    fill: FILL,
                },
            }
        }
    }
}

/// Smallest bounding box of pixels with alpha above `thr`. Fully transparent
/// input returns the full image rather than an empty box, which would yield a
/// zero-sized result downstream.
#[must_use]
pub fn alpha_bbox(rgba: &[u8], w: u32, h: u32, thr: u8) -> BBox {
    let (mut minx, mut miny) = (w, h);
    // Start at -1: only then can "no hit" be told apart from "hit in column 0".
    let (mut maxx, mut maxy) = (-1i64, -1i64);
    for y in 0..h {
        let row = (y as usize) * (w as usize) * 4;
        for x in 0..w {
            if rgba[row + (x as usize) * 4 + 3] > thr {
                minx = minx.min(x);
                maxx = maxx.max(i64::from(x));
                miny = miny.min(y);
                maxy = maxy.max(i64::from(y));
            }
        }
    }
    if maxx < 0 {
        return BBox::full(w, h);
    }
    BBox {
        x0: minx,
        y0: miny,
        x1: maxx as u32 + 1,
        y1: maxy as u32 + 1,
    }
}

/// Trim a plate matching the top-left pixel. Returns `None` when the image is
/// nothing but that plate.
///
/// A pixel counts as background when alpha > 200 and the sum of absolute
/// channel differences to the reference is < 70. A row or column is trimmed
/// while background covers more than 98% of it.
#[must_use]
pub fn solid_bbox(rgba: &[u8], w: u32, h: u32) -> Option<BBox> {
    if w < 2 || h < 2 {
        return None;
    }
    let stride = (w as usize) * 4;
    let px = |x: u32, y: u32| -> &[u8] {
        let i = (y as usize) * stride + (x as usize) * 4;
        &rgba[i..i + 4]
    };
    let (r0, g0, b0, a0) = {
        let p = px(0, 0);
        (p[0], p[1], p[2], p[3])
    };
    // A transparent top-left means there is no flat plate; leave it to the
    // alpha bbox.
    if a0 < 200 {
        return None;
    }
    let is_bg = |p: &[u8]| -> bool {
        p[3] > 200
            && (i32::from(p[0]) - i32::from(r0)).abs()
                + (i32::from(p[1]) - i32::from(g0)).abs()
                + (i32::from(p[2]) - i32::from(b0)).abs()
                < 70
    };

    let mut top = 0u32;
    let mut bot = h - 1;
    let mut lef = 0u32;
    let mut rig = w - 1;

    // Integer compare, no floats: bad * 100 > total * 2 means more than 2%
    // non-background.
    let mostly_bg_row = |y: u32| -> bool {
        let bad = (0..w).filter(|&x| !is_bg(px(x, y))).count() as u64;
        bad * 100 <= u64::from(w) * 2
    };
    let mostly_bg_col = |x: u32, top: u32, bot: u32| -> bool {
        let bad = (top..=bot).filter(|&y| !is_bg(px(x, y))).count() as u64;
        bad * 100 <= u64::from(bot - top + 1) * 2
    };

    while top < bot && mostly_bg_row(top) {
        top += 1;
    }
    while bot > top && mostly_bg_row(bot) {
        bot -= 1;
    }
    while lef < rig && mostly_bg_col(lef, top, bot) {
        lef += 1;
    }
    while rig > lef && mostly_bg_col(rig, top, bot) {
        rig -= 1;
    }

    // A very thin result is likely a false positive (uniform image); keep the
    // full image instead.
    if bot - top > 8 && rig - lef > 8 {
        Some(BBox {
            x0: lef,
            y0: top,
            x1: rig + 1,
            y1: bot + 1,
        })
    } else {
        None
    }
}

/// Zero alpha inside `bbox` with a rounded mask.
///
/// Runs once when the texture is prepared. The radius is in source pixels, so
/// it scales proportionally and the dock icon keeps the same corner ratio at
/// every size during the grow animation.
pub fn apply_round_mask(rgba: &mut [u8], w: u32, bbox: BBox, radius: f32) {
    if radius <= 0.0 {
        return;
    }
    let (bw, bh) = (bbox.width() as f32, bbox.height() as f32);
    if bw <= 0.0 || bh <= 0.0 {
        return;
    }
    // A radius above half the short edge makes the corners self-intersect.
    let r = radius.min(bw.min(bh) / 2.0);
    let stride = (w as usize) * 4;

    for y in bbox.y0..bbox.y1 {
        let fy = (y - bbox.y0) as f32 + 0.5;
        for x in bbox.x0..bbox.x1 {
            let fx = (x - bbox.x0) as f32 + 0.5;
            // Only test inside the four corner squares.
            let dx = if fx < r {
                r - fx
            } else if fx > bw - r {
                fx - (bw - r)
            } else {
                0.0
            };
            let dy = if fy < r {
                r - fy
            } else if fy > bh - r {
                fy - (bh - r)
            } else {
                0.0
            };
            if dx > 0.0 && dy > 0.0 && dx * dx + dy * dy > r * r {
                let i = (y as usize) * stride + (x as usize) * 4;
                rgba[i + 3] = 0;
            }
        }
    }
}

/// One icon to composite (RGBA, row-major, 4 bytes per pixel).
#[derive(Copy, Clone, Debug)]
pub struct QuadSource<'a> {
    pub rgba: &'a [u8],
    pub width: u32,
    pub height: u32,
}

/// Composite 1..=4 icons into one square 2x2 group icon.
///
/// Same rule as `BuildGroupIcon`: 1 child fills the canvas, 2 split it
/// top/bottom, 3-4 use all four cells. Each icon is letterboxed inside its
/// cell with a small gap, and the canvas corners are rounded by `corner`
/// (radius in output pixels).
///
/// Inputs are straight alpha (same as [`fit_icon`]) and blending is
/// alpha-over. Sampling is bilinear: four small icons packed together show
/// nearest-neighbor stair-stepping badly at 48 px.
#[must_use]
pub fn composite_quad(sources: &[QuadSource<'_>], out: u32, corner: f32) -> Vec<u8> {
    let out = out.max(1);
    let mut canvas = vec![0u8; (out as usize) * (out as usize) * 4];
    let n = sources.len().min(4);
    if n == 0 {
        return canvas;
    }
    let (cols, rows) = match n {
        1 => (1u32, 1u32),
        2 => (1, 2),
        _ => (2, 2),
    };
    let cell_w = out / cols;
    let cell_h = out / rows;

    for (i, src) in sources.iter().take(n).enumerate() {
        if src.width == 0
            || src.height == 0
            || src.rgba.len() < (src.width as usize) * (src.height as usize) * 4
        {
            continue;
        }
        let col = i as u32 % cols;
        let row = i as u32 / cols;
        let cell_x = col * cell_w;
        let cell_y = row * cell_h;
        // Cell padding: the four icons must read as four, not one.
        let pad = (cell_w.min(cell_h) / 12).max(1);
        let box_w = cell_w.saturating_sub(pad * 2).max(1);
        let box_h = cell_h.saturating_sub(pad * 2).max(1);
        // Scale to fit the content box, keeping the aspect ratio.
        let scale = (box_w as f32 / src.width as f32).min(box_h as f32 / src.height as f32);
        let dw = ((src.width as f32 * scale).round() as u32).clamp(1, box_w);
        let dh = ((src.height as f32 * scale).round() as u32).clamp(1, box_h);
        let ox = cell_x + (cell_w - dw) / 2;
        let oy = cell_y + (cell_h - dh) / 2;

        for y in 0..dh {
            // Destination pixel center to source coords; the 0.5 puts source
            // pixels on sample points.
            let sy = (y as f32 + 0.5) * (src.height as f32 / dh as f32) - 0.5;
            for x in 0..dw {
                let sx = (x as f32 + 0.5) * (src.width as f32 / dw as f32) - 0.5;
                let px = sample_bilinear(src.rgba, src.width, src.height, sx, sy);
                over(&mut canvas, out, ox + x, oy + y, px);
            }
        }
    }

    if corner > 0.0 {
        apply_round_mask(&mut canvas, out, BBox::full(out, out), corner);
    }
    canvas
}

/// Bilinear-sample one source pixel. Out-of-range coords clamp to the edge.
fn sample_bilinear(src: &[u8], sw: u32, sh: u32, x: f32, y: f32) -> [u8; 4] {
    // Bail on an empty source: with `sw`/`sh` = 0 the upper bound is -1,
    // `clamp` panics, and `sw - 1` underflows to a huge u32.
    if sw == 0 || sh == 0 {
        return [0; 4];
    }
    let x = x.clamp(0.0, sw as f32 - 1.0);
    let y = y.clamp(0.0, sh as f32 - 1.0);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(sw - 1);
    let y1 = (y0 + 1).min(sh - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let px = |ix: u32, iy: u32| {
        let i = (iy as usize * sw as usize + ix as usize) * 4;
        [src[i], src[i + 1], src[i + 2], src[i + 3]]
    };
    let p00 = px(x0, y0);
    let p10 = px(x1, y0);
    let p01 = px(x0, y1);
    let p11 = px(x1, y1);
    let mut out = [0u8; 4];
    for c in 0..4 {
        let top = f32::from(p00[c]) * (1.0 - fx) + f32::from(p10[c]) * fx;
        let bottom = f32::from(p01[c]) * (1.0 - fx) + f32::from(p11[c]) * fx;
        out[c] = (top * (1.0 - fy) + bottom * fy).round() as u8;
    }
    out
}

/// Alpha-over a straight-alpha pixel onto the canvas.
fn over(canvas: &mut [u8], out: u32, x: u32, y: u32, px: [u8; 4]) {
    if x >= out || y >= out {
        return;
    }
    let a = f32::from(px[3]) / 255.0;
    if a <= 0.0 {
        return;
    }
    let i = (y as usize * out as usize + x as usize) * 4;
    for c in 0..3 {
        let dst = f32::from(canvas[i + c]) / 255.0;
        let src = f32::from(px[c]) / 255.0;
        canvas[i + c] = ((src * a + dst * (1.0 - a)) * 255.0).round() as u8;
    }
    let dst_a = f32::from(canvas[i + 3]) / 255.0;
    canvas[i + 3] = ((a + dst_a * (1.0 - a)) * 255.0).round() as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `w`x`h` image; `f(x,y)` returns RGBA.
    fn make(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                v.extend_from_slice(&f(x, y));
            }
        }
        v
    }

    #[test]
    fn alpha_bbox_finds_content() {
        // 32x32 fully transparent, a solid square at 10..22.
        let img = make(32, 32, |x, y| {
            if (10..22).contains(&x) && (10..22).contains(&y) {
                [255, 0, 0, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
        let b = alpha_bbox(&img, 32, 32, 40);
        assert_eq!(
            b,
            BBox {
                x0: 10,
                y0: 10,
                x1: 22,
                y1: 22
            }
        );
    }

    #[test]
    fn alpha_bbox_returns_full_when_fully_transparent() {
        // Fully transparent returns the full image; an empty box would make
        // later sizes zero.
        let img = make(8, 8, |_, _| [0, 0, 0, 0]);
        assert_eq!(alpha_bbox(&img, 8, 8, 40), BBox::full(8, 8));
    }

    #[test]
    fn solid_bbox_trims_flat_plate() {
        // Win11 Calculator: opaque (70,70,70) plate with a white glyph at
        // 20..40.
        let img = make(64, 64, |x, y| {
            if (20..40).contains(&x) && (20..40).contains(&y) {
                [250, 250, 250, 255]
            } else {
                [70, 70, 70, 255]
            }
        });
        let b = solid_bbox(&img, 64, 64).expect("应当裁掉纯色底板");
        assert_eq!(
            b,
            BBox {
                x0: 20,
                y0: 20,
                x1: 40,
                y1: 40
            }
        );
    }

    #[test]
    fn solid_bbox_gives_up_on_uniform_image() {
        // Uniform image = the plate is everything; nothing is left after
        // trimming, caught by the too-thin guard.
        let img = make(64, 64, |_, _| [70, 70, 70, 255]);
        assert_eq!(solid_bbox(&img, 64, 64), None);
    }

    #[test]
    fn solid_bbox_gives_up_when_corner_transparent() {
        // A transparent top-left means there is no flat plate; this path must
        // not run.
        let img = make(64, 64, |x, y| {
            if x < 4 && y < 4 {
                [0, 0, 0, 0]
            } else {
                [70, 70, 70, 255]
            }
        });
        assert_eq!(solid_bbox(&img, 64, 64), None);
    }

    /// Key difference between the modes: `Auto` trims, `Full` keeps the image,
    /// `Square` takes the center square.
    #[test]
    fn fit_modes_differ_as_expected() {
        // 64x64 opaque plate with a center glyph: Auto trims and rounds.
        let plate = make(64, 64, |x, y| {
            if (20..40).contains(&x) && (20..40).contains(&y) {
                [250, 250, 250, 255]
            } else {
                [70, 70, 70, 255]
            }
        });
        let auto = fit_icon(&plate, 64, 64, FitMode::Auto);
        assert_eq!(auto.bbox.width(), 20);
        assert!(auto.rounded.is_some(), "纯色底板应当加圆角");
        assert!((auto.fill - 0.94).abs() < f32::EPSILON);

        let full = fit_icon(&plate, 64, 64, FitMode::Full);
        assert_eq!(full.bbox, BBox::full(64, 64));
        assert!(full.rounded.is_none());

        // Cropping only shows on a non-square image: 64x32 takes center 32x32.
        let wide = make(64, 32, |_, _| [1, 2, 3, 255]);
        let sq = fit_icon(&wide, 64, 32, FitMode::Square);
        assert_eq!(sq.bbox.width(), 32);
        assert_eq!(sq.bbox.height(), 32);
        assert_eq!(sq.bbox.x0, 16);
        assert!((sq.fill - 1.0).abs() < f32::EPSILON);
        assert!(sq.rounded.is_some());
    }

    #[test]
    fn auto_keeps_transparent_icons_unrounded() {
        // Icon with transparent edges (like Notepad): crop alpha, no rounding.
        let img = make(32, 32, |x, y| {
            if (8..24).contains(&x) && (8..24).contains(&y) {
                [100, 150, 200, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
        let f = fit_icon(&img, 32, 32, FitMode::Auto);
        assert_eq!(f.bbox.width(), 16);
        assert!(f.rounded.is_none(), "透明边图标不该被圆角切掉");
    }

    #[test]
    fn round_mask_clears_corners_only() {
        // 32x32 fully opaque, radius 8: corners cleared, center untouched.
        let mut img = make(32, 32, |_, _| [255, 0, 0, 255]);
        let bbox = BBox::full(32, 32);
        apply_round_mask(&mut img, 32, bbox, 8.0);
        let a = |x: u32, y: u32| img[((y * 32 + x) * 4 + 3) as usize];
        assert_eq!(a(0, 0), 0, "左上角应被切掉");
        assert_eq!(a(31, 0), 0, "右上角应被切掉");
        assert_eq!(a(0, 31), 0, "左下角应被切掉");
        assert_eq!(a(31, 31), 0, "右下角应被切掉");
        assert_eq!(a(16, 16), 255, "中心不该动");
        assert_eq!(a(16, 0), 255, "上边中点不该动");
        assert_eq!(a(0, 16), 255, "左边中点不该动");
    }

    #[test]
    fn round_mask_clamps_radius_to_half_short_side() {
        // An absurd radius must not erase everything: clamp to half the short
        // edge.
        let mut img = make(16, 16, |_, _| [255, 0, 0, 255]);
        apply_round_mask(&mut img, 16, BBox::full(16, 16), 1000.0);
        let a = |x: u32, y: u32| img[((y * 16 + x) * 4 + 3) as usize];
        assert_eq!(a(8, 8), 255, "中心必须还在");
        assert_eq!(a(0, 0), 0);
    }

    /// Empty or undersized input must not panic; it falls back to the full
    /// image uncropped.
    #[test]
    fn degenerate_inputs_fall_back_to_full() {
        assert_eq!(fit_icon(&[], 0, 0, FitMode::Auto).bbox, BBox::full(0, 0));
        let short = vec![0u8; 16];
        let f = fit_icon(&short, 32, 32, FitMode::Auto);
        assert_eq!(f.bbox, BBox::full(32, 32));
    }

    /// A group icon from four solid red squares: every quadrant stays red.
    #[test]
    fn composite_quad_fills_all_cells() {
        let red = make(16, 16, |_, _| [255, 0, 0, 255]);
        let src = QuadSource {
            rgba: &red,
            width: 16,
            height: 16,
        };
        let out = composite_quad(&[src, src, src, src], 64, 0.0);
        assert_eq!(out.len(), 64 * 64 * 4);
        let at = |x: u32, y: u32| {
            let i = (y as usize * 64 + x as usize) * 4;
            [out[i], out[i + 1], out[i + 2], out[i + 3]]
        };
        // The center of each quadrant must be red.
        for (x, y) in [(16, 16), (48, 16), (16, 48), (48, 48)] {
            let p = at(x, y);
            assert!(
                p[0] > 200 && p[1] < 60 && p[3] > 200,
                "({x},{y}) 应为红色: {p:?}"
            );
        }
        // The cross between quadrants is transparent, so four icons read as
        // four.
        let gap = at(32, 32);
        assert_eq!(gap[3], 0, "象限缝隙应当透明: {gap:?}");
    }

    /// A single child fills the whole cell.
    #[test]
    fn composite_quad_with_one_source_fills_the_canvas() {
        let blue = make(8, 8, |_, _| [0, 0, 255, 255]);
        let src = QuadSource {
            rgba: &blue,
            width: 8,
            height: 8,
        };
        let out = composite_quad(&[src], 32, 0.0);
        let i = (16 * 32 + 16) * 4;
        assert_eq!(&out[i..i + 4], &[0, 0, 255, 255]);
    }

    /// Two children stack top and bottom.
    #[test]
    fn composite_quad_with_two_sources_stacks_them() {
        let top = make(8, 8, |_, _| [255, 0, 0, 255]);
        let bottom = make(8, 8, |_, _| [0, 255, 0, 255]);
        let out = composite_quad(
            &[
                QuadSource {
                    rgba: &top,
                    width: 8,
                    height: 8,
                },
                QuadSource {
                    rgba: &bottom,
                    width: 8,
                    height: 8,
                },
            ],
            32,
            0.0,
        );
        let at = |x: u32, y: u32| {
            let i = (y as usize * 32 + x as usize) * 4;
            [out[i], out[i + 1], out[i + 2], out[i + 3]]
        };
        assert!(at(16, 8)[0] > 200, "上半应是红色");
        assert!(at(16, 24)[1] > 200, "下半应是绿色");
    }

    /// The corner mask must actually cut the corners.
    #[test]
    fn composite_quad_applies_the_corner_mask() {
        let red = make(16, 16, |_, _| [255, 0, 0, 255]);
        let src = QuadSource {
            rgba: &red,
            width: 16,
            height: 16,
        };
        let out = composite_quad(&[src, src, src, src], 64, 10.0);
        let i = 0; // top-left corner
        assert_eq!(out[i + 3], 0, "左上角应被圆角切掉");
        // Sample inside a cell: canvas center (32,32) sits on the transparent
        // cross between cells.
        let i = (16 * 64 + 16) * 4;
        assert!(out[i + 3] > 200, "格内不该被切");
    }

    /// Empty input must not panic; it yields a fully transparent canvas.
    #[test]
    fn composite_quad_with_no_sources_is_transparent() {
        let out = composite_quad(&[], 16, 8.0);
        assert_eq!(out.len(), 16 * 16 * 4);
        assert!(out.chunks(4).all(|p| p[3] == 0));
    }
}
