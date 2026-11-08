//! Control-centre tiles: sliders, tiles and speaker.

use super::canvas::Canvas;
use super::*;

/// Paints the sliders icon: two tracks with round knobs.
pub fn sliders(painter: &Painter, rect: EguiRect, style: IconStyle) {
    let c = Canvas::from_rect(painter, rect, style);
    for (i, knob_at) in [(0, 0.62_f32), (1, 0.38_f32)] {
        let y = if i == 0 { -0.20 } else { 0.20 };
        c.polyline(&[(-0.36, y), (0.36, y)]);
        let kx = -0.36 + 0.72 * knob_at;
        c.dot(kx, y, 0.075);
    }
}

/// Paints the icon of one control-centre tile.
///
/// `state.level` drives the lit segment count, `state.phase` drives gear
/// rotation and the hotspot ripple; the caller only advances the state.
pub fn control_tile(painter: &Painter, rect: EguiRect, tile: Tile, style: IconStyle) {
    let c = Canvas::from_rect(painter, rect, style);
    let level = style.state.level.clamp(0.0, 1.0);
    let phase = style.state.phase;
    match tile {
        Tile::Wifi => {
            // A fan shape covers only the lower half, so it is scaled 1.18 to
            // match round icons optically; unlit arcs dim but keep the width.
            const K: f32 = 1.18;
            for i in 0..3 {
                let lit = (level * 3.0).round() as usize > i;
                let ink = if lit { style.color } else { c.faint() };
                c.arc(
                    0.0,
                    0.28 * K,
                    (0.13 + i as f32 * 0.13) * K,
                    -2.50,
                    -0.64,
                    ink,
                );
            }
            c.dot(0.0, 0.28 * K, 0.055 * K);
        }
        Tile::Ethernet => {
            // Rounded case, three pins and a base; segments never touch.
            // Boxed shapes stay inside BOX so they read as large as round ones.
            c.box_stroke(-BOX * 0.88, -0.26, BOX * 1.76, 0.38);
            for dx in [-0.14_f32, 0.0, 0.14] {
                c.polyline(&[(dx, -0.06), (dx, 0.06)]);
            }
            c.polyline(&[(-0.20, 0.22), (0.20, 0.22)]);
            // Two dots on top, replacing strokes that would overlap the case.
            c.dot(-0.14, -0.34, 0.045);
            c.dot(0.14, -0.34, 0.045);
        }
        Tile::Bluetooth => {
            // One closed polyline for the right diamond and stem, plus a
            // separate V on the left; two paths never overlap at endpoints.
            c.polyline(&[
                (-0.03, -0.40),
                (0.23, -0.15),
                (-0.03, 0.0),
                (0.23, 0.15),
                (-0.03, 0.40),
                (-0.03, -0.40),
            ]);
            c.polyline(&[(-0.23, -0.16), (-0.03, 0.0), (-0.23, 0.16)]);
        }
        Tile::DarkMode => {
            // Crescent: outer arc of the big circle, inner arc over the small
            // circle's left half; drawn as two lines so the tips keep round
            // caps instead of miter spikes.
            const R1: f32 = SAFE; // outer circle
            const R2: f32 = 0.34; // subtracted circle
            const DX: f32 = 0.16; // subtracted circle centre offset
            const K: f32 = 0.90; // overall scale into the 20px safe area
            const OX: f32 = -0.04;
            let ix = (R1 * R1 - R2 * R2 + DX * DX) / (2.0 * DX);
            let iy = (R1 * R1 - ix * ix).sqrt();
            let a1 = iy.atan2(ix); // intersection angle on the outer circle
            let a2 = iy.atan2(ix - DX); // intersection angle on the small circle
            let outer_sweep = std::f32::consts::TAU - 2.0 * a1;
            let inner_sweep = std::f32::consts::TAU - 2.0 * a2;
            // Same adaptive sampling as `arc()`; K scales the arc length too.
            let outer_n = c.arc_steps(R1 * K, outer_sweep);
            let inner_n = c.arc_steps(R2 * K, inner_sweep);
            let mut outer: Vec<(f32, f32)> = Vec::with_capacity(outer_n + 1);
            for k in 0..=outer_n {
                let t = a1 + outer_sweep * k as f32 / outer_n as f32;
                let x = R1 * t.cos();
                let y = R1 * t.sin();
                outer.push((x * K + OX, y * K));
            }
            let mut inner: Vec<(f32, f32)> = Vec::with_capacity(inner_n + 1);
            for k in 0..=inner_n {
                let t = -a2 - inner_sweep * k as f32 / inner_n as f32;
                let x = DX + R2 * t.cos();
                let y = R2 * t.sin();
                inner.push((x * K + OX, y * K));
            }
            c.arc_line(&outer, style.color);
            c.arc_line(&inner, style.color);
        }
        Tile::Location => {
            // A teardrop pin: one continuous polyline, sampled arcs and a
            // tapered tip, round joins soften the corner.
            let arc_n = c.arc_steps(0.26, std::f32::consts::PI);
            let mut pts: Vec<(f32, f32)> = (0..=arc_n)
                .map(|i| {
                    let t = std::f32::consts::PI * i as f32 / arc_n as f32;
                    (0.26 * t.cos(), -0.12 - 0.26 * t.sin())
                })
                .collect();
            pts.extend([(0.0, 0.40), (0.26, -0.12)]);
            c.polyline(&pts);
            c.dot(0.0, -0.12, 0.065);
        }
        Tile::Mute => speaker(
            painter,
            rect,
            IconStyle::new(
                style.color,
                style.accent,
                IconState {
                    muted: true,
                    ..style.state
                },
            ),
        ),
        Tile::Settings => {
            // Gear: outer ring, 8 round teeth and an axle hole. The teeth are
            // kept small so the icon does not read heavier than its peers.
            c.circle_stroke(0.0, 0.0, 0.26);
            c.circle_stroke(0.0, 0.0, 0.07);
            let spin = phase * std::f32::consts::TAU / 8.0;
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0 + spin;
                c.circle_filled(0.32 * a.cos(), 0.32 * a.sin(), 0.042, style.color);
            }
        }
        Tile::Energy => {
            // Battery, bolt and a level bar; the three parts never overlap.
            c.box_stroke(-0.32, -0.20, 0.58, 0.38);
            c.circle_filled(0.30, -0.01, 0.045, style.color);
            c.polyline(&[
                (0.05, -0.14),
                (-0.09, 0.00),
                (0.01, 0.00),
                (-0.03, 0.10),
                (0.11, -0.05),
                (0.02, -0.05),
                (0.05, -0.14),
            ]);
            if level > 0.02 {
                c.box_filled(-0.27, 0.11, 0.50 * level, 0.045, style.color);
            }
        }
        Tile::Hotspot => {
            // A fan shape reads lighter than the group, so it is scaled 1.28
            // with a wider arc span; the ripple still follows `phase`.
            const K: f32 = 1.28;
            let ripple = (phase * std::f32::consts::TAU).sin() * 0.02;
            c.dot(0.0, 0.30 * K, 0.082 * K);
            c.arc(0.0, 0.30 * K, 0.19 * K + ripple, -2.55, -0.59, style.color);
            c.arc(0.0, 0.30 * K, 0.35 * K + ripple, -2.55, -0.59, style.color);
        }
    }
}

/// Paints the speaker, shared by the volume slider and the mute tile.
///
/// The body is one closed polyline with round joins; the waves light up
/// segment by segment from `level`.
pub fn speaker(painter: &Painter, rect: EguiRect, style: IconStyle) {
    let c = Canvas::from_rect(painter, rect, style);
    c.polyline(&[
        (-0.38, -0.10),
        (-0.22, -0.10),
        (-0.06, -0.26),
        (-0.06, 0.26),
        (-0.22, 0.10),
        (-0.38, 0.10),
        (-0.38, -0.10),
    ]);
    if style.state.muted {
        c.polyline(&[(0.04, -0.12), (0.28, 0.12)]);
        c.polyline(&[(0.28, -0.12), (0.04, 0.12)]);
    } else {
        let level = style.state.level.clamp(0.0, 1.0);
        for i in 0..2 {
            let lit = (level * 2.0).round() as usize > i;
            let ink = if lit { style.color } else { c.faint() };
            c.arc(0.02, 0.0, 0.20 + i as f32 * 0.12, -0.85, 0.85, ink);
        }
    }
}
