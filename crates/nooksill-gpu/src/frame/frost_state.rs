//! Frost and illusion state: polling, rebuilding and advancing.

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
        ppp: f32,
        cols: std::ops::Range<u32>,
    ) -> Option<TextureView> {
        if self.blur.is_none() {
            self.blur = crate::shaders::frost::GpuBlur::new(&self.device, &self.queue);
        }
        let blur = self.blur.as_mut()?;
        let scale = f32::from(crate::frost::CAPTURE_PCT) / 100.0;
        blur.blur_range(bits, w, h, spec.radius, spec.corner_pt * ppp * scale, cols)?;
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

    pub(super) fn ensure_illusion(&mut self) -> &mut crate::shaders::illusion::Illusion {
        if self.illusion.is_none() {
            let device = self.device.clone();
            let queue = self.queue.clone();
            self.illusion = Some(crate::shaders::illusion::Illusion::new(&device, &queue));
        }
        self.illusion
            .as_mut()
            .expect("illusion was just created above")
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

    /// Stops the live capture. The background layer stays in place, so the
    /// panel keeps showing the last frame.
    pub fn stop_live_frost(&mut self) {
        self.live = None;
        self.frost_spec = None;
        self.illusion = None;
        self.frost_size = None;
        self.stripe = 0;
        self.last_frame = None;
    }

    /// The current frost parameters.
    #[must_use]
    pub const fn frost_spec(&self) -> Option<crate::frost::Spec> {
        self.frost_spec
    }

    /// Consume the newest captured frame. The classic path re-convolves only
    /// the changed columns; the experimental path re-projects a colour field.
    pub(super) fn poll_frost(&mut self) -> bool {
        let Some(frame) = self.live.as_ref().and_then(crate::frost::Live::latest) else {
            return false;
        };
        let Some(spec) = self.frost_spec else {
            return false;
        };
        // Keep a copy (Arc shared, nearly free): when the mode switches on a
        // static desktop there is no new frame, and this is what rebuilds
        // the current picture through the new path immediately.
        self.last_frame = Some(frame.clone());
        if crate::frost::illusion() {
            return self.poll_illusion(&frame, spec);
        }

        // Coming back from the experimental path (or after a size change)
        // leaves stale columns in the texture, so the whole frame has to be
        // recomputed; the dirty column optimization does not apply.
        let force_full = self.illusion.is_some() || self.frost_size != Some((frame.w, frame.h));
        self.illusion = None;
        self.stripe = 0;
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

    /// Combined live blur plus colour illusion: every frame runs a true
    /// Gaussian, only the range is budgeted.
    ///
    /// The dirty columns are computed exactly; the rest of the frame gets
    /// the colour difference of a low-frequency colour field, so the peak
    /// cost never falls back to a full convolution.
    pub(super) fn poll_illusion(
        &mut self,
        frame: &crate::frost::FrostFrame,
        spec: crate::frost::Spec,
    ) -> bool {
        let size_changed = self.frost_size != Some((frame.w, frame.h));
        let full_frame = frame.x0 == 0 && frame.x1 == frame.w;
        if self.illusion.is_none() || size_changed || full_frame {
            return self.rebuild_illusion_base(frame, spec);
        }

        let budget = (frame.w / TRUE_BLUR_STRIPES).max(1);
        let pad = spec.radius.clamp(1, 12) + 2;
        let dirty = frame.x0.saturating_sub(pad)..frame.x1.saturating_add(pad).min(frame.w);
        let start = self.stripe.min(TRUE_BLUR_STRIPES - 1) * budget;
        self.stripe = (self.stripe + 1) % TRUE_BLUR_STRIPES;
        let cols = if dirty.start < dirty.end && dirty.end - dirty.start <= budget {
            dirty
        } else {
            start..(start + budget).min(frame.w)
        };

        // True Gaussian: the covered columns get the newest structure.
        let Some(base) = self.blur_frost(
            &frame.bits,
            frame.w,
            frame.h,
            spec,
            self.pixels_per_point,
            cols.clone(),
        ) else {
            return false;
        };
        // Colour illusion: the other columns get the low-frequency colour
        // field difference (90 ms interpolation).
        let ill = self.ensure_illusion();
        ill.push(frame.field.as_ref(), cols, frame.w);
        let Some(out) = ill.render(&base, frame.w, frame.h) else {
            return false;
        };
        self.register_frost_view(&out);
        self.frost_size = Some((frame.w, frame.h));
        true
    }

    /// Full true Gaussian rebuild (first frame, size change, mode switch,
    /// full capture frame).
    pub(super) fn rebuild_illusion_base(
        &mut self,
        frame: &crate::frost::FrostFrame,
        spec: crate::frost::Spec,
    ) -> bool {
        let Some(view) = self.blur_frost(
            &frame.bits,
            frame.w,
            frame.h,
            spec,
            self.pixels_per_point,
            0..frame.w,
        ) else {
            return false;
        };
        let field = (*frame.field).clone();
        let ill = self.ensure_illusion();
        ill.set_base(&field, frame.w);
        let Some(out) = ill.render(&view, frame.w, frame.h) else {
            return false;
        };
        self.register_frost_view(&out);
        self.frost_size = Some((frame.w, frame.h));
        self.stripe = 0;
        self.last_frame = Some(frame.clone());
        true
    }

    /// Rebuilds the background from the most recent capture when the mode
    /// switched but this frame brought no new capture.
    ///
    /// Without it the capture thread produces nothing while the desktop is
    /// static, `poll_frost` keeps returning early, and the picture stays on
    /// the old path until the next capture (up to the 300 ms fallback). The
    /// user expects the change right after the toggle.
    pub(super) fn refresh_for_mode(&mut self, illusion: bool) {
        let Some(frame) = self.last_frame.clone() else {
            return;
        };
        let Some(spec) = self.frost_spec else {
            return;
        };
        if illusion {
            let _ = self.rebuild_illusion_base(&frame, spec);
            return;
        }
        if let Some(view) = self.blur_frost(
            &frame.bits,
            frame.w,
            frame.h,
            spec,
            self.pixels_per_point,
            0..frame.w,
        ) {
            self.register_frost_view(&view);
        }
        self.illusion = None;
        self.stripe = 0;
        self.frost_size = Some((frame.w, frame.h));
    }

    /// Advances the colour transition; returns the output texture to register
    /// and whether another frame is needed.
    pub(super) fn advance_illusion(&mut self) -> (Option<TextureView>, bool) {
        if !crate::frost::illusion() {
            return (None, false);
        }
        let Some(base) = self.blur.as_ref().and_then(|b| b.view()).cloned() else {
            return (None, false);
        };
        let Some((w, h)) = self.frost_size else {
            return (None, false);
        };
        let Some(ill) = self.illusion.as_mut() else {
            return (None, false);
        };
        let more = ill.advance();
        if !ill.take_dirty() {
            return (None, more);
        }
        (ill.render(&base, w, h), more)
    }
}
