//! Icon library page: bulk icon assignment from a folder, plus a preview grid
//! of what each item would pick up.
//!
//! The grid is read-only: it answers "did the name matching find anything",
//! which the old one-line description could not.

use egui::{RichText, Sense, Ui, Vec2};

use super::widgets::{button, card, Ctx};
use super::Command;

/// Cell size of one preview tile.
const TILE: Vec2 = Vec2::new(58.0, 66.0);
/// Rendered icon side inside a tile.
const ICON: f32 = 36.0;

pub(super) fn page(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Apply icons from a folder"),
        "",
        |ctx, ui| {
            ui.horizontal(|ui| {
                button(
                    ctx,
                    ui,
                    nooksill_core::i18n::t("Choose icon folder…"),
                    Command::PickIconDir,
                );
                if !ctx.state.icon_dir.is_empty() {
                    button(
                        ctx,
                        ui,
                        nooksill_core::i18n::t("Match and apply automatically"),
                        Command::AutoMatch,
                    );
                    button(
                        ctx,
                        ui,
                        nooksill_core::i18n::t("Refresh preview"),
                        Command::LoadIconLibrary,
                    );
                }
            });
            if ctx.state.icon_dir.is_empty() {
                ui.label(
                    RichText::new(nooksill_core::i18n::t("No folder selected yet."))
                        .small()
                        .weak(),
                );
            } else {
                ui.label(
                    RichText::new(nooksill_core::i18n::fmt(
                        "Icon folder: {0}",
                        &[ctx.state.icon_dir.to_string().as_str()],
                    ))
                    .small()
                    .weak(),
                );
            }
            ui.label(
                RichText::new(nooksill_core::i18n::t(
                    "Files are matched by name against app names; .ico and common image formats are read.",
                ))
                .small()
                .weak(),
            );
        },
    );

    if ctx.state.icon_dir.is_empty() {
        return;
    }
    preview_card(ctx, ui);
}

/// One tile per item: the icon it would use, or a hollow ring when nothing
/// matched.
fn preview_card(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(
        ctx,
        ui,
        nooksill_core::i18n::t("Icon preview"),
        nooksill_core::i18n::t(
            "What the name matching finds for the current items. Nothing is applied until you press the button above.",
        ),
        |ctx, ui| {
            if ctx.state.icon_library.is_empty() {
                button(
                    ctx,
                    ui,
                    nooksill_core::i18n::t("Load preview"),
                    Command::LoadIconLibrary,
                );
                return;
            }
            // Clone the rows so the grid can borrow `ui` while the state is
            // only read; a texture handle is an id plus an Arc.
            let rows = ctx.state.icon_library.clone();
            let missing = rows.iter().filter(|r| r.tex.is_none()).count();
            ui.horizontal_wrapped(|ui| {
                for row in &rows {
                    ui.allocate_ui(TILE, |ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(ICON), Sense::hover());
                        match &row.tex {
                            Some(tex) => {
                                ui.painter().image(
                                    tex.id(),
                                    rect,
                                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                                    egui::Color32::WHITE,
                                );
                            }
                            None => {
                                ui.painter().circle_stroke(
                                    rect.center(),
                                    ICON * 0.42,
                                    egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
                                );
                            }
                        }
                        ui.label(RichText::new(&row.name).size(10.0).weak());
                    });
                }
            });
            if missing > 0 {
                ui.label(
                    RichText::new(nooksill_core::i18n::fmt(
                        "{0} item(s) have no matching icon file",
                        &[missing.to_string().as_str()],
                    ))
                    .small()
                    .weak(),
                );
            }
        },
    );
}
