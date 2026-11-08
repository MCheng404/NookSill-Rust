//! The timer panel painter and its hit layout.

use super::*;

/// State of the todo input field.
///
/// The three fields travel together, otherwise `draw_panel` would take more
/// than 7 arguments.
#[derive(Copy, Clone, Debug)]
pub(super) struct Input<'a> {
    /// Whether the field is in typing mode.
    pub(super) typing: bool,
    /// Committed text.
    pub(super) typed: &'a str,
    /// Pinyin currently being composed.
    pub(super) composing: &'a str,
}

/// Paints the panel and returns the frame's hit rects.
pub(super) fn draw_panel(
    ui: &mut egui::Ui,
    dark: bool,
    frosted: bool,
    state: &ToolState,
    now_ms: i64,
    hover: Option<egui::Pos2>,
    input: Input<'_>,
) -> Hits {
    let rect = ui.max_rect();
    let painter = ui.painter();
    // Shared panel style. The old code passed `argbc(a,r,g,b)` as if it were
    // RGBA here, which gave bright pink in dark mode and red at alpha 34 in
    // light mode, so the text was unreadable.
    let style = PanelStyle::current(dark, frosted);
    style.paint_frame(painter, rect);
    let (tc, sub) = (style.ink(), style.ink_sub());
    let accent = ui.visuals().selection.bg_fill;
    // Secondary pill fill and hover fill, sharing the sub-card color tiers.
    let pill = style.card();
    let hover_bg = style.card_hover();

    let pad = 16.0;
    let mut hits = Hits::default();
    let x0 = rect.left() + pad;
    let w = rect.width() - 2.0 * pad;

    // Title
    painter.text(
        egui::pos2(rect.left() + pad, rect.top() + pad),
        Align2::LEFT_TOP,
        "计时器 · 待办",
        FontId::proportional(16.0),
        tc,
    );

    // Mode segments (countdown / stopwatch)
    let sy = rect.top() + pad + 28.0;
    for (i, mode) in [ToolMode::Countdown, ToolMode::Stopwatch]
        .iter()
        .enumerate()
    {
        let r =
            EguiRect::from_min_size(egui::pos2(x0 + i as f32 * 68.0, sy), egui::vec2(64.0, 26.0));
        hits.mode[i] = r;
        let active = state.mode == *mode;
        painter.rect_filled(
            r,
            CornerRadius::same(PanelStyle::R_CARD),
            if active { accent } else { pill },
        );
        painter.text(
            r.center(),
            Align2::CENTER_CENTER,
            mode.title(),
            FontId::proportional(12.0),
            if active { Color32::WHITE } else { sub },
        );
    }

    // Big number
    let big_y = sy + 34.0;
    painter.text(
        egui::pos2(x0, big_y),
        Align2::LEFT_TOP,
        format_ms(state.display_ms(now_ms), state.mode),
        FontId::proportional(44.0),
        tc,
    );

    // Four buttons: start/pause, reset, -1 min, +1 min
    let by = big_y + 62.0;
    let bw = (w - 8.0) / 2.0;
    let labels = [
        if state.running { "暂停" } else { "开始" },
        "重置",
        "− 1 分钟",
        "+ 1 分钟",
    ];
    for (i, label) in labels.iter().enumerate() {
        let col = i % 2;
        let row = i / 2;
        let r = EguiRect::from_min_size(
            egui::pos2(x0 + col as f32 * (bw + 8.0), by + row as f32 * 42.0),
            egui::vec2(bw, 34.0),
        );
        hits.action[i] = r;
        // Only start/pause uses the accent color, the rest are secondary.
        let fill = if i == 0 {
            accent
        } else if hover == Some(r.center()) || hover.is_some_and(|p| r.contains(p)) {
            hover_bg
        } else {
            pill
        };
        painter.rect_filled(r, CornerRadius::same(PanelStyle::R_FIELD), fill);
        painter.text(
            r.center(),
            Align2::CENTER_CENTER,
            *label,
            FontId::proportional(13.0),
            if i == 0 { Color32::WHITE } else { tc },
        );
    }

    // Todo list
    let mut ty = by + 42.0 * 2.0 + 12.0;
    painter.text(
        egui::pos2(x0, ty),
        Align2::LEFT_TOP,
        "待办事项",
        FontId::proportional(13.0),
        tc,
    );
    ty += 22.0;

    for todo in &state.todos {
        let r = EguiRect::from_min_size(egui::pos2(x0, ty), egui::vec2(w, 24.0));
        hits.todos.push(r);
        let del = EguiRect::from_min_size(egui::pos2(r.right() - 24.0, ty), egui::vec2(24.0, 24.0));
        hits.deletes.push(del);
        if hover.is_some_and(|p| r.contains(p)) {
            painter.rect_filled(r, CornerRadius::same(PanelStyle::R_SMALL), hover_bg);
        }
        // Checkbox
        let box_rect =
            EguiRect::from_min_size(egui::pos2(x0 + 2.0, ty + 6.0), egui::vec2(12.0, 12.0));
        painter.rect_filled(
            box_rect,
            CornerRadius::same(PanelStyle::R_SMALL),
            if todo.done { accent } else { pill },
        );
        if todo.done {
            // The check mark is drawn from segments, not a character: the
            // bundled font has no check glyph and would render a tofu box.
            check_mark(painter, box_rect.center(), 3.0, Color32::WHITE);
        }
        painter.text(
            egui::pos2(x0 + 22.0, ty + 4.0),
            Align2::LEFT_TOP,
            &todo.text,
            FontId::proportional(12.0),
            if todo.done { sub } else { tc },
        );
        // Same for the delete symbol: the cross glyph is missing too.
        cross_mark(painter, del.center(), 3.0, sub);
        ty += 25.0;
    }

    // Add row / input field
    let add = EguiRect::from_min_size(egui::pos2(x0, ty), egui::vec2(w, 24.0));
    hits.add = add;
    if input.typing {
        painter.rect_filled(add, PanelStyle::pill(add.height()), pill);
        // Committed text plus the pinyin being composed. The pinyin gets an
        // underline so the user sees it is still in the IME and not lost.
        let shown = format!("{}{}", input.typed, input.composing);
        if shown.is_empty() {
            painter.text(
                egui::pos2(x0 + 6.0, ty + 4.0),
                Align2::LEFT_TOP,
                "输入后回车…",
                FontId::proportional(12.0),
                sub,
            );
        } else {
            painter.text(
                egui::pos2(x0 + 6.0, ty + 4.0),
                Align2::LEFT_TOP,
                &shown,
                FontId::proportional(12.0),
                tc,
            );
            if !input.composing.is_empty() {
                let w_all = ui.ctx().fonts_mut(|f| {
                    f.layout_no_wrap(shown.clone(), FontId::proportional(12.0), Color32::WHITE)
                        .size()
                        .x
                });
                let w_done = ui.ctx().fonts_mut(|f| {
                    f.layout_no_wrap(
                        input.typed.to_owned(),
                        FontId::proportional(12.0),
                        Color32::WHITE,
                    )
                    .size()
                    .x
                });
                painter.line_segment(
                    [
                        egui::pos2(x0 + 6.0 + w_done, ty + 18.0),
                        egui::pos2(x0 + 6.0 + w_all, ty + 18.0),
                    ],
                    egui::Stroke::new(1.0, tc),
                );
            }
        }
    } else {
        if hover.is_some_and(|p| add.contains(p)) {
            painter.rect_filled(add, PanelStyle::pill(add.height()), hover_bg);
        }
        painter.text(
            egui::pos2(x0 + 2.0, ty + 4.0),
            Align2::LEFT_TOP,
            if state.todos.len() >= MAX_TODOS {
                "已达上限（16 条）"
            } else {
                "＋ 添加待办…"
            },
            FontId::proportional(12.0),
            sub,
        );
    }

    // Bottom hint: number of pending items
    let pending = state.pending_count();
    if pending > 0 {
        painter.text(
            egui::pos2(rect.right() - pad, rect.top() + pad),
            Align2::RIGHT_TOP,
            format!("{pending} 项待办"),
            FontId::proportional(11.0),
            sub,
        );
    }

    hits
}

/// Paints a check mark: two segments, a short left stroke and a long right
/// stroke.
///
/// Not the check character: the bundled font has no glyph for it and would
/// render a tofu box.
pub(super) fn check_mark(painter: &egui::Painter, center: egui::Pos2, r: f32, color: Color32) {
    let stroke = egui::Stroke::new(1.6, color);
    let p0 = egui::pos2(center.x - r * 0.7, center.y);
    let p1 = egui::pos2(center.x - r * 0.15, center.y + r * 0.6);
    let p2 = egui::pos2(center.x + r * 0.75, center.y - r * 0.6);
    painter.line_segment([p0, p1], stroke);
    painter.line_segment([p1, p2], stroke);
}

/// Paints a cross: two diagonals, also without the cross character.
pub(super) fn cross_mark(painter: &egui::Painter, center: egui::Pos2, r: f32, color: Color32) {
    let stroke = egui::Stroke::new(1.4, color);
    painter.line_segment(
        [
            egui::pos2(center.x - r, center.y - r),
            egui::pos2(center.x + r, center.y + r),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x + r, center.y - r),
            egui::pos2(center.x - r, center.y + r),
        ],
        stroke,
    );
}
