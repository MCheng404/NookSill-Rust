//! Lunar calendar, solar terms and the sexagenary cycle.
//!
//! Pure computation, no Win32 calls, so the whole 1901..=2099 range is tested
//! by exhaustive civil -> lunar -> civil round trips. The lookup tables live in
//! [`calendar_tables`] and are generated, never hand-edited.

mod calendar_tables;

use calendar_tables::{
    GAN, LUNAR_DAY, LUNAR_INFO, LUNAR_MONTH, TERM_INFO, TERM_NAME, WEEKDAY, ZHI, ZODIAC,
};

mod lunar;
mod render;

pub use lunar::{
    civil_from_days, days_from_civil, lunar_day_name, lunar_month_name, lunar_text, lunar_to_solar,
    next_term, solar_to_lunar, term_date, term_name, weekday_name, weekday_of, Lunar, MAX_YEAR,
    MIN_YEAR,
};
pub use render::{ganzhi_text, sub_label};

/// One grid cell, precomputed so the UI only draws.
#[derive(Clone, Debug, PartialEq)]
pub struct DayCell {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    /// False for trailing days borrowed from the adjacent months.
    pub in_month: bool,
    /// Festival, solar term or lunar day.
    pub sub: String,
    /// Festivals and solar terms are drawn highlighted.
    pub highlight: bool,
}
/// Selection and visible month, independent of the UI.
#[derive(Clone, Debug)]
pub struct CalendarState {
    pub year: i32,
    pub month: u32,
    pub selected: (i32, u32, u32),
    cells: [DayCell; 42],
}

impl CalendarState {
    /// Start on the given day: it is selected and its month is shown.
    #[must_use]
    pub fn new(today: (i32, u32, u32)) -> Self {
        let mut s = Self {
            year: today.0.clamp(MIN_YEAR, MAX_YEAR - 1),
            month: today.1,
            selected: today,
            cells: std::array::from_fn(|_| DayCell {
                year: today.0,
                month: today.1,
                day: 1,
                in_month: true,
                sub: String::new(),
                highlight: false,
            }),
        };
        s.refresh();
        s
    }

    /// Step by `dy` years and `dm` months; months carry into years, and the
    /// year is clamped to 1901..=2098.
    pub fn step(&mut self, dy: i32, dm: i32) {
        let mut y = self.year + dy;
        let mut m = self.month as i32 + dm;
        while m > 12 {
            m -= 12;
            y += 1;
        }
        while m < 1 {
            m += 12;
            y -= 1;
        }
        let y = y.clamp(MIN_YEAR, MAX_YEAR - 1);
        if y != self.year {
            // Clamping the year must also clamp the month, or 2098-13 appears.
            m = m.clamp(1, 12);
        }
        self.year = y;
        self.month = m as u32;
        self.refresh();
    }

    /// Select a day, switching the visible month if needed.
    pub fn select(&mut self, year: i32, month: u32, day: u32) {
        self.selected = (year, month, day);
        if year != self.year || month != self.month {
            self.year = year.clamp(MIN_YEAR, MAX_YEAR - 1);
            self.month = month;
            self.refresh();
        }
    }

    /// Reset to today.
    pub fn go_today(&mut self, today: (i32, u32, u32)) {
        self.year = today.0.clamp(MIN_YEAR, MAX_YEAR - 1);
        self.month = today.1;
        self.selected = today;
        self.refresh();
    }

    /// 42 cells: 6 rows of 7, weeks start on Monday.
    #[must_use]
    pub fn cells(&self) -> &[DayCell; 42] {
        &self.cells
    }

    /// Recompute the grid after a month or selection change.
    fn refresh(&mut self) {
        // Weeks start on Monday: turn "Sunday = 0" into "Monday = 0".
        let first = weekday_of(self.year, self.month, 1);
        let offset = ((first + 6) % 7) as i32;
        let base = days_from_civil(self.year, self.month, 1) - offset;
        for k in 0..42 {
            let (y, m, d) = civil_from_days(base + k as i32);
            let (sub, highlight) = sub_label(y, m, d);
            self.cells[k] = DayCell {
                year: y,
                month: m,
                day: d,
                in_month: m == self.month,
                sub,
                highlight,
            };
        }
    }
}

#[cfg(test)]
mod tests;
