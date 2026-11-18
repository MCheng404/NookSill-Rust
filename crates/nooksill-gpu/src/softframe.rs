//! CPU frame path: egui -> software raster -> layered window, with no GPU device.
//!
//! Mirrors the small slice of [`crate::frame::FrameRenderer`] the dock bar uses.
//! The submission order is the same as the GPU path except that steps 3-5
//! (update_buffers, render pass, readback) collapse into one raster call: the
//! pixels are already in system memory, so there is nothing to read back.
//!
//! Deliberately absent: whole-window frost sheet, glow mesh, smoke layer and
//! MSAA. The dock turns the sheet off anyway (`set_frost_whole_window(false)`),
//! smoke is GPU compute and stays dark in this mode (ADR-119), and the
//! rasterizer antialiases through egui's own feathering instead of samples.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

use crate::frame::{FrameOutcome, Reflection};
use crate::gpu::GpuError;
use crate::present::LayeredPresenter;
use crate::soft::SoftRenderer;

/// Everything a capture depends on. The GPU path re-reads all of this every
/// live tick, so a move self-heals there; here it is compared explicitly.
#[derive(Clone, Copy, PartialEq)]
struct CaptureKey {
    /// Screen rect, so any move counts: slide, edge switch, monitor change.
    rect: [i32; 4],
    size: (u32, u32),
    ppp: u32,
    spec: crate::frost::Spec,
}

/// A dock frame that never touches wgpu.
pub struct SoftFrame {
    ctx: egui::Context,
    soft: SoftRenderer,
    presenter: LayeredPresenter,
    size: (u32, u32),
    pixels_per_point: f32,
    frost: Option<egui::TextureHandle>,
    frost_spec: Option<crate::frost::Spec>,
    frost_hwnd: Option<HWND>,
    frost_key: Option<CaptureKey>,
    reflection: Option<Reflection>,
    next_repaint: Option<Instant>,
    #[cfg(test)]
    captures: u32,
}

impl std::fmt::Debug for SoftFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SoftFrame")
            .field("size", &self.size)
            .field("pixels_per_point", &self.pixels_per_point)
            .field("frost", &self.frost.is_some())
            .finish_non_exhaustive()
    }
}

impl SoftFrame {
    /// Build the context, raster buffer and DIB at a physical pixel size.
    ///
    /// Fonts must be installed before the first frame: a missing glyph in
    /// egui 0.37 is a panic, not a tofu box.
    pub fn new(width: u32, height: u32, pixels_per_point: f32) -> Result<Self, GpuError> {
        let ctx = egui::Context::default();
        if !nooksill_settings::fonts::install_cjk(&ctx) {
            eprintln!("警告: 未找到中文字体，界面会因缺字形而崩溃。");
        }
        Ok(Self {
            ctx,
            soft: SoftRenderer::new(width, height),
            presenter: LayeredPresenter::new(width as i32, height as i32)?,
            size: (width, height),
            pixels_per_point,
            frost: None,
            frost_spec: None,
            frost_hwnd: None,
            frost_key: None,
            reflection: None,
            next_repaint: None,
            #[cfg(test)]
            captures: 0,
        })
    }

    #[must_use]
    pub const fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        self.size
    }

    pub const fn set_pixels_per_point(&mut self, ppp: f32) {
        self.pixels_per_point = ppp;
    }

    /// Resize the raster buffer and the DIB, keeping the context and its fonts.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), GpuError> {
        if self.size == (width, height) {
            return Ok(());
        }
        self.soft.resize(width, height);
        self.presenter = LayeredPresenter::new(width as i32, height as i32)?;
        self.size = (width, height);
        Ok(())
    }

    pub const fn set_reflection(&mut self, spec: Option<Reflection>) {
        self.reflection = spec;
    }

    /// No whole-window sheet exists here, so the dock's `false` is the only
    /// behaviour this path has; accepting the call keeps the two paths callable
    /// through one interface.
    pub const fn set_frost_whole_window(&mut self, _on: bool) {}

    /// The background layer as a backdrop, sampled per bar piece by the dock.
    #[must_use]
    pub fn frost_backdrop(&self) -> Option<crate::frost::Backdrop> {
        let tex = self.frost.as_ref()?.id();
        let (w, h) = self.size;
        let k = self.pixels_per_point.max(0.1);
        Some(crate::frost::Backdrop {
            tex,
            win: [w as f32 / k, h as f32 / k],
        })
    }

    /// Publish a composed CPU backdrop, reusing the handle when the size holds.
    pub fn set_frost(&mut self, image: egui::ColorImage) {
        if let Some(tex) = &mut self.frost {
            if tex.size() == image.size {
                tex.set(image, egui::TextureOptions::LINEAR);
                return;
            }
        }
        self.frost = Some(self.ctx.load_texture(
            "nooksill-frost-soft",
            image,
            egui::TextureOptions::LINEAR,
        ));
    }

    pub fn clear_frost(&mut self) {
        self.frost = None;
    }

    /// Arm the backdrop and take the first capture. No thread is started: the
    /// backdrop is refreshed by [`Self::refresh_frost`] when its inputs move.
    pub fn start_live_frost(&mut self, hwnd: HWND, spec: crate::frost::Spec, _ppp: f32) {
        self.frost_spec = Some(spec);
        self.frost_hwnd = Some(hwnd);
        self.frost_key = None;
        self.refresh_frost(hwnd);
    }

    /// A changed radius has no next live frame to wait for, so re-capture now.
    pub fn set_frost_spec(&mut self, spec: crate::frost::Spec) {
        self.frost_spec = Some(spec);
        if let Some(hwnd) = self.frost_hwnd {
            self.refresh_frost(hwnd);
        }
    }

    /// Re-capture only when the capture would differ: the window moved, the
    /// surface was resized, the DPI changed or the blur parameters changed.
    ///
    /// `capture_behind_pct` reads the window's *current* screen rect, so without
    /// this a move leaves the old desktop slice stretched over the new position
    /// forever. Must run before the caller reads [`Self::frost_backdrop`]: a
    /// resize-sized capture replaces the texture handle, and a handle resolved
    /// earlier would then point at a freed texture.
    pub fn refresh_frost(&mut self, hwnd: HWND) {
        let Some(spec) = self.frost_spec else { return };
        let key = self.capture_key(hwnd, spec);
        if self.frost_key == Some(key) {
            return;
        }
        self.frost_key = Some(key);
        #[cfg(test)]
        {
            self.captures += 1;
        }
        spec.apply_soft(self, hwnd, self.pixels_per_point);
    }

    fn capture_key(&self, hwnd: HWND, spec: crate::frost::Spec) -> CaptureKey {
        let mut r = RECT::default();
        // A failed query must not look like a move every tick.
        let rect = if unsafe { GetWindowRect(hwnd, &mut r) }.is_ok() {
            [r.left, r.top, r.right, r.bottom]
        } else {
            [0; 4]
        };
        CaptureKey {
            rect,
            size: self.size,
            ppp: self.pixels_per_point.to_bits(),
            spec,
        }
    }

    /// Nothing is ever pending: there is no capture thread to drain.
    pub const fn frost_pending(&mut self) -> bool {
        false
    }

    pub fn stop_live_frost(&mut self) {
        self.frost_spec = None;
        self.frost_hwnd = None;
        self.frost_key = None;
    }

    #[must_use]
    pub fn repaint_in(&self) -> Option<Duration> {
        self.next_repaint
            .map(|at| at.saturating_duration_since(Instant::now()))
    }

    #[must_use]
    pub fn repaint_due(&self) -> bool {
        self.next_repaint.is_some_and(|at| at <= Instant::now())
    }

    /// Draw a frame and present it. `pos` is the window's screen origin.
    pub fn render(
        &mut self,
        raw: egui::RawInput,
        hwnd: HWND,
        pos: (i32, i32),
        build: impl FnMut(&mut egui::Ui),
    ) -> Result<FrameOutcome, GpuError> {
        let full = self.ctx.run_ui(raw, build);
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
        // `apply_textures` handles both halves of the delta, so the GPU path's
        // separate free-after-submit loop has no counterpart here.
        self.soft.apply_textures(&textures_delta);
        self.soft.clear();
        self.soft.paint(&jobs, pixels_per_point);
        self.presenter
            .upload_rgba_strided(self.soft.pixels(), self.soft.stride());
        if let Some(spec) = self.reflection {
            self.presenter.mirror_below(spec);
        }
        self.presenter.present(hwnd, pos.0, pos.1)?;
        // Skipping the clear trips epaint's drop guard.
        textures_delta.clear();
        self.next_repaint = (delay < Duration::MAX).then(|| Instant::now() + delay);
        Ok(FrameOutcome {
            cursor,
            repaint_delay: delay,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::SoftFrame;

    /// A real window, so `GetWindowRect` reports a rect that can actually move.
    fn layered_window(w: i32, h: i32) -> windows::Win32::Foundation::HWND {
        use windows::core::w;
        use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, RegisterClassW, WNDCLASSW, WS_EX_LAYERED,
            WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
        };
        unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
        unsafe {
            let instance = GetModuleHandleW(None).expect("instance");
            let class = WNDCLASSW {
                hInstance: instance.into(),
                lpfnWndProc: Some(proc),
                lpszClassName: w!("NookSillSoftFrameTest"),
                ..Default::default()
            };
            RegisterClassW(&class);
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("NookSillSoftFrameTest"),
                w!("softframe-test"),
                WS_POPUP,
                0,
                0,
                w,
                h,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .expect("layered window")
        }
    }

    /// The one-frame promise: capture when the inputs move, never while they
    /// hold. `capture_behind_pct` reads the window's current rect, so a move
    /// that does not invalidate leaves the old desktop slice on the new spot.
    #[test]
    fn the_backdrop_is_recaptured_only_when_its_inputs_change() {
        use windows::Win32::UI::WindowsAndMessaging::{
            DestroyWindow, SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
        };
        let hwnd = layered_window(400, 200);
        let mut f = SoftFrame::new(400, 200, 1.0).expect("DIB");
        let spec = crate::frost::Spec {
            radius: 4,
            corner_pt: 0.0,
        };
        f.start_live_frost(hwnd, spec, 1.0);
        assert_eq!(f.captures, 1, "arming must take the first capture");
        f.refresh_frost(hwnd);
        assert_eq!(f.captures, 1, "an unchanged window must not re-capture");

        unsafe { SetWindowPos(hwnd, None, 120, 80, 400, 200, SWP_NOZORDER | SWP_NOACTIVATE) }
            .expect("move");
        f.refresh_frost(hwnd);
        assert_eq!(f.captures, 2, "a move must invalidate the capture");

        f.resize(800, 400).expect("resize");
        f.refresh_frost(hwnd);
        assert_eq!(f.captures, 3, "a resize must invalidate the capture");

        f.set_pixels_per_point(1.5);
        f.refresh_frost(hwnd);
        assert_eq!(f.captures, 4, "a DPI change must invalidate the capture");

        f.set_frost_spec(crate::frost::Spec {
            radius: 9,
            corner_pt: 0.0,
        });
        assert_eq!(f.captures, 5, "a blur change must invalidate the capture");

        f.stop_live_frost();
        f.refresh_frost(hwnd);
        assert_eq!(f.captures, 5, "a disarmed backdrop must stay idle");

        // A successful capture loaded a texture; epaint's Drop guard wants the
        // delta drained.
        let mut out = f.ctx().run_ui(egui::RawInput::default(), |_| {});
        out.textures_delta.clear();
        unsafe { DestroyWindow(hwnd) }.expect("destroy");
    }

    /// No capture thread exists here, so the loop must never wake waiting for a
    /// frost frame that cannot arrive.
    #[test]
    fn a_fresh_cpu_frame_is_idle_and_has_no_backdrop() {
        let mut f = SoftFrame::new(64, 32, 1.0).expect("DIB");
        assert!(!f.frost_pending());
        assert!(f.frost_backdrop().is_none());
        assert!(!f.repaint_due());
        assert!(f.repaint_in().is_none());
        assert_eq!(f.size(), (64, 32));
    }

    /// The backdrop is published in logical points: the dock samples it with
    /// logical rects, so reporting physical pixels would scale every UV wrong.
    #[test]
    fn backdrop_is_reported_in_logical_points() {
        let mut f = SoftFrame::new(120, 60, 1.5).expect("DIB");
        f.set_frost(egui::ColorImage::new(
            [8, 4],
            vec![egui::Color32::WHITE; 32],
        ));
        let back = f.frost_backdrop().expect("frost published");
        assert_eq!(back.win, [80.0, 40.0]);
        f.clear_frost();
        assert!(f.frost_backdrop().is_none());
        // Loading a texture leaves a delta that epaint's Drop guard rejects.
        let mut out = f.ctx().run_ui(egui::RawInput::default(), |_| {});
        out.textures_delta.clear();
    }

    /// A resize keeps the context (and its parsed CJK fonts) and follows the DIB.
    #[test]
    fn resize_follows_the_surface_without_rebuilding_fonts() {
        let mut f = SoftFrame::new(64, 32, 1.0).expect("DIB");
        f.resize(96, 48).expect("resize");
        assert_eq!(f.size(), (96, 48));
        f.resize(96, 48).expect("no-op resize");
        assert_eq!(f.size(), (96, 48));
    }
}
