//! ARGB colors and color space conversion.
//!
//! Matches GDI+ exactly: `0xAARRGGBB`, not premultiplied alpha. The DIB
//! section uses `PixelFormat32bppPARGB`; GDI+ premultiplies on write, so
//! callers always pass straight alpha. Same contract as the `ARGBC` macro.

/// Packed `0xAARRGGBB`.
pub type Argb = u32;

/// Build an opaque color.
#[must_use]
pub const fn rgb(r: u8, g: u8, b: u8) -> Argb {
    argb(255, r, g, b)
}

/// Build a color with alpha. Mirrors `ARGBC(a, r, g, b)`.
#[must_use]
pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> Argb {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

#[must_use]
pub const fn alpha(c: Argb) -> u8 {
    (c >> 24) as u8
}
#[must_use]
pub const fn red(c: Argb) -> u8 {
    (c >> 16) as u8
}
#[must_use]
pub const fn green(c: Argb) -> u8 {
    (c >> 8) as u8
}
#[must_use]
pub const fn blue(c: Argb) -> u8 {
    c as u8
}

/// Replace alpha only, keeping RGB.
#[must_use]
pub const fn with_alpha(c: Argb, a: u8) -> Argb {
    (c & 0x00FF_FFFF) | ((a as u32) << 24)
}

/// Scale alpha by `scale` (used for whole-element fading).
///
/// `scale` is clamped to `0.0..=1.0`. `ARGBC` instead stuffs `op * 255` into an
/// `int`, so `Opacity=1000` overflows into the color channels.
#[must_use]
pub fn scale_alpha(c: Argb, scale: f32) -> Argb {
    let s = scale.clamp(0.0, 1.0);
    let a = (alpha(c) as f32 * s).round().clamp(0.0, 255.0) as u8;
    with_alpha(c, a)
}

/// Linear interpolation, used for gradient endpoints and accent blends.
#[must_use]
pub fn mix(a: Argb, b: Argb, t: f32) -> Argb {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| -> u8 { (x as f32 + (y as f32 - x as f32) * t).round() as u8 };
    argb(
        f(alpha(a), alpha(b)),
        f(red(a), red(b)),
        f(green(a), green(b)),
        f(blue(a), blue(b)),
    )
}

/// HSV to RGB. `h` is in degrees (any real, wrapped internally), `s`/`v` in
/// `0.0..=1.0`.
///
/// Mirrors `HsvRgb`: with `AccentTone` on, the system accent hue generates two
/// Material You tones whose saturation and value come from the theme.
#[must_use]
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0) / 60.0;
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let i = h.floor() as i32;
    let f = h - h.floor();
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    let to = |x: f32| (x * 255.0).round().clamp(0.0, 255.0) as u8;
    (to(r), to(g), to(b))
}

/// RGB to hue in degrees, `0.0..360.0`.
///
/// Grays (`d <= 0.001`) return 0.0.
#[must_use]
pub fn hue_of(c: Argb) -> f32 {
    let r = red(c) as f32 / 255.0;
    let g = green(c) as f32 / 255.0;
    let b = blue(c) as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    if d <= 0.001 {
        return 0.0;
    }
    let mut h = if (max - r).abs() < f32::EPSILON {
        ((g - b) / d).rem_euclid(6.0)
    } else if (max - g).abs() < f32::EPSILON {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h *= 60.0;
    if h < 0.0 {
        h += 360.0;
    }
    h
}

/// Win32 `COLORREF` (`0x00BBGGRR`) to ARGB. Registry and system APIs use this
/// byte order.
#[must_use]
pub const fn from_colorref(c: u32) -> Argb {
    argb(
        255,
        (c & 0xFF) as u8,
        ((c >> 8) & 0xFF) as u8,
        ((c >> 16) & 0xFF) as u8,
    )
}

/// ARGB to `COLORREF`. `BgColor1/2` are stored as decimal `COLORREF` in the ini.
#[must_use]
pub const fn to_colorref(c: Argb) -> u32 {
    ((blue(c) as u32) << 16) | ((green(c) as u32) << 8) | (red(c) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argb_layout_matches_gdiplus() {
        // 0xAARRGGBB: alpha in the high byte, as in GDI+ and Windows.
        assert_eq!(argb(0x12, 0x34, 0x56, 0x78), 0x1234_5678);
        assert_eq!(alpha(0x1234_5678), 0x12);
        assert_eq!(red(0x1234_5678), 0x34);
        assert_eq!(green(0x1234_5678), 0x56);
        assert_eq!(blue(0x1234_5678), 0x78);
    }

    #[test]
    fn scale_alpha_clamps_instead_of_overflowing() {
        // Alpha overflow must never bleed into the color channels.
        let c = argb(200, 10, 20, 30);
        assert_eq!(scale_alpha(c, 1.5), c);
        assert_eq!(red(scale_alpha(c, 1.5)), 10);
        assert_eq!(green(scale_alpha(c, 1.5)), 20);
        assert_eq!(blue(scale_alpha(c, 1.5)), 30);
        assert_eq!(alpha(scale_alpha(c, -1.0)), 0);
        assert_eq!(alpha(scale_alpha(c, 0.5)), 100);
    }

    #[test]
    fn colorref_roundtrip() {
        let c = argb(255, 0x11, 0x22, 0x33);
        assert_eq!(to_colorref(c), 0x0033_2211);
        assert_eq!(from_colorref(to_colorref(c)), c);
    }

    #[test]
    fn hsv_primaries() {
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), (255, 0, 0));
        assert_eq!(hsv_to_rgb(120.0, 1.0, 1.0), (0, 255, 0));
        assert_eq!(hsv_to_rgb(240.0, 1.0, 1.0), (0, 0, 255));
        assert_eq!(hsv_to_rgb(360.0, 1.0, 1.0), (255, 0, 0));
        assert_eq!(hsv_to_rgb(0.0, 0.0, 0.5), (128, 128, 128));
    }

    #[test]
    fn hue_roundtrip_for_saturated_colors() {
        for hue in [0.0_f32, 30.0, 90.0, 180.0, 270.0, 350.0] {
            let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
            let back = hue_of(rgb(r, g, b));
            // 8-bit quantization in HSV->RGB costs up to ~1 degree.
            assert!((back - hue).abs() < 1.5, "hue {hue} -> {back}");
        }
    }
}
