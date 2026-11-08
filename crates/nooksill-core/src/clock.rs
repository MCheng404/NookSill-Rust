//! Clock card text.
//!
//! Only produces strings; layout and drawing live in the UI layer.
//!
//! Two date layouts, easy to confuse:
//! - Horizontal: full date with the year, weekday appended to the same line.
//! - Vertical: month/day only, weekday on its own line.
//!
//! `DateFmt` selects 0 = localized long form, 1 = slash, 2 = dash. Vertical
//! lines are ~21 px wide, so the year is dropped there.

use crate::config::{Config, Key};

/// One time snapshot. Fields mirror `SYSTEMTIME` without the Win32 dependency.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ClockParts {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    /// 0 = Sunday. Matches `SYSTEMTIME::wDayOfWeek`.
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// One line on the card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClockLine {
    pub text: String,
    /// Date group flag. Lines in the same group get no divider between them.
    pub is_date: bool,
}

const WEEKDAY: [&str; 7] = ["日", "一", "二", "三", "四", "五", "六"];

fn weekday_name(w: u8) -> &'static str {
    WEEKDAY.get(w as usize).copied().unwrap_or("日")
}

/// Build the card lines in draw order.
#[must_use]
pub fn clock_lines(cfg: &Config, t: &ClockParts) -> Vec<ClockLine> {
    // CLOCK_SHOW: 0 = both, 1 = time only, 2 = date only.
    let has_time = cfg.get_i32(Key::CLOCK_SHOW) != 2;
    let has_date = cfg.get_i32(Key::CLOCK_SHOW) != 1;
    let vertical = !cfg.edge.is_horizontal();

    let time = if cfg.get_i32(Key::CLOCK_SEC) != 0 {
        format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second)
    } else {
        format!("{:02}:{:02}", t.hour, t.minute)
    };

    let mut dates: Vec<String> = Vec::new();
    if has_date {
        let fmt = cfg.get_i32(Key::DATE_FMT);
        let week = cfg.get_i32(Key::CLOCK_WEEK) != 0;
        if vertical {
            dates.push(match fmt {
                1 => format!("{:02}/{:02}", t.month, t.day),
                2 => format!("{:02}-{:02}", t.month, t.day),
                _ => format!("{}月{}日", t.month, t.day),
            });
            if week {
                dates.push(format!("周{}", weekday_name(t.weekday)));
            }
        } else {
            let mut d = match fmt {
                1 => format!("{:04}/{:02}/{:02}", t.year, t.month, t.day),
                2 => format!("{:04}-{:02}-{:02}", t.year, t.month, t.day),
                _ => format!("{}年{}月{}日", t.year, t.month, t.day),
            };
            if week {
                d.push_str(" 周");
                d.push_str(weekday_name(t.weekday));
            }
            dates.push(d);
        }
    }

    let mut out = Vec::with_capacity(dates.len() + 1);
    // CLOCK_ORDER=1 puts the date on top; also applies when vertical.
    if has_time && has_date && cfg.get_i32(Key::CLOCK_ORDER) != 0 {
        out.extend(dates.into_iter().map(|text| ClockLine {
            text,
            is_date: true,
        }));
        out.push(ClockLine {
            text: time,
            is_date: false,
        });
    } else {
        if has_time {
            out.push(ClockLine {
                text: time,
                is_date: false,
            });
        }
        out.extend(dates.into_iter().map(|text| ClockLine {
            text,
            is_date: true,
        }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Edge};

    fn at(year: u16, month: u8, day: u8, weekday: u8) -> ClockParts {
        ClockParts {
            year,
            month,
            day,
            weekday,
            hour: 9,
            minute: 5,
            second: 7,
        }
    }

    fn cfg() -> Config {
        let mut c = Config::defaults();
        c.clock = true;
        c.edge = Edge::Bottom;
        c
    }

    /// Horizontal default: one time line, one date line with the year.
    #[test]
    fn horizontal_default_includes_year() {
        let lines = clock_lines(&cfg(), &at(2026, 10, 30, 5));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "09:05");
        assert!(!lines[0].is_date);
        assert_eq!(lines[1].text, "2026年10月30日 周五");
        assert!(lines[1].is_date);
    }

    /// Vertical: short date, weekday on its own line (no room for the year).
    #[test]
    fn vertical_puts_weekday_on_its_own_line() {
        let mut c = cfg();
        c.edge = Edge::Left;
        let lines = clock_lines(&c, &at(2026, 10, 30, 5));
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].text, "09:05");
        assert_eq!(lines[1].text, "10月30日");
        assert_eq!(lines[2].text, "周五");
        // Date and weekday share a group; no divider between them.
        assert!(lines[1].is_date && lines[2].is_date);
    }

    #[test]
    fn date_formats_follow_config() {
        let mut c = cfg();
        // Weekday off, otherwise the date line also carries the weekday and
        // the comparison no longer tests the format itself.
        c.apply_i32(Key::CLOCK_WEEK, 0);
        c.apply_i32(Key::DATE_FMT, 1);
        assert_eq!(clock_lines(&c, &at(2026, 10, 5, 1))[1].text, "2026/10/05");
        c.apply_i32(Key::DATE_FMT, 2);
        assert_eq!(clock_lines(&c, &at(2026, 10, 5, 1))[1].text, "2026-10-05");
    }

    #[test]
    fn seconds_are_optional() {
        let mut c = cfg();
        assert_eq!(clock_lines(&c, &at(2026, 1, 1, 0))[0].text, "09:05");
        c.apply_i32(Key::CLOCK_SEC, 1);
        assert_eq!(clock_lines(&c, &at(2026, 1, 1, 0))[0].text, "09:05:07");
    }

    /// Time-only and date-only modes yield a single non-empty line.
    #[test]
    fn single_part_modes_produce_one_line() {
        let mut c = cfg();
        c.apply_i32(Key::CLOCK_SHOW, 1); // time only
        let l = clock_lines(&c, &at(2026, 10, 30, 5));
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].text, "09:05");

        c.apply_i32(Key::CLOCK_SHOW, 2); // date only
        let l = clock_lines(&c, &at(2026, 10, 30, 5));
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].text, "2026年10月30日 周五");
    }

    /// Weekday can be turned off; the date line then omits it.
    #[test]
    fn weekday_can_be_disabled() {
        let mut c = cfg();
        c.apply_i32(Key::CLOCK_WEEK, 0);
        assert_eq!(
            clock_lines(&c, &at(2026, 10, 30, 5))[1].text,
            "2026年10月30日"
        );
    }

    /// `ClockOrder=1` puts the date on top.
    #[test]
    fn order_swaps_date_and_time() {
        let mut c = cfg();
        c.apply_i32(Key::CLOCK_ORDER, 1);
        let l = clock_lines(&c, &at(2026, 10, 30, 5));
        assert!(l[0].is_date, "第一行应当是日期");
        assert_eq!(l[1].text, "09:05");
    }

    /// Out-of-range weekday must not panic; the system never sends one.
    #[test]
    fn out_of_range_weekday_is_safe() {
        let l = clock_lines(&cfg(), &at(2026, 10, 30, 200));
        assert!(l[1].text.ends_with("周日"));
    }
}
