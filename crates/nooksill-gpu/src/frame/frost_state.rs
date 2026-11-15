//! Frost state: polling and rebuilding the background layer.

use super::*;

impl FrameRenderer {
    /// Runs the GPU convolution and returns the output texture view (not
    /// registered with egui).
    /// `cols` is the dirty column range; a full frame passes `0..w`.
    pub(super) fn blur_frost(
        &mut self,
        bits: &[u8],
        w: u32,
        h: u32,
        spec: crate::frost::Spec,
        _ppp: f32,
        cols: std::ops::Range<u32>,
    ) -> Option<TextureView> {
        if self.blur.is_none() {
            self.blur = crate::shaders::frost::GpuBlur::new(&self.device, &self.queue);
        }
        let blur = self.blur.as_mut()?;
        // Corner 0: the renderer clips the rounded outline in screen space, so
        // the texture must stay a full square (see `frost::round`).
        blur.blur_range(bits, w, h, spec.radius, 0.0, cols)?;
        blur.view().cloned()
    }

    /// Registers a compute output as the egui frost texture, refreshing it in
    /// place when it already exists.
    pub(super) fn register_frost_view(&mut self, view: &TextureView) {
        let filter = egui_wgpu::wgpu::FilterMode::Linear;
        let id = match &self.frost {
            Some(FrostLayer::Gpu { id }) => {
                self.renderer.update_egui_texture_from_wgpu_texture(
                    &self.device,
                    view,
                    filter,
                    *id,
                );
                *id
            }
            _ => self
                .renderer
                .register_native_texture(&self.device, view, filter),
        };
        self.frost = Some(FrostLayer::Gpu { id });
    }

    /// CPU fallback: reuse the texture handle when the size is unchanged.
    pub fn set_frost(&mut self, image: egui::ColorImage) {
        if let Some(FrostLayer::Cpu(tex)) = &mut self.frost {
            if tex.size() == image.size {
                tex.set(image, egui::TextureOptions::LINEAR);
                return;
            }
        }
        let tex = self
            .ctx
            .load_texture("nooksill-frost", image, egui::TextureOptions::LINEAR);
        self.frost = Some(FrostLayer::Cpu(tex));
    }

    /// Whether a frost layer is present.
    #[must_use]
    pub fn has_frost(&self) -> bool {
        self.frost.is_some()
    }

    /// Drops the background layer (frost turned off, or the capture failed).
    pub fn clear_frost(&mut self) {
        self.frost = None;
    }

    /// Starts the live background capture.
    pub fn start_live_frost(&mut self, hwnd: HWND, spec: crate::frost::Spec, _ppp: f32) {
        self.frost_spec = Some(spec);
        self.live = crate::frost::Live::start(hwnd);
    }

    /// Changes the blur parameters without restarting the capture.
    ///
    /// A different radius rebuilds the convolution target on the next frame, so
    /// dragging the blur slider costs no thread churn.
    pub fn set_frost_spec(&mut self, spec: crate::frost::Spec) {
        self.frost_spec = Some(spec);
    }

    /// Whether a captured frame is waiting to be blurred.
    ///
    /// Long-lived windows ask this before deciding to draw: the capture thread
    /// only sends frames when the desktop changed, so nothing is paid for a
    /// still background.
    pub fn frost_pending(&mut self) -> bool {
        self.live
            .as_mut()
            .is_some_and(crate::frost::Live::has_frame)
    }

    /// Stops the live capture. The background layer stays in place, so the
    /// panel keeps showing the last frame.
    pub fn stop_live_frost(&mut self) {
        self.live = None;
        self.frost_spec = None;
        self.frost_size = None;
    }

    /// The current frost parameters.
    #[must_use]
    pub const fn frost_spec(&self) -> Option<crate::frost::Spec> {
        self.frost_spec
    }

    /// Consume the newest captured frame and re-convolve the changed columns.
    pub(super) fn poll_frost(&mut self) -> bool {
        let Some(frame) = self.live.as_mut().and_then(crate::frost::Live::latest) else {
            return false;
        };
        let Some(spec) = self.frost_spec else {
            return false;
        };
        // The glow follows the desktop at live-frame rate, not just on open.
        self.glow = Some(crate::frost::glow::Field::sample(
            &frame.bits[..],
            frame.w as usize,
            frame.h as usize,
        ));
        // A size change leaves stale columns in the texture, so the whole
        // frame has to be recomputed; the dirty column optimization does not
        // apply.
        let force_full = self.frost_size != Some((frame.w, frame.h));
        let pad = spec.radius.clamp(1, 12) + 2;
        let cols = if force_full {
            0..frame.w
        } else {
            frame.x0.saturating_sub(pad)..frame.x1.saturating_add(pad).min(frame.w)
        };
        let Some(view) = self.blur_frost(
            &frame.bits,
            frame.w,
            frame.h,
            spec,
            self.pixels_per_point,
            cols,
        ) else {
            return false;
        };
        self.register_frost_view(&view);
        self.frost_size = Some((frame.w, frame.h));
        true
    }
}
