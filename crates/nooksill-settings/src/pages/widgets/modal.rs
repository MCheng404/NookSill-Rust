//! Generic rounded modal: softens the whole settings surface, then draws a
//! centered card. Actions only queue a `Command`; no side effects happen here.

use egui::{Rect, Vec2};

use super::*;

/// One button of a modal. `None` means "close without an action".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModalAction {
    pub label: String,
    pub command: Option<Command>,
    pub primary: bool,
}

impl ModalAction {
    #[must_use]
    pub fn close(label: &str) -> Self {
        Self {
            label: label.to_owned(),
            command: None,
            primary: false,
        }
    }

    #[must_use]
    pub fn primary(label: &str, command: Command) -> Self {
        Self {
            label: label.to_owned(),
            command: Some(command),
            primary: true,
        }
    }
}

/// Content of the single modal shown at a time.
#[derive(Clone, Debug)]
pub struct Modal {
    pub title: String,
    pub body: String,
    pub detail: String,
    pub actions: Vec<ModalAction>,
    pub dismissible: bool,
    /// When set, a "do not show again" box writes 0 to this config key.
    pub suppress_key: Option<Key>,
}

impl Modal {
    #[must_use]
    pub fn new(title: &str, body: &str) -> Self {
        Self {
            title: title.to_owned(),
            body: body.to_owned(),
            detail: String::new(),
            actions: Vec::new(),
            dismissible: true,
            suppress_key: None,
        }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: &str) -> Self {
        self.detail = detail.to_owned();
        self
    }
}

/// Draw the overlay and the card; returns after clearing `state.modal` when a
/// button, the close icon or Escape dismissed it.
pub(crate) fn render_modal(
    ui: &mut Ui,
    cfg: &mut Config,
    state: &mut SettingsState,
    out: &mut Outcome,
) {
    let Some(modal) = state.modal.clone() else {
        return;
    };
    let rect = ui.max_rect();
    if rect.width() < 1.0 || rect.height() < 1.0 {
        return;
    }
    let dark = ui.visuals().dark_mode;

    // Allocated before the card, so the card's widgets win the hit test where
    // they overlap and the rest of the surface closes the modal.
    let (_, veil) = ui.allocate_exact_size(rect.size(), Sense::click());
    let painter = ui.painter();
    soften(painter, rect, dark);

    let w = rect
        .width()
        .clamp(300.0, 470.0)
        .min(rect.width() - 24.0)
        .max(240.0);
    // A short card has no detail line; keeping the height tied to the content
    // avoids a large empty band under the actions.
    let wanted: f32 = if modal.detail.is_empty() {
        184.0
    } else {
        206.0
    };
    let h = wanted.min(rect.height() - 24.0).max(150.0);
    let panel = Rect::from_center_size(rect.center(), Vec2::new(w, h));
    let radius = CornerRadius::same(22);
    painter.rect_filled(
        panel.translate(Vec2::new(0.0, 10.0)),
        radius,
        Color32::from_black_alpha(if dark { 110 } else { 40 }),
    );
    painter.rect_filled(
        panel,
        radius,
        if dark {
            palette::DARK_MENU
        } else {
            palette::LIGHT_MENU
        },
    );
    painter.rect_stroke(
        panel,
        radius,
        Stroke::new(
            1.0,
            if dark {
                Color32::from_rgba_unmultiplied(255, 255, 255, 26)
            } else {
                Color32::from_rgba_unmultiplied(23, 26, 35, 18)
            },
        ),
        egui::StrokeKind::Inside,
    );

    let inner = panel.shrink2(Vec2::new(24.0, 20.0));
    let mut card = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );
    card.label(RichText::new(&modal.title).size(18.0).strong());
    card.add_space(8.0);
    card.label(RichText::new(&modal.body).size(13.0));
    if !modal.detail.is_empty() {
        card.add_space(6.0);
        card.label(
            RichText::new(&modal.detail)
                .size(11.5)
                .monospace()
                .color(card.visuals().weak_text_color()),
        );
    }
    card.add_space(6.0);

    if let Some(key) = modal.suppress_key {
        let mut off = cfg.get_i32(key) == 0;
        if card
            .checkbox(&mut off, nooksill_core::i18n::t("Do not show this again"))
            .changed()
        {
            cfg.apply_i32(key, i32::from(!off));
            out.changed = true;
        }
    }

    // Bottom-up pins the actions to the lower edge; right-to-left puts the
    // primary action on the right, matching the window's close button.
    card.with_layout(Layout::bottom_up(Align::Min), |ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            for action in &modal.actions {
                let btn = egui::Button::new(RichText::new(&action.label).size(12.5))
                    .corner_radius(CornerRadius::same(10))
                    .min_size(Vec2::new(96.0, 30.0));
                let btn = if action.primary {
                    btn.fill(ui.visuals().selection.bg_fill)
                } else {
                    btn.fill(field_bg(ui))
                };
                if ui.add(btn).clicked() {
                    if let Some(cmd) = &action.command {
                        out.command = Some(cmd.clone());
                    }
                    state.modal = None;
                }
            }
        });
    });

    if modal.dismissible {
        let x = Rect::from_center_size(
            egui::pos2(panel.right() - 20.0, panel.top() + 20.0),
            Vec2::splat(22.0),
        );
        let close = ui.allocate_rect(x, Sense::click());
        let p = ui.painter();
        let c = x.center();
        let d = 4.5;
        let s = Stroke::new(1.5, ui.visuals().weak_text_color());
        p.line_segment(
            [egui::pos2(c.x - d, c.y - d), egui::pos2(c.x + d, c.y + d)],
            s,
        );
        p.line_segment(
            [egui::pos2(c.x + d, c.y - d), egui::pos2(c.x - d, c.y + d)],
            s,
        );
        close.widget_info(|| {
            WidgetInfo::labeled(egui::Role::Button, true, nooksill_core::i18n::t("Close"))
        });
        // The veil covers the whole window, including the panel's empty areas.
        // Only a click outside the panel counts as "click away"; otherwise a
        // click on the title text would dismiss the modal.
        let outside = veil.clicked()
            && ui
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| !panel.contains(p));
        if close.clicked() || outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.modal = None;
        }
    }
}

/// A translucent veil plus defocused highlights: the settings surface loses
/// contrast and edge sharpness while the modal is up.
fn soften(painter: &egui::Painter, rect: Rect, dark: bool) {
    let tint = if dark {
        Color32::from_rgba_unmultiplied(14, 16, 22, 128)
    } else {
        Color32::from_rgba_unmultiplied(236, 239, 246, 132)
    };
    painter.rect_filled(rect, CornerRadius::same(20), tint);
    let glow = if dark {
        (120, 140, 210)
    } else {
        (255, 255, 255)
    };
    for (fx, fy, r) in [(0.28, 0.24, 0.34), (0.76, 0.70, 0.28)] {
        let center = rect.min + Vec2::new(rect.width() * fx, rect.height() * fy);
        let base = rect.width().min(rect.height()) * r;
        for step in (1..=5).rev() {
            let t = step as f32 / 5.0;
            painter.circle_filled(
                center,
                base * t,
                Color32::from_rgba_unmultiplied(glow.0, glow.1, glow.2, 4),
            );
        }
    }
    painter.rect_filled(
        rect,
        CornerRadius::same(20),
        Color32::from_rgba_unmultiplied(255, 255, 255, if dark { 6 } else { 18 }),
    );
}
