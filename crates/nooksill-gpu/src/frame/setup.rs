//! Frame renderer construction and surface/frost configuration.

use super::*;

impl FrameRenderer {
    /// Builds the offscreen target, the layered-window DIB and the egui
    /// renderer.
    ///
    /// `width`/`height` are physical pixels, matching both `CreateWindowExW`
    /// and the offscreen texture.
    ///
    /// The fonts must be installed before the first frame: a missing glyph in
    /// egui 0.37 is a panic, not a tofu box.
    pub fn new(
        gpu: &Gpu,
        width: u32,
        height: u32,
        pixels_per_point: f32,
    ) -> Result<Self, GpuError> {
        // Large windows use 2x: at high pixel density the extra samples cost
        // tens of MB without visible gain. Config can force 1/2/4.
        let flags = gpu
            .adapter
            .get_texture_format_features(Gpu::COLOR_FORMAT)
            .flags;
        let adapter_specific = gpu
            .device
            .features()
            .contains(egui_wgpu::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
        let area = u64::from(width) * u64::from(height);
        let wanted = crate::perf::auto_msaa_samples(area, crate::gpu::msaa_preference());
        let samples = crate::perf::resolve_samples(wanted, flags, adapter_specific);
        let offscreen = Offscreen::new(&gpu.device, width, height, Gpu::COLOR_FORMAT, samples);
        let presenter = LayeredPresenter::new(width as i32, height as i32)?;

        let ctx = egui::Context::default();
        if !nooksill_settings::fonts::install_cjk(&ctx) {
            eprintln!("警告: 未找到中文字体，界面会因缺字形而崩溃。");
        }

        let renderer = Renderer::new(
            &gpu.device,
            Gpu::COLOR_FORMAT,
            RendererOptions {
                msaa_samples: samples,
                depth_stencil_format: None,
                // Dithering: a soft large-area gradient with low alpha only
                // has a few dozen steps in 8 bit, and banding would be visible
                // without it.
                dithering: true,
                ..Default::default()
            },
        );

        Ok(Self {
            ctx,
            renderer,
            offscreen,
            presenter,
            pixels_per_point,
            device: gpu.device.clone(),
            queue: gpu.queue.clone(),
            shaders: crate::shaders::ShaderRuntime::new(&gpu.device, &gpu.queue),
            blur: None,
            illusion: None,
            frost_size: None,
            stripe: 0,
            last_frame: None,
            last_mode: false,
            smoke: None,
            reflection: None,
            frost: None,
            frost_whole: true,
            live: None,
            frost_spec: None,
            next_repaint: None,
        })
    }

    /// Changes the size of the offscreen target and the DIB while **keeping**
    /// the egui context and the wgpu pipeline.
    ///
    /// Needed because the context menu height depends on the item count, so
    /// every open would need a new size. The two expensive steps in
    /// `FrameRenderer::new` are the egui context (CJK font install, font
    /// file parsing) and the wgpu pipeline compilation, tens of milliseconds
    /// each, and neither depends on the size.
    ///
    /// Returns immediately when the size did not change, so no texture is
    /// rebuilt for nothing.
    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) -> Result<(), GpuError> {
        if self.offscreen.width() == width && self.offscreen.height() == height {
            return Ok(());
        }
        // Build both first: if the presenter fails, the offscreen stays
        // unchanged too, so the renderer never ends up with mismatched halves.
        let offscreen = Offscreen::new(
            &gpu.device,
            width,
            height,
            Gpu::COLOR_FORMAT,
            self.offscreen.samples(),
        );
        let presenter = LayeredPresenter::new(width as i32, height as i32)?;
        self.offscreen = offscreen;
        self.presenter = presenter;
        Ok(())
    }

    /// Connect a smoke compute output to this renderer. Called once at startup.
    pub fn set_smoke_layer(&mut self, layer: crate::shaders::smoke::SharedLayer) {
        self.smoke = Some(layer);
    }

    /// Blur captured bytes on the GPU and register the output with egui.
    /// Returns false so the caller can fall back to CPU blur.
    pub fn set_frost_gpu(
        &mut self,
        bits: &[u8],
        w: u32,
        h: u32,
        spec: crate::frost::Spec,
        ppp: f32,
    ) -> bool {
        let Some(view) = self.blur_frost(bits, w, h, spec, ppp, 0..w) else {
            return false;
        };
        self.register_frost_view(&view);
        true
    }

    /// First frame of the combined path: one full true Gaussian. Later frames
    /// use [`Self::poll_illusion`] for a partial true Gaussian plus colour
    /// field compensation.
    pub fn set_frost_illusion(
        &mut self,
        bits: &[u8],
        w: u32,
        h: u32,
        spec: crate::frost::Spec,
        _ppp: f32,
    ) -> bool {
        let frame = crate::frost::FrostFrame {
            bits: Arc::new(bits.to_vec()),
            w,
            h,
            x0: 0,
            x1: w,
            field: Arc::new(crate::frost::color_field(bits, w as usize, h as usize)),
        };
        self.rebuild_illusion_base(&frame, spec)
    }
}
