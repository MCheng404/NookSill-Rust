//! Left navigation: primary entries, or one group's content tabs with a back
//! row pinned to the bottom.

use egui::{Color32, CornerRadius, FontId, Rect, Sense, Ui, Vec2, WidgetInfo};

use crate::pages::nav::{NavGroup, NavItem};

use super::motion::magnet_offset;
use super::*;

/// Dispatch on whether a group is open. The filter applies to the labels
/// currently on screen, so a buried page is reachable by first selecting its
/// group (primary rows match on their tab labels too).
pub(crate) fn sidebar(ui: &mut Ui, state: &mut SettingsState) {
    match state.sub {
        Some(group) => tabs(ui, state, group),
        None => primary(ui, state),
    }
}

fn primary(ui: &mut Ui, state: &mut SettingsState) {
    let filter = state.nav_filter.trim().to_lowercase();
    let mut rows: Vec<(NavItem, Rect, egui::Response)> = Vec::new();
    for item in NavItem::PRIMARY {
        if !item.matches(&filter) {
            continue;
        }
        let (rect, resp) = nav_row(ui, item.label(), nav_selected(item, state.page));
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

    let selected = rows
        .iter()
        .find(|(item, _, _)| nav_selected(*item, state.page))
        .map(|(_, rect, _)| *rect);
    match selected {
        Some(to) => draw_pill(ui, state, to),
        None => state.nav_pill = None,
    }
    for (item, rect, resp) in &rows {
        paint_nav_row(
            ui,
            *rect,
            item.label(),
            nav_selected(*item, state.page),
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
            state.page = p;
            state.sub = None;
        }
    }
}

/// Whether `item` owns `page`.
fn nav_selected(item: NavItem, page: Page) -> bool {
    match item {
        NavItem::Page(p) => p == page,
        NavItem::Group(g) => g.tabs().contains(&page),
    }
}

fn apply(item: NavItem, state: &mut SettingsState, filter: &str) {
    match item {
        NavItem::Page(p) => {
            state.page = p;
            state.sub = None;
        }
        NavItem::Group(g) => {
            state.return_page = state.page;
            state.sub = Some(g);
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

fn tabs(ui: &mut Ui, state: &mut SettingsState, group: NavGroup) {
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
    let selected = rows
        .iter()
        .find(|(p, _, _)| *p == state.page)
        .map(|(_, rect, _)| *rect);
    match selected {
        Some(to) => draw_pill(ui, state, to),
        None => state.nav_pill = None,
    }
    for (p, rect, resp) in &rows {
        paint_nav_row(ui, *rect, p.title(), *p == state.page, resp.hovered());
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

    back_row(ui, state);
}

/// Returns to the page the group was opened from.
fn go_back(state: &mut SettingsState) {
    state.page = state.return_page;
    state.sub = None;
    state.tip = None;
    state.tip_hover = None;
}

/// Bottom-pinned return row; it stays visible while the tab strip scrolls.
fn back_row(ui: &mut Ui, state: &mut SettingsState) {
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
    // Always-on accent fill plus a left bar: a hover-only row at the bottom of
    // a long tab list was too easy to miss, and this is the only way out.
    let alpha = if resp.hovered() { 46 } else { 26 };
    row.painter().rect_filled(
        draw,
        CornerRadius::same(13),
        Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), alpha),
    );
    row.painter().rect_filled(
        Rect::from_min_size(
            egui::pos2(draw.left() + 5.0, draw.top() + 7.0),
            Vec2::new(3.0, (draw.height() - 14.0).max(1.0)),
        ),
        CornerRadius::same(2),
        accent,
    );
    row.painter().text(
        egui::pos2(draw.left() + 16.0, draw.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.5),
        vis.strong_text_color(),
    );
    // The shortcut is part of the affordance: without it nobody guesses that
    // Escape also goes back.
    row.painter().text(
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
fn paint_nav_row(ui: &Ui, rect: Rect, text: &str, selected: bool, hovered: bool) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    // The magnet moves the drawing; the hit rect stays put.
    let draw = rect.translate(magnet_offset(ui, rect, 2.5, 56.0));
    let vis = ui.visuals();
    if hovered && !selected {
        ui.painter()
            .rect_filled(draw, CornerRadius::same(13), hover(ui));
    }
    let fg = if selected {
        vis.selection.stroke.color
    } else if hovered {
        vis.strong_text_color()
    } else {
        vis.text_color()
    };
    ui.painter().text(
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
fn draw_pill(ui: &Ui, state: &mut SettingsState, to: Rect) {
    let now = std::time::Instant::now();
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
    ui.painter().rect_filled(
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
