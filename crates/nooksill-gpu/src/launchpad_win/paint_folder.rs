//! Folder panel, letter tiles and text fitting helpers.

use super::*;

/// Computes the cell under the pointer during a drag (unlike `hit_at`, whose
/// hit areas are stale while dragging).
pub(super) fn hit_cell_in(
    per: usize,
    cols: usize,
    gx: f32,
    gy: f32,
    cw: f32,
    ch: f32,
    p: (f32, f32),
) -> Option<Hit> {
    if p.0 < gx || p.1 < gy {
        return None;
    }
    let col = ((p.0 - gx) / cw) as usize;
    let row = ((p.1 - gy) / ch) as usize;
    if col >= cols || row >= (per / cols) {
        return None;
    }
    Some(Hit::Cell(row * cols + col))
}

#[expect(
    clippy::too_many_arguments,
    reason = "argument group is the API boundary"
)]
pub(super) fn draw_folder_panel(
    ui: &mut egui::Ui,
    dark: bool,
    (w, h): (f32, f32),
    lp: &Launchpad,
    fi: usize,
    _cols: usize,
    cw: f32,
    ch: f32,
    icon_s: f32,
    icons: &HashMap<usize, egui::TextureHandle>,
    hover: Option<Hit>,
    hits: &mut HitList,
) {
    let painter = ui.painter().clone();
    let tc = if dark {
        Color32::from_rgba_unmultiplied(240, 240, 245, 255)
    } else {
        Color32::from_rgba_unmultiplied(28, 28, 34, 255)
    };
    let sub = if dark {
        Color32::from_rgba_unmultiplied(200, 202, 214, 255)
    } else {
        Color32::from_rgba_unmultiplied(90, 94, 108, 255)
    };
    let hover_bg = if dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, 46)
    } else {
        Color32::from_rgba_unmultiplied(0, 0, 0, 34)
    };

    let members = &lp.folders()[fi].apps;
    let pc = members.len().clamp(1, 5);
    let pr = members.len().div_ceil(5).max(1);
    let pw = pc as f32 * cw + 60.0;
    let ph = pr as f32 * ch + 96.0;
    let panel = EguiRect::from_min_size(
        Pos2::new((w - pw) * 0.5, (h - ph) * 0.5),
        egui::vec2(pw, ph),
    );

    // Dimmed backdrop and panel.
    painter.rect_filled(
        EguiRect::from_min_size(Pos2::ZERO, egui::vec2(w, h)),
        CornerRadius::ZERO,
        Color32::from_rgba_unmultiplied(0, 0, 0, 120),
    );
    painter.rect_filled(
        panel,
        CornerRadius::same(28),
        if dark {
            Color32::from_rgba_unmultiplied(40, 41, 48, 246)
        } else {
            Color32::from_rgba_unmultiplied(246, 247, 252, 246)
        },
    );
    painter.text(
        Pos2::new(panel.center().x, panel.top() + 30.0),
        Align2::CENTER_CENTER,
        &lp.folders()[fi].name,
        FontId::proportional(15.0),
        tc,
    );
    painter.text(
        Pos2::new(panel.center().x, panel.top() + 56.0),
        Align2::CENTER_CENTER,
        nooksill_core::i18n::t("Scroll / Esc to close · right-click to move an app out"),
        FontId::proportional(10.5),
        sub,
    );

    for (k, &ai) in members.iter().enumerate() {
        let x = panel.left() + 30.0 + (k % 5) as f32 * cw;
        let y = panel.top() + 76.0 + (k / 5) as f32 * ch;
        let cell = EguiRect::from_min_size(Pos2::new(x, y), egui::vec2(cw, ch));
        if hover == Some(Hit::FolderCell(k)) {
            painter.rect_filled(
                EguiRect::from_min_max(
                    cell.min + egui::vec2(4.0, 2.0),
                    cell.max - egui::vec2(4.0, 8.0),
                ),
                CornerRadius::same(16),
                hover_bg,
            );
        }
        hits.push((Hit::FolderCell(k), cell));

        let ir = EguiRect::from_min_size(
            Pos2::new(
                cell.center().x - icon_s * 0.5,
                y + (ch - icon_s - 34.0) * 0.5,
            ),
            egui::vec2(icon_s, icon_s),
        );
        if let Some(tex) = icons.get(&ai) {
            painter.image(
                tex.id(),
                ir,
                EguiRect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            draw_letter_tile(&painter, &ir, &lp.apps()[ai].name, dark);
        }
        crate::text::draw_truncated(
            &painter,
            Pos2::new(cell.center().x, ir.bottom() + 8.0),
            Align2::CENTER_TOP,
            &lp.apps()[ai].name,
            cw - 12.0,
            11.5,
            tc,
        );

        // Hovering shows a minus in the top right corner: clicking it takes
        // this item out of the folder.
        if hover == Some(Hit::FolderCell(k)) || hover == Some(Hit::FolderOut(k)) {
            let out = EguiRect::from_center_size(
                Pos2::new(ir.right() - 6.0, ir.top() + 6.0),
                egui::vec2(18.0, 18.0),
            );
            painter.circle_filled(
                out.center(),
                9.0,
                Color32::from_rgba_unmultiplied(0, 0, 0, 150),
            );
            painter.line_segment(
                [
                    Pos2::new(out.center().x - 4.0, out.center().y),
                    Pos2::new(out.center().x + 4.0, out.center().y),
                ],
                Stroke::new(1.6, Color32::WHITE),
            );
            hits.push((Hit::FolderOut(k), out));
        }
    }
}

/// Folder icon: a translucent rounded square holding a 3x3 grid of small
/// icons.
pub(super) fn draw_folder_icon(
    painter: &egui::Painter,
    r: &EguiRect,
    lp: &Launchpad,
    fi: usize,
    icons: &HashMap<usize, egui::TextureHandle>,
) {
    painter.rect_filled(
        *r,
        CornerRadius::same((r.width() * 0.24) as u8),
        Color32::from_rgba_unmultiplied(255, 255, 255, 190),
    );
    let s = r.width();
    let cell = s * 0.26;
    let gap = s * 0.05;
    let off = (s - 3.0 * cell - 2.0 * gap) * 0.5;
    for (k, &ai) in lp.folders()[fi].apps.iter().take(9).enumerate() {
        let x = r.left() + off + (k % 3) as f32 * (cell + gap);
        let y = r.top() + off + (k / 3) as f32 * (cell + gap);
        let sub_r = EguiRect::from_min_size(Pos2::new(x, y), egui::vec2(cell, cell));
        if let Some(tex) = icons.get(&ai) {
            painter.image(
                tex.id(),
                sub_r,
                EguiRect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            painter.rect_filled(sub_r, CornerRadius::same(3), Color32::from_gray(160));
        }
    }
}

/// Placeholder shown while the icon is still loading: a rounded tile with the
/// first letter.
pub(super) fn draw_letter_tile(painter: &egui::Painter, r: &EguiRect, name: &str, dark: bool) {
    // A hash of the name picks a stable hue, so the tiles are not all the
    // same colour.
    let mut h: u32 = 0;
    for b in name.bytes() {
        h = h.wrapping_mul(31).wrapping_add(u32::from(b));
    }
    let hue = (h % 360) as f32;
    let (rr, gg, bb) = hsl_to_rgb(hue, 0.45, if dark { 0.42 } else { 0.68 });
    painter.rect_filled(
        *r,
        CornerRadius::same((r.width() * 0.22) as u8),
        Color32::from_rgba_unmultiplied(rr, gg, bb, 255),
    );
    let ch = name.chars().next().unwrap_or('?');
    painter.text(
        r.center(),
        Align2::CENTER_CENTER,
        ch.to_uppercase().to_string(),
        FontId::proportional(r.width() * 0.44),
        Color32::WHITE,
    );
}

/// HSL to RGB (good enough, not exact).
pub(super) fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c * 0.5;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// Measures the width of a text (logical points).
pub(super) fn measure(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    ui.ctx().fonts_mut(|f| {
        f.layout_no_wrap(text.to_owned(), FontId::proportional(size), Color32::WHITE)
            .size()
            .x
    })
}

/// Finds a font size that fits `text` into `max_w` (never below 9 pt).
///
/// The rename field never truncates a name: the user is looking at it, and
/// one missing character can mean renaming the wrong thing. Shrinking the
/// font is safer than cutting text.
pub(super) fn fit_size(ui: &egui::Ui, text: &str, max_w: f32, size: f32) -> f32 {
    let w = measure(ui, text, size);
    if w <= max_w || w <= 0.0 {
        size
    } else {
        (size * max_w / w).max(9.0)
    }
}
