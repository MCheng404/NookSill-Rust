//! Frame submission: egui render, readback and layered presentation.

use super::*;

/// Illusion animation repaint interval, about one frame at 60 Hz.
const ILLUSION_FRAME_MS: u64 = 16;

impl FrameRenderer {
    /// Remaining time until egui's next requested repaint; `None` means idle.
    #[must_use]
    pub fn repaint_in(&self) -> Option<Duration> {
        self.next_repaint
            .map(|at| at.saturating_duration_since(Instant::now()))
    }

    /// Whether egui's repaint deadline has arrived.
    #[must_use]
    pub fn repaint_due(&self) -> bool {
        self.next_repaint.is_some_and(|at| at <= Instant::now())
    }

    /// Whether a background layer is present. The panel uses this to pick an
    /// opaque or a translucent tint.
    #[must_use]
    pub const fn frost_on(&self) -> bool {
        self.frost.is_some()
    }

    /// The egui `Context`, for `request_repaint` and similar calls.
    #[must_use]
    pub const fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    #[must_use]
    pub const fn pixels_per_point(&self) -> f32 {
        self.pixels_per_point
    }

    /// Change the scale without rebuilding the target.
    ///
    /// Used when the dock moves to a monitor with a different DPI: egui reads
    /// the value from the next `RawInput`, and the frost radius scales with it.
    pub fn set_pixels_per_point(&mut self, ppp: f32) {
        self.pixels_per_point = ppp;
    }

    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        (self.offscreen.width(), self.offscreen.height())
    }

    /// MSAA sample count. The screenshot tool prints it so the
    /// anti-aliasing level can be verified.
    #[must_use]
    pub const fn samples(&self) -> u32 {
        self.offscreen.samples()
    }

    /// Sets the reflection parameters, effective on the next frame.
    /// `None` disables it.
    ///
    /// Reflection lives in the present path rather than in egui: it mirrors
    /// the pixels of the very same frame, which only the readback buffer can
    /// reproduce. Drawing the icons again through egui would be a second
    /// implementation and would drift.
    pub const fn set_reflection(&mut self, spec: Option<Reflection>) {
        self.reflection = spec;
    }

    /// Draw a frame and present it. `pos` is the window's screen origin.
    ///
    /// Submission path: draw into the offscreen target, read the pixels back
    /// and write them straight into the layered-window DIB, then hand the DIB
    /// to `UpdateLayeredWindow`.
    pub fn render(
        &mut self,
        gpu: &Gpu,
        raw: egui::RawInput,
        hwnd: HWND,
        pos: (i32, i32),
        build: impl FnMut(&mut egui::Ui),
    ) -> Result<FrameOutcome, GpuError> {
        let outcome = self.draw_frame(gpu, raw, build);
        {
            let presenter = &mut self.presenter;
            // Write the mapped pixels straight into the DIB: no full-frame
            // Vec allocation and no extra copy. The readback rows are padded
            // to the 256 byte alignment while the DIB rows are tightly
            // packed, so the stride has to be passed along; the DIB holds
            // premultiplied BGRA, which is what the swap produces.
            self.offscreen
                .with_mapped_pixels(&gpu.device, |data, stride| {
                    presenter.upload_rgba_strided(data, stride as usize);
                    Ok(())
                })
                .map_err(GpuError::Wgpu)?;
        }
        if let Some(spec) = self.reflection {
            self.presenter.mirror_below(spec);
        }
        self.presenter.present(hwnd, pos.0, pos.1)?;
        Ok(outcome)
    }

    /// Draw a frame and return premultiplied RGBA pixels without presenting.
    /// Used by offscreen screenshots and pixel regression.
    pub fn render_rgba(
        &mut self,
        gpu: &Gpu,
        raw: egui::RawInput,
        build: impl FnMut(&mut egui::Ui),
    ) -> Result<(Vec<u8>, FrameOutcome), GpuError> {
        let outcome = self.draw_frame(gpu, raw, build);
        let pixels = self
            .offscreen
            .read_pixels(&gpu.device)
            .map_err(GpuError::Wgpu)?;
        Ok((pixels, outcome))
    }

    /// Submit one frame. Both output paths share this draw code.
    pub(super) fn draw_frame(
        &mut self,
        gpu: &Gpu,
        raw: egui::RawInput,
        mut build: impl FnMut(&mut egui::Ui),
    ) -> FrameOutcome {
        // A live frost frame must land before `run_ui` claims texture deltas.
        // Switching the mode has to change the path right away even without a
        // new frame (see `refresh_for_mode`).
        let mode = crate::frost::illusion();
        let mode_changed = mode != self.last_mode;
        self.last_mode = mode;
        let had_new = self.poll_frost();
        if mode_changed && !had_new {
            self.refresh_for_mode(mode);
        }
        // Color transition of the experimental path: each frame advances the
        // interpolation by one step and asks for a repaint, so the desktop
        // changes smoothly instead of jumping once a full frame is ready.
        let (illusion_view, animating) = self.advance_illusion();
        if let Some(view) = illusion_view {
            self.register_frost_view(&view);
        }
        if animating {
            self.ctx
                .request_repaint_after(Duration::from_millis(ILLUSION_FRAME_MS));
        }
        let frost = self.frost.as_ref().map(FrostLayer::id);
        let full = self.ctx.run_ui(raw, |ui| {
            if let Some(id) = frost {
                let rect = ui.max_rect();
                ui.painter().image(
                    id,
                    rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            build(ui);
        });

        // `SmokeFx::paint` produced a view inside `run_ui`; register it now,
        // before the render pass samples the texture. First frame has no id yet
        // and is skipped; from then on the same texture object is refreshed.
        if let Some(layer) = &self.smoke {
            if let Ok(mut l) = layer.lock() {
                if let Some(view) = &l.view {
                    let id = self.shaders.register(&mut self.renderer, view, l.id);
                    l.id = Some(id);
                }
            }
        }

        // `repaint_delay` is how long egui wants to wait before the next
        // frame: `Duration::MAX` means it asked for none, so the caller may
        // sleep until an event arrives.
        let delay = full
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map_or(Duration::MAX, |v| v.repaint_delay);
        let cursor = full.platform_output.cursor_icon;

        let egui::FullOutput {
            shapes,
            pixels_per_point,
            mut textures_delta,
            ..
        } = full;

        let jobs = self.ctx.tessellate(shapes, pixels_per_point);
        for (id, deltas) in &textures_delta.set {
            for delta in deltas {
                self.renderer
                    .update_texture(&gpu.device, &gpu.queue, *id, delta);
            }
        }

        let screen = ScreenDescriptor {
            size_in_pixels: [self.offscreen.width(), self.offscreen.height()],
            pixels_per_point,
        };
        let mut encoder =
            gpu.device
                .create_command_encoder(&egui_wgpu::wgpu::CommandEncoderDescriptor {
                    label: Some("nooksill-frame"),
                });
        let buffers =
            self.renderer
                .update_buffers(&gpu.device, &gpu.queue, &mut encoder, &jobs, &screen);

        {
            // With MSAA the color goes into the multisampled texture and the
            // hardware resolves it into the offscreen texture; without MSAA
            // `color_target()` points both views at the same texture.
            let (color, resolve) = self.offscreen.color_target();
            let mut pass = encoder
                .begin_render_pass(&egui_wgpu::wgpu::RenderPassDescriptor {
                    label: Some("nooksill"),
                    color_attachments: &[Some(egui_wgpu::wgpu::RenderPassColorAttachment {
                        view: color,
                        resolve_target: resolve,
                        depth_slice: None,
                        ops: egui_wgpu::wgpu::Operations {
                            // Transparent clear: alpha = 0 is what lets the
                            // desktop show through the layered window.
                            load: egui_wgpu::wgpu::LoadOp::Clear(
                                egui_wgpu::wgpu::Color::TRANSPARENT,
                            ),
                            store: egui_wgpu::wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            self.renderer.render(&mut pass, &jobs, &screen);
        }
        self.offscreen.copy_to_buffer(&mut encoder);
        gpu.queue
            .submit(buffers.into_iter().chain([encoder.finish()]));

        // Official egui-wgpu order: free after submit, then clear the delta.
        // Skipping the free leaks the texture and makes the next `run` see a
        // delta it never applied.
        for id in &textures_delta.free {
            self.renderer.free_texture(id);
        }
        textures_delta.clear();

        // Drives both the loop's draw decision and its sleep timeout.
        self.next_repaint = repaint_deadline(delay, Instant::now());
        FrameOutcome {
            cursor,
            repaint_delay: delay,
        }
    }
}
