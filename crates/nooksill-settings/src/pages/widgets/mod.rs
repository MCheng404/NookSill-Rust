//! Shared widgets and card skeleton for the settings pages.
//!
//! Each row is a title (with an optional info dot) on the left and one control
//! on the right. The control's accessible name is attached to that title with
//! [`egui::Response::labelled_by`], because egui does not adopt a neighbouring
//! label as the name and both screen readers and kittest would miss it.
//!
//! Row descriptions no longer take layout space: they live in the info dot
//! ([`row::hint_button`]) and expand on click or after a 3 s hover.

mod button;
mod card;
mod combo;
mod motion;
mod pickers;
mod row;
mod sidebar;
mod slider;
mod toggle;

#[cfg(test)]
mod tests;

pub(crate) use button::{button, small_button};
pub(crate) use card::card;
pub(crate) use combo::combo;
pub(crate) use pickers::{accent_row, color_row};
pub(crate) use row::{render_tip, row};
pub(crate) use sidebar::sidebar;
pub(crate) use slider::slider;
pub(crate) use toggle::{switch_row, toggle, toggle_derived};

use std::time::Instant;

use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Layout, Response, RichText, Sense, Stroke, Ui,
    WidgetInfo,
};
use nooksill_core::color;
use nooksill_core::config::{Config, Key};

use super::{Command, Outcome, Page, SettingsState};

/// See `NAV_W` at the crate root; the window layer needs it too.
pub(crate) use crate::NAV_W;

/// Height of one navigation item.
pub(crate) const NAV_ITEM_H: f32 = 40.0;

/// Navigation label capacity; extra labels scroll.
pub(crate) const MAX_LABELS: usize = 12;

/// egui sliders claim the whole remaining row width, which squeezes the title
/// out of a left-title/right-control layout, so the width is fixed.
const SLIDER_W: f32 = 200.0;

/// Width reserved for the right-hand control; the title column gets the rest.
///
/// The title wraps rather than pushing the control out of view.
const CTL_RESERVE: f32 = 232.0;

/// Vertical gap between rows.
const ROW_GAP: f32 = 7.0;

/// Capsule switch size, at the Windows 11 Fluent ratio 40x22 with a 14 px knob.
const SWITCH_W: f32 = 40.0;
const SWITCH_H: f32 = 22.0;

/// Control fill shared by buttons, slider tracks and the off state of a switch.
/// Same value as the `text_field` shell: translucent over the card reads as a
/// tonal step in both themes.
fn field_bg(ui: &Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgba_unmultiplied(255, 255, 255, 16)
    } else {
        Color32::from_rgba_unmultiplied(23, 26, 35, 14)
    }
}

/// Linear mix of two colours by `t`, taking `a` at `t = 0`.
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// Gap between cards.
const CARD_GAP: f32 = 14.0;

/// Hover time before a help bubble opens.
const TIP_HOVER_MS: u128 = 3000;

/// Surface colours, copied from the prototype's `--sidebar`, `--card` and
/// `--menu`.
///
/// Not `visuals.panel_fill`: in light mode it is nearly the window fill, so the
/// cards vanish. The window fill is painted by `settings_win::shell`, and a
/// card must sit one step brighter to stay visible; in dark mode the menu is
/// one step brighter than the card so popups read as floating.
mod palette {
    use egui::Color32;

    // Sidebar and card are translucent so the frosted desktop shows through.
    // Opaque surfaces would cover the blur entirely. Alpha is ordered as the
    // Fluent acrylic tonal layers: backdrop < sidebar < card.
    //
    // Alphas are picked for the worst case: a bright wallpaper behind the
    // blur lifts a dark card, so the levels keep text contrast instead of
    // tracking the wallpaper.
    //
    // `from_rgba_unmultiplied` is not const, hence the functions.
    pub(super) fn light_sidebar() -> Color32 {
        Color32::from_rgba_unmultiplied(251, 251, 253, 200)
    }
    pub(super) fn light_card() -> Color32 {
        Color32::from_rgba_unmultiplied(255, 255, 255, 216)
    }
    pub(super) const LIGHT_MENU: Color32 = Color32::from_rgb(255, 255, 255);
    pub(super) fn dark_sidebar() -> Color32 {
        Color32::from_rgba_unmultiplied(28, 29, 34, 190)
    }
    pub(super) fn dark_card() -> Color32 {
        Color32::from_rgba_unmultiplied(33, 34, 40, 215)
    }
    pub(super) const DARK_MENU: Color32 = Color32::from_rgb(38, 39, 47);
}

/// Row hover fill, tinted with the accent so the active palette is visible.
fn row_hover(ui: &Ui) -> Color32 {
    ui.visuals().selection.bg_fill.linear_multiply(0.40)
}

/// Hover fill: a black layer in light mode, a white one in dark mode.
fn hover(ui: &Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgba_unmultiplied(255, 255, 255, 14)
    } else {
        Color32::from_rgba_unmultiplied(23, 26, 35, 12)
    }
}

/// Sidebar fill, layered over the window fill so the blur shows through.
pub(crate) fn sidebar_fill(ui: &Ui) -> Color32 {
    if ui.visuals().dark_mode {
        palette::dark_sidebar()
    } else {
        palette::light_sidebar()
    }
}

/// Mutable state threaded through one frame of a page.
///
/// All three fields are borrows: writing `cfg` edits the config, writing `out`
/// queues a [`Command`]. This layer never performs side effects such as
/// registry writes or dialogs; the host program does.
pub(crate) struct Ctx<'a> {
    pub(crate) cfg: &'a mut Config,
    pub(crate) state: &'a mut SettingsState,
    pub(crate) out: &'a mut Outcome,
}

/// Page header: title plus a one-line description.
pub(crate) fn page_header(ui: &mut Ui, page: Page) {
    ui.add_space(2.0);
    ui.heading(page.title());
    ui.label(RichText::new(page.subtitle()).small().weak());
}
