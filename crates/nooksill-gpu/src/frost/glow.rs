//! Content-driven soft-light overlay for the frosted backdrop.
//!
//! The operator and mesh live in `nooksill-settings`, where the settings
//! surfaces reuse them; this module keeps the established `frost::glow` paths.

pub use nooksill_settings::glow::{mesh, overlay, soft_light, source, Field, GRID};

#[cfg(test)]
mod tests {
    use super::*;

    /// The mesh must be accepted by egui's renderer with the default white
    /// texture, not just be structurally valid on paper.
    #[test]
    fn glow_mesh_renders_a_frame() {
        if !crate::test_env::desktop() {
            return;
        }
        use crate::frame::FrameRenderer;
        use crate::gpu::Gpu;

        let gpu = Gpu::new("").expect("D3D12 adapter");
        let (w, h) = (64u32, 48u32);
        let bits = vec![128u8; (w * h * 4) as usize];
        let spec = crate::frost::Spec {
            radius: 2,
            corner_pt: 0.0,
        };
        let mut f = FrameRenderer::new(&gpu, w, h, 1.0).expect("renderer");
        f.set_frost(crate::frost::compose(&bits, w, h, spec, 1.0).expect("compose"));
        f.set_frost_glow(Field::default());
        crate::frost::set_glow_strength(50);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(w as f32, h as f32),
            )),
            ..Default::default()
        };
        let (pixels, _) = f.render_rgba(&gpu, raw, |_| {}).expect("frame");
        assert_eq!(pixels.len(), (w * h * 4) as usize);
    }
}
