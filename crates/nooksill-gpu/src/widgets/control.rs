//! Control centre popup, opened from the control centre card on the bar.
//!
//! 360 px wide: three columns of tiles, then a master volume slider and the
//! battery caption at the bottom.
//!
//! Differs from the resource monitor panel in two ways: the volume is real
//! (Core Audio via `platform::audio`, not synthetic `VK_VOLUME_*` keys, which
//! would touch the "applications volume" group and flash the system OSD), and
//! the slider tracks the pointer from press to release, taking the mouse
//! capture itself so it keeps following outside the panel.
//!
//! Dark mode and energy saver are stateful: a click flips the panel's own
//! state and repaints, and dark mode also asks the main loop to reload the
//! theme (`dirty_theme`).

use std::pin::Pin;
use std::time::{Duration, Instant};

use crate::glyph::{IconState, IconStyle};
use crate::widgets::style::PanelStyle;
use egui::{Align2, Color32, CornerRadius, FontId, Rect as EguiRect};
use nooksill_core::control::{self, Tile};
use nooksill_platform::audio::{Audio, Battery};
use nooksill_platform::sysact;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetWindowLongPtrW, RegisterClassW, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, GWLP_USERDATA, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOWNA,
    WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_SETCURSOR, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::frame::FrameRenderer;
use crate::gpu::{Gpu, GpuError};

/// Window class name.
pub const CLASS_NAME: &str = "NookSillCtrlCenter";

const PANEL_W: f32 = 360.0;
const PM: f32 = 8.0;
const TILE_H: f32 = 74.0;
const TILE_GAP: f32 = 10.0;
const SLIDER_H: f32 = 28.0;

/// Panel height: title, tiles, slider and battery caption.
fn panel_h(tiles: usize) -> f32 {
    PM * 2.0 + 40.0 + control::rows(tiles) as f32 * (TILE_H + TILE_GAP) + 30.0 + 30.0
}

/// Top-left corner and width of the tile area.
fn tile_origin() -> (f32, f32) {
    (PM + 12.0, PM + 40.0)
}

fn tile_w() -> f32 {
    (PANEL_W - 2.0 * PM - 24.0 - (control::COLS as f32 - 1.0) * TILE_GAP) / control::COLS as f32
}

/// Where a click landed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Hit {
    Tile(usize),
    Slider,
}

/// Control centre panel.
pub struct CcWindow {
    hwnd: HWND,
    renderer: FrameRenderer,
    input: crate::input::Input,
    pos: (i32, i32),
    ppp: f32,
    visible: bool,
    dark: bool,
    /// Set when this panel changed the theme; the main loop reloads it.
    pub dirty_theme: bool,
    /// Icon theme color (`UiAccentCcIcons` toggle); `None` uses a neutral ink.
    icon_accent: Option<Color32>,

    /// Tiles, in the order chosen by core.
    tiles: Vec<Tile>,
    /// Audio device; `None` renders the slider disabled.
    audio: Option<Audio>,
    /// Local volume cache, written to the system after the drag.
    volume: u32,
    mute: bool,
    /// Energy saver on/off, re-read from the system at construction and on
    /// every open.
    energy: bool,
    /// Remembers the pre-energy-saver plan and restores it on exit.
    power: sysact::PowerSaver,

    hover: Option<Hit>,
    hits: Vec<EguiRect>,
    slider: EguiRect,
    /// Volume slider is being dragged.
    dragging: bool,
    /// Tile index pressed, before it is known to be a click or a drag.
    press: Option<usize>,
    /// Grid slot the dragged tile currently occupies.
    drag_tile: Option<usize>,
    /// Client coords at press time, used to detect movement.
    down: (f32, f32),
    /// Whether the tile actually changed slot since the press.
    moved: bool,
    /// New tile order to hand to the main loop after a drag.
    pending_order: Option<Vec<Tile>>,
    next_repaint: Instant,
}

impl std::fmt::Debug for CcWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CcWindow")
            .field("hwnd", &(self.hwnd.0 as isize))
            .field("visible", &self.visible)
            .finish_non_exhaustive()
    }
}

mod input;
mod paint;
mod render;
mod win;
mod window;

pub use self::win::is_cc_window;

#[cfg(test)]
mod tests {
    use super::*;

    /// Panel height follows the number of tile rows.
    #[test]
    fn panel_height_follows_rows() {
        let two = panel_h(6);
        let three = panel_h(8);
        assert!(three > two);
        assert!((three - two - (TILE_H + TILE_GAP)).abs() < 0.01);
    }

    /// Three tiles plus two gaps exactly fill the content width.
    #[test]
    fn three_tiles_fill_the_row() {
        let tw = tile_w();
        let total = tw * 3.0 + TILE_GAP * 2.0;
        let expect = PANEL_W - 2.0 * PM - 24.0;
        assert!((total - expect).abs() < 0.01, "{total} vs {expect}");
    }
}
