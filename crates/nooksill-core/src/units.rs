//! DPI and pixel conversion.
//!
//! `Sc(v)` is `MulDiv(v, g_dpi, 96)` throughout. One function keeps the exact
//! rounding and avoids missed scaling that only shows up at 150%.

/// Compute `v * dpi / 96`, rounded.
///
/// Matches Win32 `MulDiv`: a 64-bit intermediate prevents overflow, and the
/// result rounds away from zero.
#[must_use]
pub fn scale(v: i32, dpi: i32) -> i32 {
    if dpi == 96 {
        return v;
    }
    let num = i64::from(v) * i64::from(dpi);
    // Integer math stays predictable across platforms at extreme values.
    let half = 96 / 2;
    let r = if num >= 0 {
        (num + half) / 96
    } else {
        (num - half) / 96
    };
    r.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// Convert physical pixels to 96 DPI logical pixels, rounded.
///
/// Used to migrate legacy ini values (`CONFIG_VERSION` 3 -> 4) and to write
/// the drag-derived splitter position back to config.
#[must_use]
pub fn unscale(v: i32, dpi: i32) -> i32 {
    if dpi == 96 || dpi <= 0 {
        return v;
    }
    let num = i64::from(v) * 96;
    let half = i64::from(dpi) / 2;
    let r = if num >= 0 {
        (num + half) / i64::from(dpi)
    } else {
        (num - half) / i64::from(dpi)
    };
    r.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_at_96() {
        assert_eq!(scale(48, 96), 48);
    }

    #[test]
    fn rounds_like_muldiv() {
        assert_eq!(scale(48, 120), 60);
        assert_eq!(scale(48, 144), 72);
        assert_eq!(scale(48, 192), 96);
        // 25 * 120 / 96 = 31.25 -> 31
        assert_eq!(scale(25, 120), 31);
        // 1 * 120 / 96 = 1.25 -> 1; 1 * 144 / 96 = 1.5 -> 2
        assert_eq!(scale(1, 120), 1);
        assert_eq!(scale(1, 144), 2);
    }

    #[test]
    fn negative_rounds_away_from_zero() {
        assert_eq!(scale(-1, 144), -2);
        assert_eq!(scale(-48, 120), -60);
    }

    #[test]
    fn no_overflow_on_extremes() {
        // MulDiv overflows a 32-bit intermediate; this must saturate, not wrap.
        assert_eq!(scale(i32::MAX, 480), i32::MAX);
        assert_eq!(scale(i32::MIN, 480), i32::MIN);
    }

    /// `unscale` inverts `scale` on the common DPI tiers.
    #[test]
    fn unscale_round_trips_the_common_tiers() {
        for dpi in [96, 120, 144, 168, 192, 240] {
            for v in [0, 1, 2, 4, 8, 12, 18, 24, 48, 72, 96, 128] {
                let back = unscale(scale(v, dpi), dpi);
                assert!((back - v).abs() <= 1, "dpi={dpi} v={v} 往返变成 {back}");
            }
        }
        // 100 * 96 / 144 = 66.67, rounds to 67 (truncation gives 66).
        assert_eq!(unscale(100, 144), 67);
        assert_eq!(unscale(72, 144), 48);
    }
}
