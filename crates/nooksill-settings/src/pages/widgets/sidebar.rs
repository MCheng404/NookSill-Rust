//! Left navigation: primary entries, or one group's content tabs with a back
//! row pinned to the bottom.

use std::time::{Duration, Instant};

use egui::{Color32, CornerRadius, FontId, Rect, Sense, Ui, Vec2, WidgetInfo};

use crate::pages::nav::{NavGroup, NavItem};
use crate::pages::NavWipe;

use super::motion::magnet_offset;
use super::*;

/// Longer than the pill so the level change reads as a drawn column.
const WIPE_S: f32 = 0.22;

/// Dispatch on whether a group is open. The filter applies to the labels
/// currently on screen, so a buried page is reachable by first selecting its
/// group (primary rows match on their tab labels too).
pub(crate) fn sidebar(ui: &mut Ui, state: &mut SettingsState) {
    let now = Instant::now();
    let wipe = state.nav_wipe.and_then(|w| {
        let t = now.saturating_duration_since(w.at).as_secs_f32() / WIPE_S;
        (t < 1.0).then_some((t.min(1.0), w.down))
    });
    if wipe.is_none() {
        state.nav_wipe = None;
    } else {
        // A deadline, not an immediate repaint: an immediate one would stop
        // the headless harness from ever settling.
        ui.ctx().request_repaint_after(Duration::from_millis(16));
    }
    match state.sub {
        Some(group) => tabs(ui, state, group, wipe),
        None => primary(ui, state, wipe),
    }
}

/// Clip covering the revealed part of a column; widget hit rects stay put.
fn reveal_clip(rows: &[Rect], wipe: Option<(f32, bool)>, col: Rect) -> Rect {
    let Some((t, down)) = wipe else {
        return col;
    };
    if rows.is_empty() {
        return col;
    }
    let n = rows.len() as f32;
    let frontier = (t * n).min(n);
    if frontier >= n {
        return col;
    }
    let idx = frontier.floor() as usize;
    let frac = frontier - frontier.floor();
    if down {
        let r = rows[idx.min(rows.len() - 1)];
        Rect::from_min_max(
            col.min,
            egui::pos2(col.right(), r.top() + r.height() * frac),
        )
    } else {
        let r = rows[rows.len() - 1 - idx.min(rows.len() - 1)];
        Rect::from_min_max(
            egui::pos2(col.left(), r.bottom() - r.height() * frac),
            col.max,
        )
    }
}

fn primary(ui: &mut Ui, state: &mut SettingsState, wipe: Option<(f32, bool)>) {
    let filter = state.nav_filter.trim().to_lowercase();
    let mut rows: Vec<(NavItem, Rect, egui::Response)> = Vec::new();
    for item in NavItem::PRIMARY {
        if !item.matches(&filter) {
            continue;
        }
        let (rect, resp) = nav_row(ui, item.label(), item.owns(state.page));
        rows.push((item, rect, resp));
    }

    // Apply the click before painting: the pill then starts from where it was
    // and heads for the new row in this very frame.
    let clicked = rows.iter().position(|(_, _, r)| r.clicked());
    if let Some(i) = clicked {
        state.tip = None;
        state.tip_hover = None;
        apply(rows[i].0, state, &filter);
    }

    let col = ui.max_rect();
    let rects: Vec<Rect> = rows.iter().map(|(_, r, _)| *r).collect();
    let clip = reveal_clip(&rects, wipe, col);
    let selected = rows
        .iter()
        .find(|(item, _, _)| item.owns(state.page))
        .map(|(_, rect, _)| *rect);
    match selected {
        Some(to) => draw_pill(ui, clip, state, to),
        None => state.nav_pill = None,
    }
    for (item, rect, resp) in &rows {
        paint_nav_row(
            ui,
            clip,
            *rect,
            item.label(),
            item.owns(state.page),
            resp.hovered(),
        );
    }

    // Only a plain page can be auto-selected: jumping into a group on a search
    // keystroke would replace the whole column unexpectedly.
    if clicked.is_none() && selected.is_none() {
        if let Some(p) = rows.iter().find_map(|(item, _, _)| match item {
            NavItem::Page(p) => Some(*p),
            NavItem::Group(_) => None,
        }) {
            state.primary = NavItem::Page(p);
            state.page = p;
            state.sub = None;
        }
    }
}

fn apply(item: NavItem, state: &mut SettingsState, filter: &str) {
    state.primary = item;
    match item {
        NavItem::Page(p) => {
            state.page = p;
            state.sub = None;
        }
        NavItem::Group(g) => {
            state.sub = Some(g);
            state.nav_wipe = Some(NavWipe {
                down: true,
                at: Instant::now(),
            });
            // A search that matched a tab of this group lands on that tab.
            state.page = g
                .tabs()
                .iter()
                .find(|p| !filter.is_empty() && p.title().to_lowercase().contains(filter))
                .copied()
                .unwrap_or(g.tabs()[0]);
        }
    }
}

fn tabs(ui: &mut Ui, state: &mut SettingsState, group: NavGroup, wipe: Option<(f32, bool)>) {
    let filter = state.nav_filter.trim().to_lowercase();
    // The group name is the window title in this mode; a second, smaller copy
    // at the top of the column was redundant and hard to read.
    ui.add_space(8.0);

    let mut rows: Vec<(Page, Rect, egui::Response)> = Vec::new();
    for p in group.tabs() {
        if !filter.is_empty() && !p.title().to_lowercase().contains(&filter) {
            continue;
        }
        let (rect, resp) = nav_row(ui, p.title(), state.page == *p);
        rows.push((*p, rect, resp));
    }

    let clicked = rows.iter().position(|(_, _, r)| r.clicked());
    if let Some(i) = clicked {
        state.tip = None;
        state.tip_hover = None;
        state.page = rows[i].0;
    }
    let col = ui.max_rect();
    let rects: Vec<Rect> = rows.iter().map(|(_, r, _)| *r).collect();
    let clip = reveal_clip(&rects, wipe, col);
    let selected = rows
        .iter()
        .find(|(p, _, _)| *p == state.page)
        .map(|(_, rect, _)| *rect);
    match selected {
        Some(to) => draw_pill(ui, clip, state, to),
        None => state.nav_pill = None,
    }
    for (p, rect, resp) in &rows {
        paint_nav_row(ui, clip, *rect, p.title(), *p == state.page, resp.hovered());
    }
    if clicked.is_none() && selected.is_none() {
        if let Some((p, _, _)) = rows.first() {
            state.page = *p;
        }
    }

    // Escape steps out of the group. A focused text field consumes it first
    // (egui surrenders focus on Escape), and an open popup or editor owns it,
    // so this only fires when the column itself has focus.
    let typing = ui.memory(|m| m.focused().is_some());
    if !typing
        && state.modal.is_none()
        && state.tip.is_none()
        && ui.input(|i| i.key_pressed(egui::Key::Escape))
    {
        go_back(state);
    }

    back_row(ui, state, clip);
}

/// Returns to the primary column, keeping the group highlighted.
fn go_back(state: &mut SettingsState) {
    if let Some(group) = state.sub {
        state.primary = NavItem::Group(group);
    }
    state.sub = None;
    state.nav_wipe = Some(NavWipe {
        down: false,
        at: Instant::now(),
    });
    state.tip = None;
    state.tip_hover = None;
}

/// Bottom-pinned return row; it stays visible while the tab strip scrolls.
fn back_row(ui: &mut Ui, state: &mut SettingsState, clip: Rect) {
    let area = ui.max_rect();
    let h = NAV_ITEM_H.min(area.height());
    let rect = Rect::from_min_size(
        egui::pos2(area.left(), area.bottom() - h),
        Vec2::new(area.width().max(1.0), h.max(1.0)),
    );
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let label = nooksill_core::i18n::t("< Back");
    let (r, resp) = row.allocate_exact_size(Vec2::new(rect.width(), h), Sense::click());
    let draw = r.translate(magnet_offset(&row, r, 2.0, 40.0));
    let vis = row.visuals();
    let accent = vis.selection.bg_fill;
    let p = row.painter().with_clip_rect(clip);
    // Always-on accent fill plus a left bar: a hover-only row at the bottom of
    // a long tab list was too easy to miss, and this is the only way out.
    let alpha = if resp.hovered() { 46 } else { 26 };
    p.rect_filled(
        draw,
        CornerRadius::same(13),
        Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), alpha),
    );
    p.rect_filled(
        Rect::from_min_size(
            egui::pos2(draw.left() + 5.0, draw.top() + 7.0),
            Vec2::new(3.0, (draw.height() - 14.0).max(1.0)),
        ),
        CornerRadius::same(2),
        accent,
    );
    p.text(
        egui::pos2(draw.left() + 16.0, draw.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.5),
        vis.strong_text_color(),
    );
    // The shortcut is part of the affordance: without it nobody guesses that
    // Escape also goes back.
    p.text(
        egui::pos2(draw.right() - 10.0, draw.center().y),
        egui::Align2::RIGHT_CENTER,
        "Esc",
        FontId::monospace(10.5),
        vis.weak_text_color(),
    );
    resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, label));
    if resp.clicked() {
        go_back(state);
    }
}

/// Allocates one full-width navigation row; painting happens in a second pass
/// so the sliding pill can sit below every label.
fn nav_row(ui: &mut Ui, text: &str, selected: bool) -> (Rect, egui::Response) {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, NAV_ITEM_H), Sense::click());
    // Self-drawn widgets must report themselves, or screen readers and
    // kittest cannot find the node.
    resp.widget_info(|| WidgetInfo::selected(egui::Role::Tab, true, selected, text));
    (rect, resp)
}

/// Label and hover fill of one row; the selection background is the pill.
fn paint_nav_row(ui: &Ui, clip: Rect, rect: Rect, text: &str, selected: bool, hovered: bool) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter().with_clip_rect(clip);
    // The magnet moves the drawing; the hit rect stays put.
    let draw = rect.translate(magnet_offset(ui, rect, 2.5, 56.0));
    let vis = ui.visuals();
    if hovered && !selected {
        p.rect_filled(draw, CornerRadius::same(13), hover(ui));
    }
    let fg = if selected {
        vis.selection.stroke.color
    } else if hovered {
        vis.strong_text_color()
    } else {
        vis.text_color()
    };
    p.text(
        egui::pos2(draw.left() + 14.0, draw.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::proportional(13.0),
        fg,
    );
}

/// Slides the accent-filled selection block to `to`.
///
/// 160 ms ease-out cubic: WinUI's navigation pill uses 150 ms, and anything
/// past ~250 ms reads as lag rather than motion.
fn draw_pill(ui: &Ui, clip: Rect, state: &mut SettingsState, to: Rect) {
    let now = Instant::now();
    let pill = match state.nav_pill {
        // Target unchanged: keep the running slide.
        Some(p) if p.to == to => p,
        // Target moved: start again from wherever the pill is right now.
        Some(p) => crate::pages::NavPill {
            from: p.cur(now),
            to,
            at: now,
        },
        None => crate::pages::NavPill {
            from: to,
            to,
            at: now,
        },
    };
    let running = pill.running(now);
    ui.painter().with_clip_rect(clip).rect_filled(
        pill.cur(now),
        CornerRadius::same(13),
        ui.visuals().selection.bg_fill.linear_multiply(0.70),
    );
    state.nav_pill = Some(pill);
    if running {
        // A deadline, not an immediate repaint: the next frame is only needed
        // after a display interval, and an immediate request would keep the
        // headless harness from ever settling.
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(rows: usize) -> Vec<Rect> {
        (0..rows)
            .map(|i| Rect::from_min_size(egui::pos2(0.0, i as f32 * 40.0), egui::vec2(100.0, 40.0)))
            .collect()
    }

    #[test]
    fn down_reveal_grows_from_the_top() {
        let col = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 160.0));
        let rows = column(4);
        let early = reveal_clip(&rows, Some((0.125, true)), col);
        assert_eq!(early.max.y, 20.0, "first row half revealed");
        let mid = reveal_clip(&rows, Some((0.5, true)), col);
        assert_eq!(mid.max.y, 80.0, "two rows fully revealed");
        assert_eq!(reveal_clip(&rows, Some((1.0, true)), col), col);
    }

    #[test]
    fn up_reveal_grows_from_the_bottom() {
        let col = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 160.0));
        let rows = column(4);
        let early = reveal_clip(&rows, Some((0.125, false)), col);
        assert_eq!(early.min.y, 140.0, "last row half revealed");
        let mid = reveal_clip(&rows, Some((0.5, false)), col);
        assert_eq!(mid.min.y, 80.0, "two rows fully revealed");
        assert_eq!(reveal_clip(&rows, Some((1.0, false)), col), col);
    }

    #[test]
    fn no_wipe_leaves_the_column_visible() {
        let col = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 160.0));
        assert_eq!(reveal_clip(&column(4), None, col), col);
        assert_eq!(reveal_clip(&[], Some((0.5, true)), col), col);
    }

    #[test]
    fn opening_a_group_starts_a_downward_reveal() {
        let mut state = SettingsState::default();
        apply(NavItem::Group(NavGroup::Look), &mut state, "");
        assert_eq!(state.sub, Some(NavGroup::Look));
        assert!(state.nav_wipe.is_some_and(|w| w.down));
    }

    #[test]
    fn returning_starts_an_upward_reveal() {
        let mut state = SettingsState {
            sub: Some(NavGroup::Look),
            ..SettingsState::default()
        };
        go_back(&mut state);
        assert!(state.sub.is_none());
        assert!(state.nav_wipe.is_some_and(|w| !w.down));
    }
}
