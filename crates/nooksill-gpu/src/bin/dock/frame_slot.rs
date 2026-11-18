//! Which renderer draws the dock bar: the GPU frame or the CPU one.
//!
//! The two are separate types rather than one renderer with optional device
//! fields, so the GPU path stays byte-for-byte what it was and a CPU-mode
//! process never constructs a wgpu device at all (ADR-119).
//!
//! Only the bar is dual-path. Panels, menus, the Launchpad and the settings
//! window are GPU-only and pull the device up on first use through
//! `App::gpu`.

use std::time::Duration;

use nooksill_gpu::frame::{FrameOutcome, FrameRenderer, Reflection};
use nooksill_gpu::frost::{Backdrop, Spec};
use nooksill_gpu::gpu::{Gpu, GpuError};
use nooksill_gpu::softframe::SoftFrame;
use windows::Win32::Foundation::HWND;

/// The dock's frame renderer, chosen once at startup from `GpuAccel`.
pub(super) enum FrameSlot {
    /// Both boxed: the GPU renderer is ~1.8 KB and the CPU one carries a 1 KB
    /// sRGB decode table inline, while `App` holds this enum by value.
    Gpu(Box<FrameRenderer>),
    Soft(Box<SoftFrame>),
}

impl std::fmt::Debug for FrameSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gpu(_) => f.write_str("FrameSlot::Gpu"),
            Self::Soft(s) => f.debug_tuple("FrameSlot::Soft").field(s).finish(),
        }
    }
}

impl super::App {
    /// The GPU device, built on first need and shared from then on.
    ///
    /// Returns a clone rather than a reference so callers can read other `App`
    /// fields in the same call expression; wgpu handles are refcounted, so this
    /// is not a second device.
    pub(super) fn gpu(&mut self) -> Result<Gpu, GpuError> {
        if let Some(gpu) = &self.gpu {
            return Ok(gpu.clone());
        }
        let gpu = Gpu::new(&self.dock.cfg.adapter)?;
        self.gpu = Some(gpu.clone());
        Ok(gpu)
    }
}

impl super::App {
    /// The device an existing panel was built with.
    ///
    /// Takes the field, not `&self`, so it stays usable while a panel slot is
    /// mutably borrowed. Never creates a device: these call sites run on every
    /// loop tick, and creating one here would defeat CPU mode immediately.
    pub(super) fn panel_device(gpu: &Option<Gpu>) -> Result<&Gpu, GpuError> {
        gpu.as_ref()
            .ok_or(GpuError::Win32("panel exists without a GPU device"))
    }
}

impl FrameSlot {
    /// The device the GPU variant needs; the CPU variant ignores it.
    fn device(gpu: Option<&Gpu>) -> Result<&Gpu, GpuError> {
        gpu.ok_or(GpuError::Win32("GPU frame path without a device"))
    }

    pub(super) fn ctx(&self) -> &egui::Context {
        match self {
            Self::Gpu(f) => f.ctx(),
            Self::Soft(f) => f.ctx(),
        }
    }

    pub(super) fn size(&self) -> (u32, u32) {
        match self {
            Self::Gpu(f) => f.size(),
            Self::Soft(f) => f.size(),
        }
    }

    pub(super) fn set_pixels_per_point(&mut self, ppp: f32) {
        match self {
            Self::Gpu(f) => f.set_pixels_per_point(ppp),
            Self::Soft(f) => f.set_pixels_per_point(ppp),
        }
    }

    pub(super) fn set_frost_whole_window(&mut self, on: bool) {
        match self {
            Self::Gpu(f) => f.set_frost_whole_window(on),
            Self::Soft(f) => f.set_frost_whole_window(on),
        }
    }

    pub(super) fn set_reflection(&mut self, spec: Option<Reflection>) {
        match self {
            Self::Gpu(f) => f.set_reflection(spec),
            Self::Soft(f) => f.set_reflection(spec),
        }
    }

    /// Smoke is GPU compute and stays dark on the CPU path, so the shared
    /// layer has nowhere to go there.
    pub(super) fn set_smoke_layer(&mut self, layer: nooksill_gpu::shaders::smoke::SharedLayer) {
        if let Self::Gpu(f) = self {
            f.set_smoke_layer(layer);
        }
    }

    /// The GPU live thread re-reads the window rect every tick, so only the CPU
    /// path has to be told to look again.
    pub(super) fn refresh_frost(&mut self, hwnd: HWND) {
        if let Self::Soft(f) = self {
            f.refresh_frost(hwnd);
        }
    }

    pub(super) fn frost_backdrop(&self) -> Option<Backdrop> {
        match self {
            Self::Gpu(f) => f.frost_backdrop(),
            Self::Soft(f) => f.frost_backdrop(),
        }
    }

    pub(super) fn clear_frost(&mut self) {
        match self {
            Self::Gpu(f) => f.clear_frost(),
            Self::Soft(f) => f.clear_frost(),
        }
    }

    pub(super) fn start_live_frost(&mut self, hwnd: HWND, spec: Spec, ppp: f32) {
        match self {
            Self::Gpu(f) => f.start_live_frost(hwnd, spec, ppp),
            Self::Soft(f) => f.start_live_frost(hwnd, spec, ppp),
        }
    }

    pub(super) fn stop_live_frost(&mut self) {
        match self {
            Self::Gpu(f) => f.stop_live_frost(),
            Self::Soft(f) => f.stop_live_frost(),
        }
    }

    pub(super) fn set_frost_spec(&mut self, spec: Spec) {
        match self {
            Self::Gpu(f) => f.set_frost_spec(spec),
            Self::Soft(f) => f.set_frost_spec(spec),
        }
    }

    pub(super) fn frost_pending(&mut self) -> bool {
        match self {
            Self::Gpu(f) => f.frost_pending(),
            Self::Soft(f) => f.frost_pending(),
        }
    }

    pub(super) fn repaint_due(&self) -> bool {
        match self {
            Self::Gpu(f) => f.repaint_due(),
            Self::Soft(f) => f.repaint_due(),
        }
    }

    pub(super) fn repaint_in(&self) -> Option<Duration> {
        match self {
            Self::Gpu(f) => f.repaint_in(),
            Self::Soft(f) => f.repaint_in(),
        }
    }

    pub(super) fn resize(
        &mut self,
        gpu: Option<&Gpu>,
        width: u32,
        height: u32,
    ) -> Result<(), GpuError> {
        match self {
            Self::Gpu(f) => f.resize(Self::device(gpu)?, width, height),
            Self::Soft(f) => f.resize(width, height),
        }
    }

    pub(super) fn render(
        &mut self,
        gpu: Option<&Gpu>,
        raw: egui::RawInput,
        hwnd: HWND,
        pos: (i32, i32),
        build: impl FnMut(&mut egui::Ui),
    ) -> Result<FrameOutcome, GpuError> {
        match self {
            Self::Gpu(f) => f.render(Self::device(gpu)?, raw, hwnd, pos, build),
            Self::Soft(f) => f.render(raw, hwnd, pos, build),
        }
    }
}
