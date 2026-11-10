//! The settings shell UI and its frost variant.

use super::*;

/// Draws the window shell: background, title bar and close button, leaving
/// the middle to `settings_ui`.
///
/// The shell lives here rather than in `pages.rs`: the kittest there walks
/// every widget, so one more button means several assertion updates, while
/// "does the settings window have a close button" is a window-level question
/// with the same answer for every page.
///
/// Public because the offscreen screenshot (`examples/settings_shot.rs`) must
/// draw the same shell; a look-alike would not represent the real window.
pub fn shell(ui: &mut egui::Ui, state: &mut SettingsState, cfg: &mut Config, out: &mut Outcome) {
    shell_with_frost(ui, state, cfg, out, false);
}

/// [`shell`] with the frosted-glass variant.
///
/// The offscreen screenshot uses the plain shell (there is no desktop to
/// capture), the real window uses this one; both share a single shell
/// implementation so the screenshot matches the real window.
pub fn shell_with_frost(
    ui: &mut egui::Ui,
    state: &mut SettingsState,
    cfg: &mut Config,
    out: &mut Outcome,
    frost: bool,
) {
    // The accent color must reach `Visuals` before any child `Ui` is built:
    // children inherit the style they were created with.
    crate::accent::apply_visuals(ui, cfg);
    let rect = ui.max_rect();
    let dark = ui.visuals().dark_mode;
    // 250/255 leaves the rounded edge slightly transparent so it melts into
    // the desktop without letting the wallpaper interfere with reading.
    // Frost mode drops to 140/255 so the blurred desktop shows through.
    let bg = if frost {
        crate::frost::panel_bg(dark, true)
    } else if dark {
        Color32::from_rgba_unmultiplied(24, 25, 30, 250)
    } else {
        Color32::from_rgba_unmultiplied(246, 247, 250, 250)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(20), bg);

    let inner = rect.shrink(18.0);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );

    // Top bar: brand, centered search, status plus the close icon.
    //
    // All three are positioned with absolute rects: the search box has to be
    // geometrically centered and the brand has to be centered inside the nav
    // width, which flex and margins cannot do (they shift with the text
    // width).
    let bar_h = 56.0;
    let bar = egui::Rect::from_min_size(
        child.max_rect().min,
        egui::vec2(child.max_rect().width(), bar_h),
    );
    let nav_w = nooksill_settings::NAV_W;

    // Ctrl+K focuses the search box.
    let ctrl_k = child
        .ctx()
        .input(|i| i.key_pressed(egui::Key::K) && i.modifiers.ctrl);
    // The fixed id is deliberate: Ctrl+K must request focus for the same
    // field across frames.
    let search_id = egui::Id::unique("nav-search");
    if ctrl_k {
        child.memory_mut(|m| m.request_focus(search_id));
    }

    // Brand: one nav-width slot, text centered in it to line up with the nav
    // items below.
    let mut brand = child.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_size(bar.min, egui::vec2(nav_w, bar_h)))
            .layout(Layout::top_down(Align::Center)),
    );
    brand.centered_and_justified(|ui| {
        ui.label(RichText::new("NookSill").size(20.0).strong());
    });

    // Search: shares `nooksill_settings::text_field` with every other input.
    //
    // The shared widget draws the shell (rounded corners, fill, focus glow,
    // icon, shortcut, clear button); the text uses the official
    // `align(Align2::LEFT_CENTER)`. Squeezing a `TextEdit` into a 34 px high
    // zero-padding `Frame` left the text sitting at the top of its content
    // height, visibly too high.
    let search_rect = egui::Rect::from_center_size(
        bar.center(),
        egui::vec2(260.0, nooksill_settings::text_field::FIELD_H),
    );
    nooksill_settings::text_field::text_field(
        &mut child,
        search_rect,
        search_id,
        &mut state.nav_filter,
        &nooksill_settings::text_field::FieldOpts {
            hint: nooksill_core::i18n::t("Search settings…"),
            icon: nooksill_settings::text_field::FieldIcon::Search,
            trailing: Some("Ctrl K"),
            clearable: true,
        },
    );

    // Right column: status plus a self-drawn close icon in the accent color.
    // Right-to-left, so the first allocation ends up on the right: the icon
    // has to hug the window edge.
    let right = egui::Rect::from_min_max(
        egui::pos2(bar.left() + nav_w, bar.top()),
        egui::pos2(bar.right(), bar.bottom()),
    );
    let mut side = child.new_child(
        egui::UiBuilder::new()
            .max_rect(right)
            .layout(Layout::right_to_left(Align::Center)),
    );
    let (btn, resp) = side.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
    let (icon_color, hover_bg) = {
        let v = side.visuals();
        (
            v.selection.stroke.color,
            v.selection.bg_fill.linear_multiply(0.85),
        )
    };
    if resp.hovered() {
        side.painter()
            .rect_filled(btn, egui::CornerRadius::same(10), hover_bg);
    }
    let c = btn.center();
    let d = 5.0;
    let stroke = egui::Stroke::new(1.8, icon_color);
    side.painter().line_segment(
        [egui::pos2(c.x - d, c.y - d), egui::pos2(c.x + d, c.y + d)],
        stroke,
    );
    side.painter().line_segment(
        [egui::pos2(c.x + d, c.y - d), egui::pos2(c.x - d, c.y + d)],
        stroke,
    );
    // A self-drawn widget must report its node, otherwise screen readers and
    // kittest cannot find it.
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, true, nooksill_core::i18n::t("Close"))
    });
    if resp.clicked() {
        out.command = Some(Command::Close);
    }
    if !state.status.is_empty() {
        side.add_space(8.0);
        side.label(RichText::new(&state.status).small().weak());
    }

    // A child `Ui` does not advance the parent cursor: push the body start
    // below the top bar by hand.
    child.add_space(bar_h);

    let mut page_out = settings_ui(&mut child, cfg, state);
    out.changed |= page_out.changed;
    if out.command.is_none() {
        out.command = page_out.command.take();
    }
}
