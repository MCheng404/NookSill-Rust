//! Notes panel painting: row layout, buttons, paging and text fitting.

use super::{NoteAction, BTN_H, BTN_W, FOOTER_H, HEADER_H, PAD, PAGE_SIZE, ROW_H, W};
use crate::widgets::style::PanelStyle;
use egui::{Align2, Color32, CornerRadius, FontId, Rect as EguiRect, Stroke};
use nooksill_core::note::{Note, NoteBook};

/// Rect of row `k` in the window (logical coords).
pub(super) fn row_rect(k: usize) -> EguiRect {
    EguiRect::from_min_size(
        egui::pos2(PAD, PAD + HEADER_H + k as f32 * ROW_H),
        egui::vec2(W - PAD * 2.0, ROW_H - 4.0),
    )
}

/// Which row is hit (footer buttons excluded).
pub(super) fn row_at(x: f32, y: f32, total: usize, page: usize) -> Option<usize> {
    let start = page * PAGE_SIZE;
    let shown = total.saturating_sub(start).min(PAGE_SIZE);
    (0..shown).find(|&k| row_rect(k).contains(egui::pos2(x, y)))
}

/// Paints the panel. Returns the action of this frame.
pub(super) fn paint(
    ui: &mut egui::Ui,
    dark: bool,
    frosted: bool,
    notes: &[Note],
    page: usize,
    pages: usize,
    hover: Option<usize>,
) -> Option<NoteAction> {
    let rect = ui.max_rect();
    // Same base fill, overlay and text tiers as the other four panels.
    let style = PanelStyle::current(dark, frosted);
    let (bg, ink, faint, border, field) = palette(&style);
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(PanelStyle::R_PANEL), bg);
    painter.rect_filled(
        rect,
        CornerRadius::same(PanelStyle::R_PANEL),
        style.overlay(),
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(PanelStyle::R_PANEL),
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    painter.text(
        egui::pos2(rect.left() + PAD, rect.top() + PAD + 2.0),
        Align2::LEFT_TOP,
        nooksill_core::i18n::t("Notes"),
        FontId::proportional(15.0),
        ink,
    );

    let mut action = None;
    // "New" button, top right of the header.
    let new_rect = EguiRect::from_min_size(
        egui::pos2(rect.right() - PAD - BTN_W, rect.top() + PAD),
        egui::vec2(BTN_W, BTN_H),
    );
    let new_resp = ui.interact(new_rect, egui::Id::unique("note-new"), egui::Sense::click());
    painter.rect_filled(new_rect, CornerRadius::same(PanelStyle::R_SMALL), field);
    painter.text(
        new_rect.center(),
        Align2::CENTER_CENTER,
        nooksill_core::i18n::t("＋ New"),
        FontId::proportional(12.0),
        ink,
    );
    if new_resp.clicked() {
        action = Some(NoteAction::New);
    }

    // List.
    let start = page * PAGE_SIZE;
    let shown = notes.len().saturating_sub(start).min(PAGE_SIZE);
    if shown == 0 {
        painter.text(
            egui::pos2(rect.center().x, rect.top() + HEADER_H + 80.0),
            Align2::CENTER_CENTER,
            nooksill_core::i18n::t("No notes yet; click ＋ New in the top right"),
            FontId::proportional(13.0),
            faint,
        );
    }
    for k in 0..shown {
        let note = &notes[start + k];
        let r = row_rect(k);
        if hover == Some(k) {
            painter.rect_filled(r, CornerRadius::same(PanelStyle::R_CARD), field);
        }
        let row_resp = ui.interact(r, egui::Id::unique(("note-row", k)), egui::Sense::click());
        painter.text(
            egui::pos2(r.left() + 10.0, r.top() + 8.0),
            Align2::LEFT_TOP,
            fit_text(painter, &note.display_title(), r.width() - 140.0, 13.0),
            FontId::proportional(13.0),
            ink,
        );
        painter.text(
            egui::pos2(r.left() + 10.0, r.top() + 30.0),
            Align2::LEFT_TOP,
            fit_text(painter, &note.preview(), r.width() - 140.0, 11.0),
            FontId::proportional(11.0),
            faint,
        );

        // In-row buttons. Registered after the whole row, so egui's hit
        // order lets the buttons win.
        let del_rect = EguiRect::from_min_size(
            egui::pos2(r.right() - BTN_W - 6.0, r.center().y - BTN_H / 2.0),
            egui::vec2(BTN_W, BTN_H),
        );
        let edit_rect = EguiRect::from_min_size(
            egui::pos2(del_rect.left() - BTN_W - 6.0, del_rect.top()),
            egui::vec2(BTN_W, BTN_H),
        );
        for (br, label, act) in [
            (
                edit_rect,
                nooksill_core::i18n::t("Edit"),
                NoteAction::Edit(start + k),
            ),
            (
                del_rect,
                nooksill_core::i18n::t("Delete"),
                NoteAction::Remove(start + k),
            ),
        ] {
            let resp = ui.interact(
                br,
                egui::Id::unique(("note-btn", k, label)),
                egui::Sense::click(),
            );
            painter.rect_filled(br, PanelStyle::pill(br.height()), field);
            painter.text(
                br.center(),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(11.5),
                if label == nooksill_core::i18n::t("Delete") {
                    Color32::from_rgb(230, 110, 110)
                } else {
                    ink
                },
            );
            if resp.clicked() {
                action = Some(act);
            }
        }
        // When no button was hit, clicking the row means edit.
        if row_resp.clicked() && action.is_none() {
            action = Some(NoteAction::Edit(start + k));
        }
    }

    // Footer: paging and the item count.
    let fy = rect.bottom() - PAD - FOOTER_H;
    painter.text(
        egui::pos2(rect.left() + PAD, fy + FOOTER_H / 2.0),
        Align2::LEFT_CENTER,
        nooksill_core::i18n::fmt(
            "{0} notes · page {1} / {2}",
            &[
                notes.len().to_string().as_str(),
                (page + 1).to_string().as_str(),
                pages.to_string().as_str(),
            ],
        ),
        FontId::proportional(11.0),
        faint,
    );
    let prev_rect = EguiRect::from_min_size(
        egui::pos2(rect.right() - PAD - BTN_W * 2.0 - 8.0, fy + 5.0),
        egui::vec2(BTN_W, BTN_H),
    );
    let next_rect = EguiRect::from_min_size(
        egui::pos2(rect.right() - PAD - BTN_W, fy + 5.0),
        egui::vec2(BTN_W, BTN_H),
    );
    let prev = ui.interact(
        prev_rect,
        egui::Id::unique("note-prev"),
        egui::Sense::click(),
    );
    let next = ui.interact(
        next_rect,
        egui::Id::unique("note-next"),
        egui::Sense::click(),
    );
    for (r, label, enabled) in [
        (prev_rect, nooksill_core::i18n::t("Previous"), page > 0),
        (next_rect, nooksill_core::i18n::t("Next"), page + 1 < pages),
    ] {
        painter.rect_filled(r, CornerRadius::same(PanelStyle::R_SMALL), field);
        painter.text(
            r.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(11.0),
            if enabled { ink } else { faint },
        );
    }
    if prev.clicked() && page > 0 {
        return Some(NoteAction::Page(-1));
    }
    if next.clicked() && page + 1 < pages {
        return Some(NoteAction::Page(1));
    }
    action
}

/// Total number of pages. Pure function so it can be tested without a window.
pub(super) fn page_count(total: usize) -> usize {
    total.div_ceil(PAGE_SIZE).max(1)
}

/// Offscreen entry point (`examples/panels_shot.rs`).
///
/// Screenshots must use the real window's painting code; a hand-built
/// approximation would not catch anything. This builds no window and touches
/// no Win32 API, it only paints the panel into the caller's `Ui`.
pub fn paint_preview(
    ui: &mut egui::Ui,
    dark: bool,
    frosted: bool,
    book: &NoteBook,
    page: usize,
    hover: Option<usize>,
) {
    let _ = paint(
        ui,
        dark,
        frosted,
        &book.notes,
        page,
        page_count(book.notes.len()),
        hover,
    );
}

fn palette(style: &PanelStyle) -> (Color32, Color32, Color32, Color32, Color32) {
    // Keeps five semantic names so the drawing code below stays unchanged;
    // all values come from the shared style.
    (
        style.bg(),
        style.ink(),
        style.ink_sub(),
        style.border(),
        style.card(),
    )
}

/// Truncates overlong text and appends an ellipsis.
fn fit_text(painter: &egui::Painter, text: &str, max_w: f32, size: f32) -> String {
    let font = FontId::proportional(size);
    if painter
        .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x
        <= max_w
    {
        return text.to_owned();
    }
    let mut out: Vec<char> = text.chars().collect();
    while !out.is_empty() {
        out.pop();
        let s: String = out.iter().collect();
        if painter
            .layout_no_wrap(format!("{s}…"), font.clone(), Color32::WHITE)
            .size()
            .x
            <= max_w
        {
            return format!("{s}…");
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_laid_out_sequentially() {
        let a = row_rect(0);
        let b = row_rect(1);
        assert!(b.top() > a.top());
        assert_eq!(a.left(), b.left());
        assert_eq!(a.width(), b.width());
    }

    #[test]
    fn row_at_matches_the_visible_page() {
        let a = row_rect(0).center();
        assert_eq!(row_at(a.x, a.y, 3, 0), Some(0));
        let b = row_rect(1).center();
        assert_eq!(row_at(b.x, b.y, 3, 0), Some(1));
        // Row 0 of page 2 is also index 0 within the page; the global index
        // is `page * PAGE_SIZE` plus it.
        assert_eq!(row_at(a.x, a.y, 10, 1), Some(0));
        assert_eq!(row_at(5.0, 5.0, 3, 0), None, "the title bar is not a row");
    }

    #[test]
    fn page_count_has_a_floor_of_one() {
        assert_eq!(page_count(0), 1);
        assert_eq!(page_count(1), 1);
        assert_eq!(page_count(PAGE_SIZE), 1);
        assert_eq!(page_count(PAGE_SIZE + 1), 2);
    }

    #[test]
    fn max_notes_divides_into_pages() {
        assert_eq!(page_count(nooksill_core::note::MAX_NOTES), 6);
    }
}
