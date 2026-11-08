//! The control centre panel painter.

use super::*;

#[expect(
    clippy::too_many_arguments,
    reason = "argument group is the API boundary"
)]
pub(super) fn draw_panel(
    ui: &mut egui::Ui,
    dark: bool,
    frosted: bool,
    tiles: &[Tile],
    volume: u32,
    mute: bool,
    energy: bool,
    has_audio: bool,
    battery: Battery,
    hover: Option<Hit>,
    icon_accent: Option<Color32>,
) -> (Vec<EguiRect>, EguiRect) {
    let rect = ui.max_rect();
    let painter = ui.painter().clone();

    // Shared panel style: base fill, overlay, hairline, three text tiers.
    let style = PanelStyle::current(dark, frosted);
    style.paint_frame(&painter, rect);
    let (tc, sub) = (style.ink(), style.ink_sub());
    let accent = ui.visuals().selection.bg_fill;
    // Icon theme color, behind its own `UiAccentCcIcons` toggle; off means a
    // neutral foreground.
    let icon_color = icon_accent.unwrap_or(tc);
    let tile_off = style.card();
    let tile_off_hover = style.card_hover();

    // Title
    painter.text(
        egui::pos2(rect.left() + PANEL_W * 0.5, rect.top() + PM + 14.0),
        Align2::CENTER_CENTER,
        "控制中心",
        FontId::proportional(15.0),
        tc,
    );

    let (tx0, ty0) = tile_origin();
    let tw = tile_w();
    let mut hits = Vec::with_capacity(tiles.len());

    for (k, tile) in tiles.iter().enumerate() {
        let col = k % control::COLS;
        let row = k / control::COLS;
        let x = rect.left() + tx0 + col as f32 * (tw + TILE_GAP);
        let y = rect.top() + ty0 + row as f32 * (TILE_H + TILE_GAP);
        let r = EguiRect::from_min_size(egui::pos2(x, y), egui::vec2(tw, TILE_H));
        hits.push(r);

        let on = match tile {
            Tile::DarkMode => dark,
            Tile::Mute => mute,
            Tile::Energy => energy,
            _ => false,
        };
        let hovered = hover == Some(Hit::Tile(k));
        let fill = if on {
            accent
        } else if hovered {
            tile_off_hover
        } else {
            tile_off
        };
        painter.rect_filled(r, CornerRadius::same(PanelStyle::R_CARD), fill);

        // Off state uses `icon_color` (neutral when the accent toggle is
        // off); the on state keeps a white glyph on the accent fill.
        let fg = if on { Color32::WHITE } else { icon_color };
        let state = IconState {
            active: on,
            level: if *tile == Tile::Mute {
                volume as f32 / 100.0
            } else {
                1.0
            },
            muted: mute,
            ..IconState::default()
        };
        crate::glyph::control_tile(
            &painter,
            EguiRect::from_center_size(egui::pos2(x + tw * 0.5, y + 24.0), egui::vec2(24.0, 24.0)),
            *tile,
            IconStyle::new(fg, accent, state),
        );
        painter.text(
            egui::pos2(x + tw * 0.5, y + 52.0),
            Align2::CENTER_CENTER,
            tile.label(),
            FontId::proportional(11.5),
            if on {
                Color32::from_rgba_unmultiplied(255, 255, 255, 240)
            } else {
                sub
            },
        );
    }

    // Volume slider.
    let rows = control::rows(tiles.len());
    let sy = rect.top() + ty0 + rows as f32 * (TILE_H + TILE_GAP) + 6.0;
    let slider = EguiRect::from_min_size(
        egui::pos2(rect.left() + tx0, sy),
        egui::vec2(
            tw * control::COLS as f32 + TILE_GAP * (control::COLS as f32 - 1.0),
            SLIDER_H,
        ),
    );
    let icon_c = egui::pos2(slider.left() + 11.0, slider.center().y);
    crate::glyph::speaker(
        &painter,
        EguiRect::from_center_size(icon_c, egui::vec2(20.0, 20.0)),
        IconStyle::new(
            icon_color,
            accent,
            IconState {
                level: volume as f32 / 100.0,
                muted: mute,
                ..IconState::default()
            },
        ),
    );
    let track = EguiRect::from_min_max(
        egui::pos2(slider.left() + 30.0, slider.center().y - 3.0),
        egui::pos2(slider.right() - 4.0, slider.center().y + 3.0),
    );
    let track_bg = if dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, 70)
    } else {
        Color32::from_rgba_unmultiplied(0, 0, 0, 40)
    };
    // The volume track is a pill: half the short side, no near-square ends.
    painter.rect_filled(track, PanelStyle::pill(track.height()), track_bg);

    let frac = if has_audio {
        volume as f32 / 100.0
    } else {
        0.0
    };
    let filled_w = track.width() * frac;
    if filled_w > 1.0 {
        painter.rect_filled(
            EguiRect::from_min_size(track.min, egui::vec2(filled_w, track.height())),
            PanelStyle::pill(track.height()),
            if has_audio { accent } else { sub },
        );
    }
    // The knob stays inside the track even at both ends.
    let knob_x = (track.left() + filled_w).clamp(track.left() + 7.0, track.right() - 7.0);
    painter.circle_filled(
        egui::pos2(knob_x, slider.center().y),
        7.0,
        Color32::from_rgba_unmultiplied(255, 255, 255, 255),
    );

    // Battery caption
    painter.text(
        egui::pos2(slider.left(), slider.bottom() + 8.0),
        Align2::LEFT_TOP,
        battery.text(),
        FontId::proportional(11.0),
        sub,
    );

    (hits, slider)
}
