//! Submission path for one frame: egui -> offscreen texture -> readback ->
//! layered window.
//!
//! The order matters and is the reason this lives in one place:
//!
//! 1. `run_ui` produces a `FullOutput`;
//! 2. upload each `textures_delta.set` entry (glyph atlas, icon textures);
//! 3. `tessellate` -> `update_buffers` writes vertices and indices;
//! 4. `begin_render_pass` with `LoadOp::Clear(TRANSPARENT)`: the transparent
//!    clear is what lets the desktop show through, while `Load` or any opaque
//!    colour turns the window into a solid block;
//! 5. `copy_texture_to_buffer` -> synchronous readback -> premultiplied BGRA
//!    into the DIB;
//! 6. `UpdateLayeredWindow` hands the result to DWM;
//! 7. free `textures_delta.free`, then call `clear()` - skipping it trips an
//!    assertion in `Drop`.
//!
//! The offscreen target must be sRGB ([`Gpu::COLOR_FORMAT`]). With
//! `Rgba8Unorm` egui applies its own gamma encoding and the readback turns
//! panel grey 24 into grey 81.

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_wgpu::wgpu::TextureView;
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use windows::Win32::Foundation::HWND;

use crate::gpu::{Gpu, GpuError};
use crate::offscreen::Offscreen;
use crate::present::LayeredPresenter;
pub use crate::present::Reflection;

/// Per frame the full gaussian covers at most `1/STRIPES` of the width. When
/// the changed columns fit the budget they all run the real blur; otherwise
/// the stripes rotate and the rest is reprojected from the colour field, which
/// caps the peak instead of falling back to a full-width convolution.
const TRUE_BLUR_STRIPES: u32 = 4;

/// The two things the caller needs after a frame is rendered.
///
/// `egui::FullOutput` is not returned: shapes and texture deltas are already
/// consumed inside `render`, and handing them out would invite double
/// processing.
#[derive(Clone, Copy, Debug)]
pub struct FrameOutcome {
    /// Cursor shape egui wants (used by `WM_SETCURSOR`).
    pub cursor: egui::CursorIcon,
    /// When to repaint next; `Duration::MAX` = nothing pending, block on messages.
    pub repaint_delay: Duration,
}

/// Frost may come from the GPU compute target or the CPU fallback image.
enum FrostLayer {
    Gpu { id: egui::TextureId },
    Cpu(egui::TextureHandle),
}

impl FrostLayer {
    fn id(&self) -> egui::TextureId {
        match self {
            Self::Gpu { id } => *id,
            Self::Cpu(handle) => handle.id(),
        }
    }
}

/// Render target plus presenter; one per window.
pub struct FrameRenderer {
    ctx: egui::Context,
    renderer: Renderer,
    offscreen: Offscreen,
    presenter: LayeredPresenter,
    pixels_per_point: f32,
    device: egui_wgpu::wgpu::Device,
    queue: egui_wgpu::wgpu::Queue,
    /// Shared compute runtime; also registers native textures with egui.
    shaders: crate::shaders::ShaderRuntime,
    /// Lazily created: only windows that enable frost pay for the pipeline.
    blur: Option<crate::shaders::frost::GpuBlur>,
    /// Experimental live-blur plus illusion combination; both switches must be on.
    illusion: Option<crate::shaders::illusion::Illusion>,
    /// Size of the current base texture; a size change requires a rebuild.
    frost_size: Option<(u32, u32)>,
    /// Which stripe runs the real gaussian next (0..[`TRUE_BLUR_STRIPES`]).
    stripe: u32,
    /// Last captured frame; rebuilds at once when a switch flips on a static desktop.
    last_frame: Option<crate::frost::FrostFrame>,
    /// Blur path used last frame, to detect switch changes.
    last_mode: bool,
    /// Smoke compute output, registered after `run_ui` produces it.
    smoke: Option<crate::shaders::smoke::SharedLayer>,
    /// Reflection parameters; `None` = draw none (settings window and menus).
    reflection: Option<Reflection>,
    /// Frost background layer, drawn below the UI (see `crate::frost`).
    frost: Option<FrostLayer>,
    /// Live capture thread while the panel is visible; `None` = one capture at open.
    live: Option<crate::frost::Live>,
    /// Last [`crate::frost::Spec`]; live frames need it for radius and corners.
    frost_spec: Option<crate::frost::Spec>,
    /// Deadline egui asked for on the last frame; `None` means no repaint.
    next_repaint: Option<Instant>,
}

impl std::fmt::Debug for FrameRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameRenderer")
            .field("size", &(self.offscreen.width(), self.offscreen.height()))
            .field("pixels_per_point", &self.pixels_per_point)
            .finish_non_exhaustive()
    }
}

/// Convert egui's `repaint_delay` into a deadline. `Duration::MAX` means idle.
fn repaint_deadline(delay: Duration, now: Instant) -> Option<Instant> {
    if delay == Duration::MAX {
        None
    } else if delay.is_zero() {
        Some(now)
    } else {
        // Cap the add so a pathological delay cannot overflow `Instant`.
        Some(now + delay.min(Duration::from_secs(3600)))
    }
}

mod frost_state;
mod render;
mod setup;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repaint_deadline_maps_egui_states() {
        let now = Instant::now();
        assert_eq!(repaint_deadline(Duration::MAX, now), None);
        assert_eq!(repaint_deadline(Duration::ZERO, now), Some(now));
        assert_eq!(
            repaint_deadline(Duration::from_millis(16), now),
            Some(now + Duration::from_millis(16))
        );
        assert_eq!(
            repaint_deadline(Duration::from_secs(100_000), now),
            Some(now + Duration::from_secs(3600))
        );
    }
}
