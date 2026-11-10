//! Row skeleton: title (with hint dot) on the left, control on the right.
//! Also owns the hint dot and the tooltip it expands into.

use super::motion::magnet_offset;
use super::*;

/// One row: title (plus an info dot) on the left, control on the right.
///
/// `note` takes no layout space; it lives in the dot ([`hint_button`]). `add`
/// receives the title's `Id` so the control can call `labelled_by` on it.
pub(crate) fn row<R>(
    ui: &mut Ui,
    state: &mut SettingsState,
    label: &str,
    note: &str,
    add: impl FnOnce(&mut Ui, egui::Id) -> R,
) -> R {
    let note = note.trim();
    let base_min = ui.cursor().min;

    // Two-pass hover band: the previous frame's rect is drawn before this row's
    // content, so it lands above the card fill and below the content. Only the
    // first frame after scrolling or a page change is briefly off.
    let prev_band = state
        .row_bands_prev
        .iter()
        .find(|(p, _)| (*p - base_min).length() < 1.0)
        .map(|(_, r)| *r);
    if let Some(band) = prev_band {
        let hovered = ui
            .ctx()
            .input(|i| i.pointer.hover_pos().is_some_and(|p| band.contains(p)));
        if hovered {
            ui.painter()
                .rect_filled(band, egui::CornerRadius::same(12), row_hover(ui));
        }
    }

    let outer = ui.horizontal(|ui| {
        // Reserve a fixed width for the control; `horizontal` does not wrap, so
        // a long title would otherwise push it out of view.
        let max = (ui.available_width() - CTL_RESERVE).max(120.0);
        let id = ui
            .vertical(|ui| {
                ui.set_max_width(max);
                let mut id = egui::Id::unique("row-label");
                ui.horizontal(|ui| {
                    let resp = ui.label(label);
                    id = resp.id;
                    if !note.is_empty() {
                        hint_button(ui, state, note);
                    }
                });
                id
            })
            .inner;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| add(ui, id))
    });
    // `horizontal` yields `InnerResponse<InnerResponse<R>>`.
    let r = outer.inner.inner;
    // Spacing is explicit; `item_spacing` would also stretch the title/dot gap.
    ui.add_space(ROW_GAP);
    state.row_bands.push((base_min, outer.response.rect));
    r
}

/// Info dot: a 15 px circle with an exclamation mark, accent-tinted on hover.
///
/// A click pins the bubble, a 3 s hover opens it, and leaving closes it unless
/// pinned. Only state changes here; the bubble is drawn by
/// [`crate::pages::settings_ui`] at the end of the frame.
fn hint_button(ui: &mut Ui, state: &mut SettingsState, note: &str) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(15.0, 15.0), egui::Sense::click());
    let draw = rect.translate(magnet_offset(ui, rect, 1.2, 30.0));
    let vis = ui.visuals();
    let open = state.tip.as_ref().is_some_and(|t| t.id == resp.id);

    let (bg, fg) = if open || resp.hovered() {
        (
            vis.selection.bg_fill.linear_multiply(0.70),
            vis.selection.stroke.color,
        )
    } else {
        (hover(ui), vis.weak_text_color())
    };
    // Radius 15 on a 15 px square gives a circle.
    ui.painter().rect_filled(draw, CornerRadius::same(15), bg);
    ui.painter().text(
        draw.center(),
        Align2::CENTER_CENTER,
        "!",
        FontId::proportional(10.0),
        fg,
    );

    // Hover timer: opens once the cursor has rested for 3 s.
    if resp.hovered() {
        if state
            .tip_hover
            .as_ref()
            .is_none_or(|(id, _)| *id != resp.id)
        {
            state.tip_hover = Some((resp.id, Instant::now()));
        } else if let Some((_, at)) = state.tip_hover {
            let elapsed = at.elapsed().as_millis();
            let already = state.tip.as_ref().is_some_and(|t| t.id == resp.id);
            if elapsed >= TIP_HOVER_MS && !already {
                state.tip = Some(crate::pages::Tip {
                    id: resp.id,
                    text: note.to_owned(),
                    anchor: rect,
                    pinned: false,
                });
            }
        }
    } else {
        if state
            .tip_hover
            .as_ref()
            .is_some_and(|(id, _)| *id == resp.id)
        {
            state.tip_hover = None;
        }
        // Unpinned bubbles close when the cursor leaves.
        if state
            .tip
            .as_ref()
            .is_some_and(|t| t.id == resp.id && !t.pinned)
        {
            state.tip = None;
        }
    }

    // Click pins or unpins.
    if resp.clicked() {
        if state
            .tip
            .as_ref()
            .is_some_and(|t| t.id == resp.id && t.pinned)
        {
            state.tip = None;
        } else {
            state.tip = Some(crate::pages::Tip {
                id: resp.id,
                text: note.to_owned(),
                anchor: rect,
                pinned: true,
            });
        }
    }

    // Self-drawn widgets must report themselves or screen readers and kittest
    // cannot find the node.
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            nooksill_core::i18n::fmt("Note: {0}", &[note.to_string().as_str()]),
        )
    });
}

/// The help bubble itself. Rendered at the end of
/// [`crate::pages::settings_ui`], after every row has settled its state.
pub(crate) fn render_tip(ui: &mut Ui, state: &mut SettingsState) {
    let Some(tip) = state.tip.clone() else { return };
    let mag = magnet_offset(ui, tip.anchor, 1.2, 30.0);
    // Below the dot by default; flips above when there is no room. The `pivot`
    // does the flip without knowing the bubble height up front.
    let below = tip.anchor.bottom() + 100.0 <= ui.max_rect().bottom();
    let (pivot, pos) = if below {
        (
            Align2::LEFT_TOP,
            egui::pos2(
                (tip.anchor.left() - 6.0).max(8.0),
                tip.anchor.bottom() + 8.0,
            ),
        )
    } else {
        (
            Align2::LEFT_BOTTOM,
            egui::pos2((tip.anchor.left() - 6.0).max(8.0), tip.anchor.top() - 8.0),
        )
    };
    // The bubble follows the magnet pull too.
    let pos = pos + mag;

    let rect = std::cell::Cell::new(egui::Rect::NOTHING);
    let area_id = egui::Id::unique("settings-tip");
    egui::Area::new(area_id)
        .order(egui::Order::Tooltip)
        .pivot(pivot)
        .fixed_pos(pos)
        .show(ui.ctx(), |ui| {
            let dark = ui.visuals().dark_mode;
            let frame = egui::Frame::new()
                .fill(if dark {
                    palette::DARK_MENU
                } else {
                    palette::LIGHT_MENU
                })
                .corner_radius(egui::CornerRadius::same(12))
                .inner_margin(egui::Margin::symmetric(12, 10))
                // `spread` must stay 0, as on cards: it grows the shadow rect
                // but not the radius, leaking a rim through the AA edge.
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 20,
                    spread: 0,
                    color: Color32::from_black_alpha(if dark { 90 } else { 24 }),
                });
            let resp = frame.show(ui, |ui| {
                ui.set_max_width(240.0);
                ui.label(
                    RichText::new(&tip.text)
                        .size(11.5)
                        .color(ui.visuals().weak_text_color()),
                );
            });
            rect.set(resp.response.rect);
        });

    // A click outside closes the bubble even when pinned. The click that opens
    // it cannot close it, since the pointer is still on the anchor.
    let clicked_outside = ui.ctx().input(|i| {
        i.pointer.any_click()
            && i.pointer
                .interact_pos()
                .is_some_and(|p| !tip.anchor.expand(6.0).contains(p) && !rect.get().contains(p))
    });
    if clicked_outside {
        state.tip = None;
        state.tip_hover = None;
    }
}
