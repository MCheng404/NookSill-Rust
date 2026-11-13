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
    let current = state.page;
    let mut first_page: Option<Page> = None;
    let mut current_visible = false;
    let mut clicked: Option<NavItem> = None;

    for item in NavItem::PRIMARY {
        if !item.matches(&filter) {
            continue;
        }
        // Only a plain page can be auto-selected: jumping into a group on a
        // search keystroke would replace the whole column unexpectedly.
        if let NavItem::Page(p) = item {
            if first_page.is_none() {
                first_page = Some(p);
            }
        }
        let selected = match item {
            NavItem::Page(p) => p == current,
            NavItem::Group(g) => g.tabs().contains(&current),
        };
        if selected {
            current_visible = true;
        }
        if nav_item(ui, item.label(), selected).clicked() {
            // Applying after the loop keeps the click from being overwritten
            // by the auto-jump below.
            clicked = Some(item);
            state.tip = None;
            state.tip_hover = None;
        }
    }

    if let Some(item) = clicked {
        apply(item, state, &filter);
    } else if !current_visible {
        if let Some(p) = first_page {
            state.page = p;
            state.sub = None;
        }
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
    ui.add_space(2.0);
    ui.label(egui::RichText::new(group.label()).small().weak());
    ui.add_space(6.0);

    let current = state.page;
    let mut first_visible: Option<Page> = None;
    let mut current_visible = false;
    let mut clicked: Option<Page> = None;
    for p in group.tabs() {
        if !filter.is_empty() && !p.title().to_lowercase().contains(&filter) {
            continue;
        }
        if first_visible.is_none() {
            first_visible = Some(*p);
        }
        if current == *p {
            current_visible = true;
        }
        if nav_item(ui, p.title(), current == *p).clicked() {
            clicked = Some(*p);
            state.tip = None;
            state.tip_hover = None;
        }
    }
    if let Some(p) = clicked {
        state.page = p;
    } else if !current_visible {
        if let Some(p) = first_visible {
            state.page = p;
        }
    }

    back_row(ui, state);
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
    if resp.hovered() {
        row.painter()
            .rect_filled(draw, CornerRadius::same(13), hover(&row));
    }
    row.painter().text(
        egui::pos2(draw.left() + 14.0, draw.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        row.visuals().weak_text_color(),
    );
    resp.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, label));
    if resp.clicked() {
        state.page = state.return_page;
        state.sub = None;
        state.tip = None;
        state.tip_hover = None;
    }
}

/// One navigation item: a full-width rounded block, accent-filled when
/// selected.
///
/// Not `selectable_label`, which shrinks to the text width and leaves the list
/// ragged with a hit area no wider than the label.
fn nav_item(ui: &mut Ui, text: &str, selected: bool) -> egui::Response {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, NAV_ITEM_H), Sense::click());

    // The magnet moves the drawing; the hit rect stays put.
    let draw = rect.translate(magnet_offset(ui, rect, 2.5, 56.0));

    if ui.is_rect_visible(rect) {
        let vis = ui.visuals();
        let (bg, fg) = if selected {
            (
                vis.selection.bg_fill.linear_multiply(0.70),
                vis.selection.stroke.color,
            )
        } else if resp.hovered() {
            (hover(ui), vis.strong_text_color())
        } else {
            (Color32::TRANSPARENT, vis.text_color())
        };
        if bg != Color32::TRANSPARENT {
            ui.painter().rect_filled(draw, CornerRadius::same(13), bg);
        }
        ui.painter().text(
            egui::pos2(draw.left() + 14.0, draw.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            FontId::proportional(13.0),
            fg,
        );
    }

    // Self-drawn widgets must report themselves, or screen readers and
    // kittest cannot find the node.
    resp.widget_info(|| WidgetInfo::selected(egui::Role::Tab, true, selected, text));
    resp
}
