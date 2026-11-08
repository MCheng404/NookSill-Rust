//! Left navigation: flat list, whole row clickable, label text only.

use super::motion::magnet_offset;
use super::*;

/// Flat navigation list: whole row clickable, label text only.
///
/// A non-empty `nav_filter` hides non-matching labels; when the current page is
/// filtered out, the first match is selected so both columns stay in step.
pub(crate) fn sidebar(ui: &mut Ui, state: &mut SettingsState) {
    let filter = state.nav_filter.trim().to_lowercase();
    // Snapshot the page before the loop: a click mutates `state.page`, so
    // reading it while iterating makes "is the old page visible" depend on
    // iteration order, and the auto-jump below would fall back to the first
    // page instead of honouring the click.
    let current = *page_ref(state);
    let mut first_visible: Option<Page> = None;
    let mut current_visible = false;
    let mut clicked: Option<Page> = None;

    for p in Page::ALL {
        if !filter.is_empty() && !p.title().to_lowercase().contains(&filter) {
            continue;
        }
        if first_visible.is_none() {
            first_visible = Some(p);
        }
        if current == p {
            current_visible = true;
        }
        if nav_item(ui, p.title(), current == p).clicked() {
            // Only record; applying it after the loop keeps the click from
            // being overwritten by the auto-jump.
            clicked = Some(p);
            // A bubble's anchor belongs to the old page and would float loose.
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
}

/// Borrow-splitting helper: `nav_item` needs `ui`, the page comes from `state`.
fn page_ref(state: &SettingsState) -> &Page {
    &state.page
}

/// One navigation item: a full-width rounded block, accent-filled when
/// selected.
///
/// Not `selectable_label`, which shrinks to the text width and leaves the list
/// ragged with a hit area no wider than the label.
fn nav_item(ui: &mut Ui, text: &str, selected: bool) -> egui::Response {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, NAV_ITEM_H), egui::Sense::click());

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
            ui.painter()
                .rect_filled(draw, egui::CornerRadius::same(13), bg);
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
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, selected, text));
    resp
}
