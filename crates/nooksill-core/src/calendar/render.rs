//! Cell sub-labels (festival, solar term, lunar day) and the sexagenary cycle.

use super::lunar::month_days;
use super::*;

/// Solar festivals as (month, day, name).
const SOLAR_FESTIVALS: [(u32, u32, &str); 6] = [
    (1, 1, "元旦"),
    (2, 14, "情人节"),
    (5, 1, "劳动节"),
    (6, 1, "儿童节"),
    (10, 1, "国庆节"),
    (12, 25, "圣诞节"),
];

/// The sub-label under a calendar cell and whether it is highlighted.
///
/// Priority: solar festival > lunar festival > solar term > month name on
/// the first day > lunar day name. Only festivals and solar terms are
/// highlighted.
#[must_use]
pub fn sub_label(year: i32, month: u32, day: u32) -> (String, bool) {
    if let Some((_, _, name)) = SOLAR_FESTIVALS
        .iter()
        .find(|(m, d, _)| *m == month && *d == day)
    {
        return ((*name).to_owned(), true);
    }

    let lunar = solar_to_lunar(year, month, day);
    if let Some(l) = lunar {
        if !l.leap {
            let festival = match (l.month, l.day) {
                (1, 1) => Some("春节"),
                (1, 15) => Some("元宵"),
                (5, 5) => Some("端午"),
                (7, 7) => Some("七夕"),
                (8, 15) => Some("中秋"),
                (9, 9) => Some("重阳"),
                (12, d) if d == month_days(l.year, 12) => Some("除夕"),
                _ => None,
            };
            if let Some(name) = festival {
                return (name.to_owned(), true);
            }
        }
    }

    // Each civil month has two solar terms: (m-1)*2 and (m-1)*2+1.
    for n in [(month as usize - 1) * 2, (month as usize - 1) * 2 + 1] {
        let (tm, td) = term_date(year, n);
        if tm == month && td == day {
            return (term_name(n).to_owned(), true);
        }
    }

    if let Some(l) = lunar {
        if l.day == 1 {
            return (
                format!(
                    "{}{}月",
                    if l.leap { "闰" } else { "" },
                    lunar_month_name(l.month)
                ),
                false,
            );
        }
        return (lunar_day_name(l.day).to_owned(), false);
    }

    (String::new(), false)
}

/// Sexagenary year, month and day.
///
/// The year changes at the solar term that begins spring and the month at
/// each month's first solar term, not at the first day of the lunar month.
///
/// These are the year, month and day pillars of the Four Pillars; the hour
/// pillar needs a birth time and is not shown.
#[must_use]
pub fn ganzhi_text(year: i32, month: u32, day: u32) -> String {
    // The sexagenary year is still the previous one before that term.
    let (lm, ld) = term_date(year, 2); // index 2 = the spring term
    let gy = if month > lm || (month == lm && day >= ld) {
        year
    } else {
        year - 1
    };
    let ys = ((gy - 4) % 10 + 10) % 10;
    let yz = ((gy - 4) % 12 + 12) % 12;

    // The sexagenary month starts at the month's first solar term.
    let (_tm, td) = term_date(year, (month as usize - 1) * 2);
    let eff_month = if day >= td {
        month as i32
    } else {
        month as i32 - 1
    };
    let branch = (eff_month % 12 + 12) % 12;
    // Five Tigers: the year's stem decides the first month's stem. The first
    // month is the Yin branch, so the stem counts `offset` steps from it.
    let offset = (branch - 2 + 12) % 12;
    let ms = ((ys % 5) * 2 + 2 + offset) % 10;

    // The day pillar cycles every 60 days; 1970-01-01 is the Xin-Si day.
    let di = ((days_from_civil(year, month, day) + 17) % 60 + 60) % 60;

    format!(
        "{}{}年  {}{}月  {}{}日",
        GAN[ys as usize],
        ZHI[yz as usize],
        GAN[ms as usize],
        ZHI[branch as usize],
        GAN[(di % 10) as usize],
        ZHI[(di % 12) as usize]
    )
}
