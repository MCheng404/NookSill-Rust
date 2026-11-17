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
    has_audio: bool,
    battery: Battery,
    toggles: &Toggles,
    // Cached brightness level; `bright_ready` is false without DDC/CI.
    bright: u32,
    bright_ready: bool,
    hover: Option<Hit>,
    icon_accent: Option<Color32>,
) -> (Vec<EguiRect>, EguiRect, EguiRect) {
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
        nooksill_core::i18n::t("Control Center"),
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
            Tile::Wifi => toggles.wifi.as_ref().is_some_and(|w| w.on),
            Tile::Ethernet => true,
            Tile::Bluetooth => toggles.bt_on.unwrap_or(false),
            Tile::Location => toggles.location_on.unwrap_or(false),
            Tile::DarkMode => dark,
            Tile::Settings | Tile::Hotspot => false,
        };
        // A connected WiFi tile shows the SSID instead of the generic label.
        let label = match tile {
            Tile::Wifi => toggles
                .wifi
                .as_ref()
                .and_then(|w| w.ssid.as_deref())
                .unwrap_or_else(|| tile.label()),
            _ => tile.label(),
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
            label,
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
    let track_bg = if dark {
        Color32::from_rgba_unmultiplied(255, 255, 255, 70)
    } else {
        Color32::from_rgba_unmultiplied(0, 0, 0, 40)
    };
    paint_track(
        &painter,
        slider,
        if has_audio {
            volume as f32 / 100.0
        } else {
            0.0
        },
        if has_audio { accent } else { sub },
        track_bg,
    );

    // The brightness row is always drawn. Without DDC/CI it is greyed out;
    // dragging still moves the cached knob and retries the hardware probe.
    let bright_slider = EguiRect::from_min_size(
        egui::pos2(rect.left() + tx0, sy + SLIDER_H + 8.0),
        egui::vec2(slider.width(), SLIDER_H),
    );
    let bright_fg = if bright_ready { icon_color } else { sub };
    crate::glyph::brightness(
        &painter,
        EguiRect::from_center_size(
            egui::pos2(bright_slider.left() + 11.0, bright_slider.center().y),
            egui::vec2(20.0, 20.0),
        ),
        IconStyle::new(
            bright_fg,
            accent,
            IconState {
                level: bright as f32 / 100.0,
                ..IconState::default()
            },
        ),
    );
    paint_track(
        &painter,
        bright_slider,
        bright as f32 / 100.0,
        if bright_ready { accent } else { sub },
        track_bg,
    );

    // Battery caption sits under the brightness slider.
    let last = bright_slider;
    painter.text(
        egui::pos2(last.left(), last.bottom() + 8.0),
        Align2::LEFT_TOP,
        battery.text(),
        FontId::proportional(11.0),
        sub,
    );

    (hits, slider, bright_slider)
}

/// Paints a slider track, its accent fill and the knob.
fn paint_track(
    painter: &egui::Painter,
    slider: EguiRect,
    frac: f32,
    fill: Color32,
    track_bg: Color32,
) {
    let track = EguiRect::from_min_max(
        egui::pos2(slider.left() + 30.0, slider.center().y - 3.0),
        egui::pos2(slider.right() - 4.0, slider.center().y + 3.0),
    );
    // A pill: half the short side, no near-square ends.
    painter.rect_filled(track, PanelStyle::pill(track.height()), track_bg);
    let filled_w = track.width() * frac.clamp(0.0, 1.0);
    if filled_w > 1.0 {
        painter.rect_filled(
            EguiRect::from_min_size(track.min, egui::vec2(filled_w, track.height())),
            PanelStyle::pill(track.height()),
            fill,
        );
    }
    // The knob stays inside the track even at both ends.
    let knob_x = (track.left() + filled_w).clamp(track.left() + 7.0, track.right() - 7.0);
    painter.circle_filled(
        egui::pos2(knob_x, slider.center().y),
        7.0,
        Color32::from_rgba_unmultiplied(255, 255, 255, 255),
    );
}
