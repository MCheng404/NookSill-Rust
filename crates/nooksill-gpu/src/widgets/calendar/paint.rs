//! The calendar panel painter.

use super::*;

/// English weekday names; the Chinese names come from core::calendar.
const WEEKDAY_EN: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// Relaxed layout: one source for both painting and the fit test below.
const SIDE_PAD: f32 = 18.0;
const TOP_PAD: f32 = 18.0;
const NAV_H: f32 = 36.0;
const HEAD_GAP: f32 = 14.0;
const HEAD_H: f32 = 32.0;
const CELL_H: f32 = 54.0;
const INFO_GAP: f32 = 12.0;
const LINE_H: f32 = 26.0;
const INFO_TOP: f32 = 12.0;

const FONT_NAV: f32 = 15.0;
const FONT_TODAY: f32 = 14.0;
const FONT_WEEKDAY: f32 = 12.0;
const FONT_DAY: f32 = 16.0;
const FONT_SUB: f32 = 10.5;
const FONT_INFO: f32 = 13.0;

/// Paints navigation, weekday header, grid and the selected day's details.
///
/// Everything is positioned explicitly. Returns the frame's hit rects.
pub(super) fn draw_panel(
    ui: &mut egui::Ui,
    dark: bool,
    frosted: bool,
    state: &CalendarState,
    today: (i32, u32, u32),
    hover: Option<Hit>,
) -> Hits {
    let rect = ui.max_rect();
    let painter = ui.painter().clone();

    // Shared panel style: base fill, overlay, hairline, three text tiers.
    let style = PanelStyle::current(dark, frosted);
    style.paint_frame(&painter, rect);
    let (tc, sub, faded) = (style.ink(), style.ink_sub(), style.faint());
    let accent = ui.visuals().selection.bg_fill;
    let red = Color32::from_rgba_unmultiplied(230, 74, 70, 255);
    let hover_bg = style.card_hover();
    let line_c = style.line();

    // Layout baseline, all values from `CalDraw`'s `Sc` numbers.
    let x0 = rect.left() + PM + SIDE_PAD;
    let y0 = rect.top() + PM + TOP_PAD;
    let cw = PANEL_W - 2.0 * PM - 2.0 * SIDE_PAD;
    let bw = 32.0_f32;
    let nav_h = NAV_H;

    let mut hits = Hits::default();

    // Navigation row: year < >, month < >, Today.
    let nx = [
        x0,
        x0 + bw + 62.0,
        x0 + 132.0,
        x0 + 132.0 + bw + 48.0,
        x0 + cw - 58.0,
    ];
    let nw = [bw, bw, bw, bw, 58.0];
    for i in 0..5 {
        hits.nav[i] = EguiRect::from_min_size(egui::pos2(nx[i], y0), egui::vec2(nw[i], nav_h));
        // Only Today keeps a permanent fill; arrows fill on hover only.
        if i == 4 {
            let on = matches!(hover, Some(Hit::Nav(4)));
            painter.rect_filled(
                hits.nav[i],
                PanelStyle::pill(nav_h),
                if on {
                    accent
                } else {
                    // Theme color at 8..10% alpha.
                    Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 46)
                },
            );
        } else if matches!(hover, Some(Hit::Nav(j)) if j == i) {
            painter.rect_filled(hits.nav[i], PanelStyle::pill(nav_h), hover_bg);
        }
    }

    // Arrows are drawn as segments, not "<" glyphs: the bundled font may
    // lack those code points.
    for (idx, left) in [(0_usize, nx[0]), (1, nx[1]), (2, nx[2]), (3, nx[3])] {
        let cx = left + bw * 0.5;
        let cy = y0 + nav_h * 0.5;
        let dir = if idx % 2 == 0 { -1.0 } else { 1.0 };
        let stroke = Stroke::new(1.8, tc);
        painter.line_segment(
            [
                egui::pos2(cx - dir * 3.0, cy - 5.0),
                egui::pos2(cx + dir * 3.0, cy),
            ],
            stroke,
        );
        painter.line_segment(
            [
                egui::pos2(cx + dir * 3.0, cy),
                egui::pos2(cx - dir * 3.0, cy + 5.0),
            ],
            stroke,
        );
    }

    painter.text(
        egui::pos2(nx[0] + bw + 31.0, y0 + nav_h * 0.5),
        Align2::CENTER_CENTER,
        nooksill_core::i18n::fmt("Year {0}", &[(state.year).to_string().as_str()]),
        FontId::proportional(FONT_NAV),
        tc,
    );
    painter.text(
        egui::pos2(nx[2] + bw + 24.0, y0 + nav_h * 0.5),
        Align2::CENTER_CENTER,
        nooksill_core::i18n::fmt("Month {0}", &[(state.month).to_string().as_str()]),
        FontId::proportional(FONT_NAV),
        tc,
    );
    painter.text(
        hits.nav[4].center(),
        Align2::CENTER_CENTER,
        nooksill_core::i18n::t("Today"),
        FontId::proportional(FONT_TODAY),
        if matches!(hover, Some(Hit::Nav(4))) {
            Color32::WHITE
        } else {
            accent
        },
    );

    // Weekday header, Monday first.
    let wy = y0 + nav_h + HEAD_GAP;
    let cell_w = cw / 7.0;
    let cell_h = CELL_H;
    let gy = wy + HEAD_H;
    for i in 0..7 {
        // i = 0 is Monday, i.e. WEEKDAY[(1+0)%7].
        let name = calendar::weekday_name(((i + 1) % 7) as u32);
        painter.text(
            egui::pos2(x0 + (i as f32 + 0.5) * cell_w, wy + 12.0),
            Align2::CENTER_CENTER,
            name,
            FontId::proportional(FONT_WEEKDAY),
            if i >= 5 { red } else { sub },
        );
    }

    // 6 x 7 grid.
    let sel = state.selected;
    for (k, c) in state.cells().iter().enumerate() {
        let col = k % 7;
        let row = k / 7;
        let x = x0 + col as f32 * cell_w;
        let y = gy + row as f32 * cell_h;
        hits.cell[k] = EguiRect::from_min_size(egui::pos2(x, y), egui::vec2(cell_w, cell_h));

        let is_today = (c.year, c.month, c.day) == today;
        let is_sel = (c.year, c.month, c.day) == sel;
        let r = (cell_w.min(cell_h)) * 0.5 - 2.0;
        let cx = x + cell_w * 0.5;
        let cy = y + cell_h * 0.5 - 3.0;

        if is_today {
            painter.circle_filled(egui::pos2(cx, cy), r, accent);
        } else if matches!(hover, Some(Hit::Cell(j)) if j == k) {
            painter.circle_filled(egui::pos2(cx, cy), r, hover_bg);
        }
        if is_sel && !is_today {
            painter.circle_stroke(egui::pos2(cx, cy), r, Stroke::new(1.8, accent));
        } else if is_sel {
            painter.circle_stroke(
                egui::pos2(cx, cy),
                r - 2.0,
                Stroke::new(1.6, Color32::WHITE),
            );
        }

        let num_color = if is_today {
            Color32::WHITE
        } else if !c.in_month {
            faded
        } else if col >= 5 {
            red
        } else {
            tc
        };
        painter.text(
            egui::pos2(cx, cy - 2.0),
            Align2::CENTER_BOTTOM,
            c.day.to_string(),
            FontId::proportional(FONT_DAY),
            num_color,
        );

        let sub_color = if is_today {
            Color32::from_rgba_unmultiplied(255, 255, 255, 230)
        } else if !c.in_month {
            faded
        } else if c.highlight {
            red
        } else {
            sub
        };
        painter.text(
            egui::pos2(cx, cy + 2.0),
            Align2::CENTER_TOP,
            &c.sub,
            FontId::proportional(FONT_SUB),
            sub_color,
        );
    }

    // Details of the selected day.
    let info_y = gy + 6.0 * cell_h + INFO_GAP;
    painter.line_segment(
        [egui::pos2(x0, info_y), egui::pos2(x0 + cw, info_y)],
        Stroke::new(1.0, line_c),
    );

    let (sy, sm, sd) = sel;
    let weekday = calendar::weekday_of(sy, sm, sd);
    let is_today = sel == today;
    let weekday_text = if nooksill_core::i18n::language() == nooksill_core::i18n::Language::En {
        WEEKDAY_EN
            .get(weekday as usize)
            .copied()
            .unwrap_or("Sun")
            .to_owned()
    } else {
        calendar::weekday_name(weekday).to_owned()
    };
    let line1 = nooksill_core::i18n::fmt(
        "{0}-{1}-{2}  {3}{4}",
        &[
            (sy).to_string().as_str(),
            (sm).to_string().as_str(),
            (sd).to_string().as_str(),
            weekday_text.as_str(),
            (if is_today {
                nooksill_core::i18n::t("  · Today")
            } else {
                ""
            })
            .to_string()
            .as_str(),
        ],
    );
    let line2 = match calendar::solar_to_lunar(sy, sm, sd) {
        Some(l) => nooksill_core::i18n::fmt(
            "Lunar  {0}",
            &[(calendar::lunar_text(l)).to_string().as_str()],
        ),
        None => nooksill_core::i18n::t("Lunar  (1901-2099 only)").to_owned(),
    };
    let line3 = nooksill_core::i18n::fmt(
        "Ganzhi  {0}",
        &[(calendar::ganzhi_text(sy, sm, sd)).to_string().as_str()],
    );
    let line4 = match calendar::next_term(sy, sm, sd) {
        Some((y, n, days)) => {
            let (tm, td) = calendar::term_date(y, n);
            nooksill_core::i18n::fmt(
                "Next solar term  {0}  {1}/{2} ({3} days left)",
                &[
                    (calendar::term_name(n)).to_string().as_str(),
                    (tm).to_string().as_str(),
                    (td).to_string().as_str(),
                    (days).to_string().as_str(),
                ],
            )
        }
        None => String::new(),
    };

    let lh = LINE_H;
    let lines = [(line1, tc), (line2, tc), (line3, sub), (line4, sub)];
    for (i, (text, color)) in lines.iter().enumerate() {
        painter.text(
            egui::pos2(x0, info_y + INFO_TOP + i as f32 * lh),
            Align2::LEFT_TOP,
            text,
            FontId::proportional(FONT_INFO),
            *color,
        );
    }

    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The looser spacing must not push the fourth detail line off the panel.
    #[test]
    fn relaxed_layout_fits_and_keeps_nav_buttons_apart() {
        let y0 = PM + TOP_PAD;
        let wy = y0 + NAV_H + HEAD_GAP;
        let gy = wy + HEAD_H;
        let info_y = gy + 6.0 * CELL_H + INFO_GAP;
        let last_line = info_y + INFO_TOP + 3.0 * LINE_H + FONT_INFO + 2.0;
        assert!(
            last_line < PANEL_H - PM,
            "last detail line overflows: {last_line}"
        );

        let cw = PANEL_W - 2.0 * PM - 2.0 * SIDE_PAD;
        assert!(cw / 7.0 >= CELL_H * 0.9, "cells became narrow strips");

        let bw = 32.0_f32;
        let nav_end = 132.0 + bw + 48.0 + bw;
        assert!(cw - 58.0 > nav_end, "nav buttons overlap Today");
    }
}
