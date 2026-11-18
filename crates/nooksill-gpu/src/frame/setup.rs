//! Frame renderer construction and surface/frost configuration.

use super::*;

impl FrameRenderer {
    /// Builds the egui context and renderer; the pixel buffers are deferred to
    /// the first frame.
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
            // A window that is never drawn must not pay for its pixels: the
            // six preloaded panels start hidden and stay hidden until clicked.
            offscreen: None,
            presenter: None,
            size: (width, height),
            samples,
            pixels_per_point,
            device: gpu.device.clone(),
            queue: gpu.queue.clone(),
            shaders: crate::shaders::ShaderRuntime::new(&gpu.device, &gpu.queue),
            blur: None,
            frost_size: None,
            smoke: None,
            reflection: None,
            frost: None,
            frost_corner: 0.0,
            glow: None,
            frost_whole: true,
            live: None,
            frost_spec: None,
            frost_hwnd: None,
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
    /// A renderer that has not drawn yet only records the size.
    ///
    /// Returns immediately when the size did not change, so no texture is
    /// rebuilt for nothing.
    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) -> Result<(), GpuError> {
        if self.size == (width, height) {
            return Ok(());
        }
        // Build before committing: if the presenter fails, the offscreen stays
        // unchanged too, so the renderer never ends up with mismatched halves.
        let offscreen = self
            .offscreen
            .is_some()
            .then(|| Offscreen::new(&gpu.device, width, height, Gpu::COLOR_FORMAT, self.samples));
        let presenter = if self.presenter.is_some() {
            Some(LayeredPresenter::new(width as i32, height as i32)?)
        } else {
            None
        };
        if offscreen.is_some() {
            self.offscreen = offscreen;
        }
        if presenter.is_some() {
            self.presenter = presenter;
        }
        self.size = (width, height);
        Ok(())
    }

    /// Give the layered-window DIB back while the window is hidden.
    ///
    /// The DIB is the only per-window buffer that is plain system memory on
    /// every adapter, so it is the whole working-set cost of an idle window.
    /// The GPU target stays: releasing it measured no working-set gain and
    /// left the D3D12 suballocator holding a higher private-commit watermark.
    ///
    /// The context, glyph atlas, pipeline and GPU target all survive, so a
    /// reopen only pays `CreateDIBSection`. Only call this on a hidden window.
    pub fn release_dib(&mut self) {
        self.presenter = None;
    }

    /// Whether the DIB is currently released.
    #[must_use]
    pub const fn dib_released(&self) -> bool {
        self.presenter.is_none()
    }

    /// Allocate the GPU target if this renderer has not drawn yet.
    pub(super) fn ensure_offscreen(&mut self) {
        if self.offscreen.is_some() {
            return;
        }
        let (width, height) = self.size;
        self.offscreen = Some(Offscreen::new(
            &self.device,
            width,
            height,
            Gpu::COLOR_FORMAT,
            self.samples,
        ));
    }

    /// Allocate the GPU target and the DIB, whichever is missing.
    pub(super) fn ensure_targets(&mut self) -> Result<(), GpuError> {
        self.ensure_offscreen();
        if self.presenter.is_none() {
            let (width, height) = self.size;
            self.presenter = Some(LayeredPresenter::new(width as i32, height as i32)?);
        }
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
}
