//! Pixel-level guarantees for the software path.

use super::{color::Transfer, SoftRenderer};
use egui::epaint::{Mesh, Vertex};
use egui::{pos2, Color32, Pos2, TextureId};

fn vertex(x: f32, y: f32, u: f32, v: f32, c: Color32) -> Vertex {
    Vertex {
        pos: pos2(x, y),
        uv: pos2(u, v),
        color: c,
    }
}

/// Two triangles covering `0,0..size,size`, sharing the main diagonal.
fn quad(size: f32, c: Color32, texture_id: TextureId) -> Mesh {
    Mesh {
        indices: vec![0, 1, 2, 0, 2, 3],
        vertices: vec![
            vertex(0.0, 0.0, 0.0, 0.0, c),
            vertex(size, 0.0, 1.0, 0.0, c),
            vertex(size, size, 1.0, 1.0, c),
            vertex(0.0, size, 0.0, 1.0, c),
        ],
        texture_id,
    }
}

fn primitive(mesh: Mesh, size: f32) -> egui::ClippedPrimitive {
    egui::ClippedPrimitive {
        clip_rect: egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(size, size)),
        primitive: egui::epaint::Primitive::Mesh(mesh),
    }
}

fn pixel(r: &SoftRenderer, x: u32, y: u32) -> [u8; 4] {
    let idx = (y as usize * r.stride()) + x as usize * 4;
    let p = r.pixels();
    [p[idx], p[idx + 1], p[idx + 2], p[idx + 3]]
}

/// The layered window shows wallpaper through untouched pixels, so a fresh
/// surface must be transparent, not black-opaque.
#[test]
fn surface_starts_transparent() {
    let r = SoftRenderer::new(4, 4);
    assert!(r.pixels().iter().all(|&b| b == 0));
}

/// The whole point of writing this instead of using an opaque rasterizer:
/// alpha must survive as coverage, or the dock loses its transparent corners.
#[test]
fn alpha_carries_coverage() {
    let mut r = SoftRenderer::new(8, 8);
    let half = Color32::from_rgba_premultiplied(60, 60, 60, 128);
    r.paint(
        &[primitive(quad(8.0, half, TextureId::default()), 8.0)],
        1.0,
    );
    let p = pixel(&r, 3, 3);
    assert_eq!(p[3], 128, "alpha must not be forced opaque: {p:?}");
    assert_eq!(p[0], 60);
}

#[test]
fn opaque_fill_is_exact() {
    let mut r = SoftRenderer::new(8, 8);
    let solid = Color32::from_rgb(10, 20, 30);
    r.paint(
        &[primitive(quad(8.0, solid, TextureId::default()), 8.0)],
        1.0,
    );
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(pixel(&r, x, y), [10, 20, 30, 255], "at {x},{y}");
        }
    }
}

/// Two triangles share the diagonal; pixel centers land exactly on it. Without
/// the top-left rule those pixels blend twice and the seam shows as a bright
/// line across every rounded rect and glyph.
#[test]
fn shared_edge_blends_once() {
    let mut r = SoftRenderer::new(8, 8);
    let half = Color32::from_rgba_premultiplied(50, 50, 50, 100);
    r.paint(
        &[primitive(quad(8.0, half, TextureId::default()), 8.0)],
        1.0,
    );
    for i in 0..8 {
        assert_eq!(
            pixel(&r, i, i),
            pixel(&r, 0, 7),
            "diagonal pixel {i} differs from an interior pixel"
        );
    }
}

#[test]
fn clip_rect_is_respected() {
    let mut r = SoftRenderer::new(8, 8);
    let mut p = primitive(
        quad(8.0, Color32::from_rgb(255, 0, 0), TextureId::default()),
        8.0,
    );
    p.clip_rect = egui::Rect::from_min_max(pos2(2.0, 2.0), pos2(5.0, 5.0));
    r.paint(&[p], 1.0);
    assert_eq!(pixel(&r, 1, 1), [0, 0, 0, 0]);
    assert_eq!(pixel(&r, 2, 2), [255, 0, 0, 255]);
    assert_eq!(pixel(&r, 4, 4), [255, 0, 0, 255]);
    assert_eq!(pixel(&r, 5, 5), [0, 0, 0, 0]);
}

#[test]
fn clear_resets_to_transparent() {
    let mut r = SoftRenderer::new(4, 4);
    r.paint(
        &[primitive(
            quad(4.0, Color32::from_rgb(1, 2, 3), TextureId::default()),
            4.0,
        )],
        1.0,
    );
    r.clear();
    assert!(r.pixels().iter().all(|&b| b == 0));
}

#[test]
fn resize_keeps_textures_and_clears_pixels() {
    let mut r = SoftRenderer::new(4, 4);
    r.paint(
        &[primitive(
            quad(4.0, Color32::from_rgb(9, 9, 9), TextureId::default()),
            4.0,
        )],
        1.0,
    );
    r.resize(6, 2);
    assert_eq!(r.size(), (6, 2));
    assert_eq!(r.pixels().len(), 6 * 2 * 4);
    assert!(r.pixels().iter().all(|&b| b == 0));
}

/// sRGB tables must round-trip every byte, or flat fills drift by one.
#[test]
fn transfer_round_trips_every_byte() {
    let t = Transfer::new();
    for g in 0..=255_u8 {
        assert_eq!(t.gamma(t.linear(g)), g, "gamma {g}");
    }
}

/// Blending in gamma space would make antialiased edges visibly darker. This
/// pins the linear-space result so a future "simplification" cannot regress it.
#[test]
fn blending_happens_in_linear_space() {
    let t = Transfer::new();
    let mut dst = [255, 255, 255, 255];
    t.blend(&mut dst, [0, 0, 0, 128]);
    // 127/255 of white in linear space encodes to 187; blending the gamma bytes
    // directly would give 127, which is what makes edges look muddy.
    assert_eq!(dst[0], 187, "gamma-space blending would give 127");
    assert_eq!(dst[3], 255);
}

#[test]
fn opaque_source_skips_the_tables() {
    let t = Transfer::new();
    let mut dst = [1, 2, 3, 4];
    t.blend(&mut dst, [10, 20, 30, 255]);
    assert_eq!(dst, [10, 20, 30, 255]);
}

#[test]
fn empty_source_leaves_destination() {
    let t = Transfer::new();
    let mut dst = [1, 2, 3, 4];
    t.blend(&mut dst, [0, 0, 0, 0]);
    assert_eq!(dst, [1, 2, 3, 4]);
}

/// egui points every untextured shape at a white texel, often at uv (0,0).
/// Clamping the sample coordinate instead of each tap index mixes in the
/// transparent neighbour and renders the whole UI at a quarter opacity.
#[test]
fn edge_texel_is_not_diluted_by_its_neighbour() {
    let mut pixels = vec![Color32::TRANSPARENT; 16];
    pixels[0] = Color32::from_rgb(200, 100, 50);
    let image = egui::ColorImage::new([4, 4], pixels);
    let id = TextureId::User(7);
    let mut delta = egui::TexturesDelta::default();
    delta.set.insert(
        id,
        std::iter::once(egui::epaint::ImageDelta {
            image: egui::ImageData::Color(std::sync::Arc::new(image)),
            pos: None,
            options: egui::TextureOptions::LINEAR,
        })
        .collect(),
    );

    let mut r = SoftRenderer::new(4, 4);
    r.apply_textures(&delta);
    delta.clear();
    let mut mesh = quad(4.0, Color32::WHITE, id);
    for v in &mut mesh.vertices {
        v.uv = pos2(0.0, 0.0);
    }
    r.paint(&[primitive(mesh, 4.0)], 1.0);
    assert_eq!(pixel(&r, 1, 1), [200, 100, 50, 255]);
}

/// A real egui frame must reach the surface with antialiased, non-opaque edges.
#[test]
fn real_egui_frame_produces_coverage() {
    let ctx = egui::Context::default();
    let mut r = SoftRenderer::new(64, 64);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(64.0, 64.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |ui| {
        ui.painter()
            .rect_filled(ui.max_rect(), 6.0, Color32::from_rgb(40, 80, 160));
    });
    r.apply_textures(&out.textures_delta);
    let prims = ctx.tessellate(out.shapes, 1.0);
    r.paint(&prims, 1.0);
    out.textures_delta.clear();
    assert!(r.pixels().iter().any(|&b| b != 0), "nothing was drawn");
    assert_eq!(
        pixel(&r, 32, 32),
        [40, 80, 160, 255],
        "the interior of a solid fill must be exactly its color"
    );
    let alphas: Vec<u8> = r.pixels().as_chunks::<4>().0.iter().map(|p| p[3]).collect();
    assert!(alphas.contains(&255), "no fully covered pixel");
    assert!(
        alphas.iter().any(|&a| a > 0 && a < 255),
        "rounded corners produced no partial coverage"
    );
}

/// The empty-destination fast path in `blend` leans on the tables being exact
/// inverses, which `transfer_round_trips_every_byte` already pins down.
#[test]
fn blending_onto_empty_equals_the_source() {
    let t = super::color::Transfer::new();
    for a in [1_u8, 40, 128, 214, 254] {
        let src = [a / 3, a / 2, a, a];
        let mut fast = [0_u8; 4];
        t.blend(&mut fast, src);
        assert_eq!(fast, src, "alpha {a}");
    }
}

/// A near-horizontal edge makes the row-span solve divide by almost zero. The
/// bound saturates the float-to-int cast, and a plain `+ 1` margin then aborts
/// the process in a debug build instead of drawing the triangle.
#[test]
fn near_horizontal_edges_still_rasterize() {
    let mut r = SoftRenderer::new(64, 8);
    let c = Color32::from_rgba_premultiplied(200, 100, 50, 255);
    let mut mesh = Mesh {
        indices: vec![0, 1, 2],
        vertices: vec![
            vertex(0.0, 4.0, 0.0, 0.0, c),
            vertex(64.0, 4.0 + 1e-7, 1.0, 0.0, c),
            vertex(32.0, 8.0, 0.5, 1.0, c),
        ],
        texture_id: TextureId::default(),
    };
    r.paint(&[primitive(mesh.clone(), 64.0)], 1.0);
    assert!(
        r.pixels().as_chunks::<4>().0.iter().any(|p| p[3] == 255),
        "the sliver produced no coverage at all"
    );
    mesh.vertices[1].pos.y = 4.0 - 1e-7;
    r.clear();
    r.paint(&[primitive(mesh, 64.0)], 1.0);
}

/// The clip rect arrives in logical points, so it must be scaled before use.
/// At ppp 2 an unscaled clip covers only the top-left quarter of the surface,
/// which is how the bar lost its right and bottom edges on a scaled monitor.
#[test]
fn clip_rect_is_scaled_by_pixels_per_point() {
    let ctx = egui::Context::default();
    let mut r = SoftRenderer::new(64, 64);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(32.0, 32.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |ui| {
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, Color32::from_rgb(40, 80, 160));
    });
    r.apply_textures(&out.textures_delta);
    let prims = ctx.tessellate(out.shapes, 2.0);
    r.paint(&prims, 2.0);
    out.textures_delta.clear();
    assert_eq!(
        pixel(&r, 40, 40),
        [40, 80, 160, 255],
        "past the unscaled 32x32 clip: the rect was not scaled by ppp"
    );
    assert_eq!(
        pixel(&r, 60, 60),
        [40, 80, 160, 255],
        "the far corner of the surface was clipped away"
    );
}
