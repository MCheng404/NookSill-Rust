use super::*;

/// Round-trip over the whole supported range; nothing below matters if this
/// fails.
#[test]
fn civil_roundtrip_over_the_supported_range() {
    for y in MIN_YEAR..=MAX_YEAR {
        for m in 1..=12 {
            // Days past the month end roll into the next month, so day 31
            // covers carry-over as well.
            for d in 1..=31 {
                let z = days_from_civil(y, m, d);
                let (y2, m2, d2) = civil_from_days(z);
                let expect = days_from_civil(y2, m2, d2);
                assert_eq!(z, expect, "{y}-{m}-{d} 往返不一致");
            }
        }
    }
}

/// Known weekdays; the index is 0 = Sunday, matching `weekday_name`.
#[test]
fn weekday_anchors() {
    assert_eq!(weekday_of(1970, 1, 1), 4, "1970-01-01 是周四");
    assert_eq!(weekday_of(2000, 1, 1), 6, "2000-01-01 是周六");
    assert_eq!(weekday_of(2026, 11, 2), 1, "2026-11-02 是周一");
    assert_eq!(weekday_name(0), "日");
    assert_eq!(weekday_name(1), "一");
}

/// Lunar new year is the day a wrong leap month or month length moves first.
///
/// The day before must be month 12 and never day 1, which catches both errors.
#[test]
fn spring_festival_anchors() {
    let anchors = [
        ((2020, 1, 25), 2020),
        ((2021, 2, 12), 2021),
        ((2022, 2, 1), 2022),
        ((2023, 1, 22), 2023),
        ((2024, 2, 10), 2024),
        ((2025, 1, 29), 2025),
        ((2026, 2, 17), 2026),
    ];
    for ((y, m, d), ly) in anchors {
        let l = solar_to_lunar(y, m, d).expect("范围内");
        assert_eq!(
            (l.year, l.month, l.day, l.leap),
            (ly, 1, 1, false),
            "{y}-{m}-{d} 应当是农历 {ly} 年正月初一"
        );
        // The day before is the last day of month 12 of the previous year.
        let (py, pm, pd) = civil_from_days(days_from_civil(y, m, d) - 1);
        let prev = solar_to_lunar(py, pm, pd).expect("范围内");
        assert_eq!(prev.month, 12, "{py}-{pm}-{pd} 应当是腊月");
        assert_ne!(prev.day, 1, "春节前一天不该是初一");
    }
}

/// Leap months are the branch a plain month table never exercises.
#[test]
fn leap_month_anchors() {
    // 2020 has a leap month 4, starting 2020-05-23.
    let l = solar_to_lunar(2020, 5, 23).expect("范围内");
    assert_eq!((l.month, l.day, l.leap), (4, 1, true));
    // 2023 has a leap month 2, starting 2023-03-22.
    let l = solar_to_lunar(2023, 3, 22).expect("范围内");
    assert_eq!((l.month, l.day, l.leap), (2, 1, true));
    // 2017 has a leap month 6, starting 2017-07-23.
    let l = solar_to_lunar(2017, 7, 23).expect("范围内");
    assert_eq!((l.month, l.day, l.leap), (6, 1, true));
}

/// Every day from 1901-01-01 to 2099-12-31 must survive solar -> lunar ->
/// solar. Sampling cannot do this: one wrong month length breaks one date.
#[test]
fn solar_lunar_roundtrip_every_day() {
    let start = days_from_civil(MIN_YEAR, 1, 1);
    let end = days_from_civil(MAX_YEAR, 12, 31);
    let mut checked = 0_u32;
    for z in start..=end {
        let (y, m, d) = civil_from_days(z);
        let l = solar_to_lunar(y, m, d).unwrap_or_else(|| panic!("{y}-{m}-{d} 换算失败"));
        assert!(
            (1..=12).contains(&l.month) && (1..=30).contains(&l.day),
            "{y}-{m}-{d} 得到非法农历 {:?}",
            l
        );
        let back = lunar_to_solar(l).unwrap_or_else(|| panic!("{y}-{m}-{d} 反算失败: {l:?}"));
        assert_eq!((y, m, d), back, "{y}-{m}-{d} 往返不一致（农历 {l:?}）");
        checked += 1;
    }
    assert_eq!(checked, 72_684, "1901..=2099 共 72684 天");
}

/// A day counter that skips or repeats still round-trips, so continuity is
/// checked on its own: each lunar day is either +1 or the 1st of a new month.
#[test]
fn lunar_days_are_contiguous() {
    let start = days_from_civil(MIN_YEAR, 1, 1);
    let end = days_from_civil(MAX_YEAR, 12, 31);
    let mut prev = solar_to_lunar(MIN_YEAR, 1, 1).expect("范围内");
    for z in start + 1..=end {
        let (y, m, d) = civil_from_days(z);
        let cur = solar_to_lunar(y, m, d).expect("范围内");
        let expected_day = if cur.day == 1 {
            assert_ne!(prev.day, 1, "{y}-{m}-{d} 连续两天都是初一");
            assert!(
                prev.day == 29 || prev.day == 30,
                "上个月只有 {} 天",
                prev.day
            );
            1
        } else {
            prev.day + 1
        };
        assert_eq!(cur.day, expected_day, "{y}-{m}-{d} 农历日不连续");
        prev = cur;
    }
}

/// The mean-sun formula is up to a day off, so the assertion is a window.
#[test]
fn solar_terms_land_in_the_right_windows() {
    for y in 1901..=2099 {
        let (m, d) = term_date(y, 2); // start of spring
        assert_eq!(m, 2, "{y} 立春应当在 2 月");
        assert!((3..=5).contains(&d), "{y} 立春在 2 月 {d} 日");
        let (m, d) = term_date(y, 23); // winter solstice
        assert_eq!(m, 12, "{y} 冬至应当在 12 月");
        assert!((21..=23).contains(&d), "{y} 冬至在 12 月 {d} 日");
        let (m, d) = term_date(y, 5); // spring equinox
        assert_eq!(m, 3, "{y} 春分应当在 3 月");
        // The mean-sun formula drifts over March 19 to 22 across 199 years.
        assert!((19..=22).contains(&d), "{y} 春分在 3 月 {d} 日");
        let (m, d) = term_date(y, 11); // summer solstice
        assert_eq!(m, 6, "{y} 夏至应当在 6 月");
        assert!((20..=22).contains(&d), "{y} 夏至在 6 月 {d} 日");
    }
}

/// A festival outranks a solar term, which outranks the lunar day.
#[test]
fn sub_labels_follow_priority() {
    // Gregorian holiday.
    assert_eq!(sub_label(2026, 1, 1).0, "元旦");
    // Lunar new year.
    assert_eq!(sub_label(2026, 2, 17).0, "春节");
    // Solar term.
    assert_eq!(sub_label(2026, 2, 4).0, "立春");
    // A plain day falls back to the lunar day name. 2026-02-18 is the rain
    // term, so search forward for the first day that is neither.
    let (d, (label, _)) = (18..=28)
        .map(|d| (d, sub_label(2026, 2, d)))
        .find(|(_, (_, hl))| !hl)
        .expect("春节后总能找到普通日子");
    assert!(
        label.starts_with('初') || label.starts_with('十') || label.starts_with('廿'),
        "2026-02-{d} 应当显示农历日名，实际是 {label}"
    );
    // Every date has some sub-label, lunar day or month name.
    for d in 1..=28 {
        assert!(
            !sub_label(2026, 11, d).0.is_empty(),
            "2026-11-{d} 没有副标签"
        );
    }
}

/// The year pillar turns at the start of spring, not on 1 January.
#[test]
fn ganzhi_year_turns_at_lichun() {
    // Before the start of spring, still the previous year.
    assert!(
        ganzhi_text(2024, 2, 3).starts_with("癸卯年"),
        "{}",
        ganzhi_text(2024, 2, 3)
    );
    // After it, the new year.
    assert!(
        ganzhi_text(2024, 2, 5).starts_with("甲辰年"),
        "{}",
        ganzhi_text(2024, 2, 5)
    );
}

/// The day pillar repeats every 60 days; consecutive days must differ.
#[test]
fn ganzhi_day_cycles_by_sixty() {
    let a = ganzhi_text(2026, 11, 2);
    let b = ganzhi_text(2026, 11, 3);
    let day_of = |s: &str| s.rsplit("  ").next().unwrap_or("").to_owned();
    assert_ne!(day_of(&a), day_of(&b));

    // 60 days later only the day pillar matches: year and month may differ.
    let later = civil_from_days(days_from_civil(2026, 11, 2) + 60);
    let c = ganzhi_text(later.0, later.1, later.2);
    assert_eq!(day_of(&a), day_of(&c), "60 天后日柱应当相同");
}

/// Two solar terms are at most 16 days apart, and the next one is not today.
#[test]
fn next_term_is_within_a_fortnight() {
    for (y, m, d) in [(2026, 11, 2), (2024, 2, 4), (1999, 12, 31)] {
        let (_, n, days) = next_term(y, m, d).expect("总能找到");
        assert!(
            (1..=16).contains(&days),
            "{y}-{m}-{d} 下一个节气还有 {days} 天"
        );
        assert!(n < 24);
    }
}

/// Fixed 42 cells starting on the Monday of the week holding the 1st.
#[test]
fn grid_starts_on_monday() {
    let s = CalendarState::new((2026, 11, 2));
    let cells = s.cells();
    // 2026-11-01 is a Sunday, so that week starts on 10-26.
    assert_eq!((cells[0].month, cells[0].day), (10, 26));
    assert!(!cells[0].in_month, "10-26 不属于 11 月");
    // The 1st lands in the seventh cell.
    assert_eq!((cells[6].month, cells[6].day), (11, 1));
    assert!(cells[6].in_month);
    // 42 cells always cover six whole weeks.
    let first = days_from_civil(cells[0].year, cells[0].month, cells[0].day);
    let last = days_from_civil(cells[41].year, cells[41].month, cells[41].day);
    assert_eq!(last - first, 41);
}

/// Month steps carry into the year, year steps ignore the month.
#[test]
fn stepping_wraps_across_years() {
    let mut s = CalendarState::new((2026, 12, 15));
    s.step(0, 1);
    assert_eq!((s.year, s.month), (2027, 1));
    s.step(0, -1);
    assert_eq!((s.year, s.month), (2026, 12));
    s.step(1, 0);
    assert_eq!(s.year, 2027);
}

/// The shown year stops at `MAX_YEAR - 1`: the grid spills into the next year,
/// so lookups must stay inside the table.
#[test]
fn stepping_is_clamped_to_the_table() {
    let mut s = CalendarState::new((2026, 1, 1));
    s.step(-200, 0);
    assert_eq!(s.year, MIN_YEAR);
    let mut s = CalendarState::new((2026, 1, 1));
    s.step(200, 0);
    assert_eq!(s.year, MAX_YEAR - 1);
    // Clamping the year must leave a valid month behind.
    assert!((1..=12).contains(&s.month));
}

/// Selecting a cell of the leading or trailing month moves the view there.
#[test]
fn selecting_a_leading_cell_flips_the_month() {
    let mut s = CalendarState::new((2026, 11, 2));
    let cell = &s.cells()[0];
    let (y, m, d) = (cell.year, cell.month, cell.day);
    s.select(y, m, d);
    assert_eq!((s.year, s.month), (2026, 10));
    assert_eq!(s.selected, (2026, 10, 26));
}
