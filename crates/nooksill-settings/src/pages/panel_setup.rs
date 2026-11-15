//! Widget panel settings: placement, per-panel overrides and background.

use egui::Ui;
use nooksill_core::config::panel::{override_get, override_set, PanelKind, Placement};
use nooksill_core::config::Key;
use nooksill_core::i18n::t;

use super::options::{PANEL_OVERRIDE_OPTS, PANEL_POS_OPTS};
use super::widgets::{card, combo, row, toggle, Ctx};

/// Panels whose placement can be set independently.
const ROWS: [(PanelKind, &str); 6] = [
    (PanelKind::Calendar, "Calendar"),
    (PanelKind::ControlCenter, "Control Center"),
    (PanelKind::Resource, "Resource Monitor"),
    (PanelKind::Tool, "Timer / To-do"),
    (PanelKind::Notes, "Notes"),
    (PanelKind::Group, "Group"),
];

/// Combo code to the packed override value; `0` inherits the global policy.
fn placement_of(code: i32) -> Option<Placement> {
    match code {
        1 => Some(Placement::FollowWidget),
        2 => Some(Placement::ScreenCenter),
        3 => Some(Placement::Auto),
        _ => None,
    }
}

fn code_of(value: Option<Placement>) -> i32 {
    value.map_or(0, |p| p.code() + 1)
}

pub(super) fn page(ctx: &mut Ctx<'_>, ui: &mut Ui) {
    card(ctx, ui, t("Widget panels"), "", |ctx, ui| {
        toggle(
            ctx,
            ui,
            Key::PRELOAD_PANELS,
            t("Preload widget panels"),
            t("Preload widget panels at startup to avoid the first-click delay"),
        );
        combo(ctx, ui, Key::PANEL_POS, t("Panel position"), PANEL_POS_OPTS);
        for (kind, label) in ROWS {
            override_row(ctx, ui, kind, t(label));
        }
    });
}

fn override_row(ctx: &mut Ctx<'_>, ui: &mut Ui, kind: PanelKind, label: &str) {
    let packed = ctx.cfg.get_i32(Key::PANEL_POS_OVERRIDE);
    let before = code_of(override_get(packed, kind));
    let mut cur = before;
    // The salt is a stable index, not the translated label.
    let salt = format!("panel-override-{}", kind.index());
    row(ui, ctx.state, label, "", |ui, id| {
        let text = PANEL_OVERRIDE_OPTS
            .iter()
            .find(|(v, _)| *v == cur)
            .map_or("—", |(_, text)| t(text));
        egui::ComboBox::from_id_salt(salt.clone())
            .selected_text(text)
            .width(180.0)
            .show_ui(ui, |ui| {
                for (v, text) in PANEL_OVERRIDE_OPTS {
                    ui.selectable_value(&mut cur, *v, t(text));
                }
            })
            .response
            .labelled_by(id);
    });
    if cur != before {
        ctx.cfg.apply_i32(
            Key::PANEL_POS_OVERRIDE,
            override_set(packed, kind, placement_of(cur)),
        );
        ctx.out.changed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combo_codes_round_trip_every_placement() {
        assert_eq!(code_of(None), 0);
        assert_eq!(placement_of(0), None);
        for p in [
            Placement::FollowWidget,
            Placement::ScreenCenter,
            Placement::Auto,
        ] {
            assert_eq!(placement_of(code_of(Some(p))), Some(p));
        }
        assert_eq!(placement_of(9), None);
    }

    #[test]
    fn every_row_has_a_distinct_kind() {
        for (i, (kind, _)) in ROWS.iter().enumerate() {
            assert_eq!(kind.index(), i);
            assert!(kind.override_slot().is_some());
        }
        assert_eq!(ROWS.len(), 6, "the modal editor is not a widget panel");
    }
}
