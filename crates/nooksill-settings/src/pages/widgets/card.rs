//! Card frame and card skeleton: a page is a stack of `card()` calls.

use super::motion::magnet_offset;
use super::*;

/// Card frame: translucent fill, large radius, soft shadow, 1 px hairline.
///
/// A translucent fill lets the blur through but dissolves the edge, so the
/// stroke draws the boundary while the shadow keeps the card floating.
fn card_frame(ui: &Ui) -> egui::Frame {
    let dark = ui.visuals().dark_mode;
    egui::Frame::new()
        .fill(if dark {
            palette::dark_card()
        } else {
            palette::light_card()
        })
        .stroke(egui::Stroke::new(
            1.0,
            if dark {
                Color32::from_rgba_unmultiplied(255, 255, 255, 20)
            } else {
                Color32::from_rgba_unmultiplied(23, 26, 35, 16)
            },
        ))
        .corner_radius(egui::CornerRadius::same(18))
        .inner_margin(egui::Margin::symmetric(16, 12))
        // `spread` must stay 0: it grows the shadow rect without growing the
        // radius, so a dark rim leaks through the antialiased corner arc.
        // Density comes from alpha and blur instead.
        .shadow(egui::Shadow {
            offset: [0, 8],
            blur: 22,
            spread: 0,
            color: Color32::from_black_alpha(if dark { 90 } else { 16 }),
        })
}

/// One card: title, optional note, then the content closure.
pub(crate) fn card<R>(
    ctx: &mut Ctx<'_>,
    ui: &mut Ui,
    title: &str,
    note: &str,
    add: impl FnOnce(&mut Ctx<'_>, &mut Ui) -> R,
) -> R {
    // Magnet pull moves the whole card, controls included, because they are
    // laid out inside the offset child UI. Decay uses the height measured last
    // frame, with an estimate of 300 on the first frame.
    let avail_w = ui.available_width();
    let base_min = ui.cursor().min;
    let prev_h = ctx
        .state
        .card_h_prev
        .iter()
        .find(|(p, _)| (*p - base_min).length() < 1.0)
        .map(|(_, h)| *h);
    let base = egui::Rect::from_min_size(base_min, egui::vec2(avail_w, prev_h.unwrap_or(300.0)));
    let off = magnet_offset(ui, base, 3.5, 90.0);

    // `new_child` does not advance the parent cursor, so the real height is
    // pushed afterwards; the 3 px magnet offset must not enter layout or it
    // would accumulate card by card.
    let mut card_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_size(
                base_min + off,
                egui::vec2(avail_w, ui.available_height()),
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    // Cards must always span the full width: a `Frame` shrinks to its content,
    // so a card holding one short label would collapse into a small block.
    let frame = card_frame(&card_ui);
    let inner = frame
        .show(&mut card_ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.strong(title);
            if !note.is_empty() {
                ui.label(RichText::new(note).small().weak());
            }
            ui.add_space(6.0);
            add(ctx, ui)
        })
        .inner;

    ctx.state
        .card_h
        .push((base_min, card_ui.min_rect().height()));
    ui.advance_cursor_after_rect(egui::Rect::from_min_size(
        base_min,
        egui::vec2(avail_w, card_ui.min_rect().height()),
    ));
    // Spacing between cards is explicit; `item_spacing` would also change the
    // gaps inside a card.
    ui.add_space(CARD_GAP);
    inner
}
