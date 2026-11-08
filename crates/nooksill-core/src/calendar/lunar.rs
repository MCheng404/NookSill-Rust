//! Civil date arithmetic, lunar conversion and solar term dates.

use super::*;

/// Convert a civil date to days since 1970-01-01.
///
/// Uses the Hinnant algorithm: fold by 400-year eras, then by a 153-day
/// five-month cycle inside the era. There is no leap-year branch, and the
/// result is monotonic across 1901..=2099.
#[must_use]
pub fn days_from_civil(year: i32, month: u32, day: u32) -> i32 {
    debug_assert!((1..=12).contains(&month), "月份越界: {month}");
    debug_assert!((1..=31).contains(&day), "日越界: {day}");
    let y = year - i32::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32; // [0, 399]
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe as i32 - 719_468
}

/// Convert days since 1970-01-01 to a civil date, the inverse of
/// [`days_from_civil`].
#[must_use]
pub fn civil_from_days(days: i32) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i32 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (y + i32::from(m <= 2), m, d)
}

/// Day of the week, 0 = Sunday.
#[must_use]
pub fn weekday_of(year: i32, month: u32, day: u32) -> u32 {
    (((days_from_civil(year, month, day) % 7) + 11) % 7) as u32
}

/// Weekday name, shared by the calendar header and the info panel.
#[must_use]
pub fn weekday_name(w: u32) -> &'static str {
    WEEKDAY[(w % 7) as usize]
}

/// A lunar date.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Lunar {
    /// Lunar year, which differs from the civil year before Spring Festival.
    pub year: i32,
    /// Month, 1..=12.
    pub month: u32,
    /// Day, 1..=30.
    pub day: u32,
    /// Whether this is a leap month.
    pub leap: bool,
}

/// Years covered by the lunar table (index `year - 1900`, 201 entries).
const LUNAR_MIN: i32 = 1900;
const LUNAR_MAX: i32 = 2100;

/// Civil years the conversion supports: 1901..=2099.
///
/// The early 1900 lunar data is incomplete and the table stops at 2100.
pub const MIN_YEAR: i32 = 1901;
pub const MAX_YEAR: i32 = 2099;

/// Leap month of a lunar year; 0 means there is none.
fn leap_month(year: i32) -> u32 {
    LUNAR_INFO[(year - LUNAR_MIN) as usize] & 0xf
}

/// Length in days of the leap month; 0 when there is no leap month.
fn leap_days(year: i32) -> u32 {
    if leap_month(year) == 0 {
        0
    } else if LUNAR_INFO[(year - LUNAR_MIN) as usize] & 0x1_0000 != 0 {
        30
    } else {
        29
    }
}

/// Length in days of the `month`-th regular month (1..=12) of a lunar year.
pub(super) fn month_days(year: i32, month: u32) -> u32 {
    if LUNAR_INFO[(year - LUNAR_MIN) as usize] & (0x1_0000 >> month) != 0 {
        30
    } else {
        29
    }
}

/// Total length in days of a lunar year (354..=385).
fn year_days(year: i32) -> i32 {
    // 348 = 12 x 29: assume every month is short, then add a day for each
    // long month. The bit is `0x8000 >> (m - 1)` with m = 1..=12, so the
    // shift runs from 0x8000 down to 0x10.
    let mut sum = 348;
    let info = LUNAR_INFO[(year - LUNAR_MIN) as usize];
    for shift in 0..12 {
        if info & (0x8000 >> shift) != 0 {
            sum += 1;
        }
    }
    sum + leap_days(year) as i32
}

/// Convert a civil date to lunar. Out of 1901..=2099 yields `None`.
#[must_use]
pub fn solar_to_lunar(year: i32, month: u32, day: u32) -> Option<Lunar> {
    if !(MIN_YEAR..=MAX_YEAR).contains(&year) {
        return None;
    }
    // 1900-01-31 is the first day of the lunar month 1 of 1900, the epoch.
    let mut offset = days_from_civil(year, month, day) - days_from_civil(1900, 1, 31);

    // Locate the lunar year first.
    let mut ly = LUNAR_MIN;
    loop {
        let d = year_days(ly);
        if offset < d {
            break;
        }
        offset -= d;
        ly += 1;
        // The table's upper bound: reaching 2100 means the input was out of
        // range. The check above already rejects it; this only keeps a
        // release build from reading past the table.
        if ly > LUNAR_MAX {
            return None;
        }
    }

    // Then locate the month, leap months included.
    let leap = leap_month(ly);
    let mut m = 1_u32;
    let mut is_leap = false;
    loop {
        let d = if is_leap {
            leap_days(ly)
        } else {
            month_days(ly, m)
        };
        if offset < d as i32 {
            break;
        }
        offset -= d as i32;
        // The month order is "..., month 4, leap month 4, month 5, ...": after
        // the regular month that equals the leap month, the next step stays
        // on the leap month of the same number.
        if !is_leap && leap > 0 && m == leap {
            is_leap = true;
        } else {
            is_leap = false;
            m += 1;
            if m > 12 {
                return None;
            }
        }
    }

    Some(Lunar {
        year: ly,
        month: m,
        day: offset as u32 + 1,
        leap: is_leap,
    })
}

/// Convert a lunar date to civil, the inverse of [`solar_to_lunar`].
#[must_use]
pub fn lunar_to_solar(l: Lunar) -> Option<(i32, u32, u32)> {
    if l.year < LUNAR_MIN || l.year > MAX_YEAR || !(1..=12).contains(&l.month) {
        return None;
    }
    if l.day < 1 || l.day > 30 {
        return None;
    }
    // Check that the day really exists: a leap month must be a leap month,
    // and the day must not exceed the month length.
    let leap = leap_month(l.year);
    if l.leap && leap != l.month {
        return None;
    }
    let len = if l.leap {
        leap_days(l.year)
    } else {
        month_days(l.year, l.month)
    };
    if l.day > len {
        return None;
    }

    let mut days = days_from_civil(1900, 1, 31);
    for y in LUNAR_MIN..l.year {
        days += year_days(y);
    }
    for m in 1..l.month {
        days += month_days(l.year, m) as i32;
        if leap > 0 && m == leap {
            days += leap_days(l.year) as i32;
        }
    }
    if l.leap {
        days += month_days(l.year, l.month) as i32;
    }
    days += l.day as i32 - 1;
    let (y, m, d) = civil_from_days(days);
    Some((y, m, d))
}

/// Lunar month name.
#[must_use]
pub fn lunar_month_name(month: u32) -> &'static str {
    LUNAR_MONTH[(month.clamp(1, 12) - 1) as usize]
}

/// Lunar day name.
#[must_use]
pub fn lunar_day_name(day: u32) -> &'static str {
    LUNAR_DAY[(day.clamp(1, 30) - 1) as usize]
}

/// Full lunar description, e.g. the sexagenary year, the zodiac animal, the
/// month and the day.
#[must_use]
pub fn lunar_text(l: Lunar) -> String {
    let yi = ((l.year - 4) % 10 + 10) % 10;
    let zi = ((l.year - 4) % 12 + 12) % 12;
    format!(
        "{}{}年（{}） {}{}月{}",
        GAN[yi as usize],
        ZHI[zi as usize],
        ZODIAC[zi as usize],
        if l.leap { "闰" } else { "" },
        lunar_month_name(l.month),
        lunar_day_name(l.day)
    )
}

/// Date of the `n`-th solar term in `year`.
///
/// Uses the mean-solar-term formula (the tropical year as a constant
/// 365.2422 days), not the modern apparent one, so it differs from the real
/// term by up to a day. That is enough to match the published calendar;
/// minute-level accuracy would need VSOP87.
#[must_use]
pub fn term_date(year: i32, n: usize) -> (u32, u32) {
    let n = n % 24;
    let ms = 31_556_925_974.7_f64 * f64::from(year - 1900) + TERM_INFO[n] as f64 * 60_000.0;
    let days =
        f64::from(days_from_civil(1900, 1, 6)) + (2.0 * 60.0 + 5.0) / 1440.0 + ms / 86_400_000.0;
    let (_, m, d) = civil_from_days(days.floor() as i32);
    (m, d)
}

/// Solar term name.
#[must_use]
pub fn term_name(n: usize) -> &'static str {
    TERM_NAME[n % 24]
}

/// Nearest solar term after `(y, m, d)`, skipping that day itself.
#[must_use]
pub fn next_term(year: i32, month: u32, day: u32) -> Option<(i32, usize, i32)> {
    let today = days_from_civil(year, month, day);
    for y in year..=year + 1 {
        for n in 0..24 {
            let (tm, td) = term_date(y, n);
            let t = days_from_civil(y, tm, td);
            if t > today {
                return Some((y, n, t - today));
            }
        }
    }
    None
}
