//! Settings window: the second layered window inside the Dock process.
//!
//! Same process so a setting takes effect immediately: `settings_ui` edits the
//! very `Config` the Dock renders from, visible on the next frame. A separate
//! process would need IPC or a restart, and the GPU device is shared anyway -
//! each window only owns its `FrameRenderer`.
//!
//! Differences from the menu window:
//!
//! - The menu is modal (`SetCapture` plus its own message loop); this window is
//!   non-modal, since the user must see the bar change while tuning a value.
//! - It therefore needs `WS_EX_APPWINDOW` to appear in the taskbar and Alt+Tab;
//!   otherwise a covered window could never be found again (there is no tray
//!   entry to fall back on).
//! - It must not use `WM_MOUSEACTIVATE -> MA_NOACTIVATE`: the colour picker
//!   and text input need focus. The menu needs no focus, this window does.

use std::pin::Pin;
use std::time::{Duration, Instant};

use egui::{Align, Color32, CornerRadius, Layout, RichText};
use nooksill_core::config::{Config, Key};
use nooksill_settings::{settings_ui, Command, Outcome, SettingsState};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_CONTROL, VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetForegroundWindow, GetWindowLongPtrW, RegisterClassW,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWLP_USERDATA,
    SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOW, WM_CHAR, WM_ERASEBKGND, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WM_SETCURSOR, WNDCLASSW, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_TOPMOST, WS_POPUP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    WM_IME_COMPOSITION, WM_IME_ENDCOMPOSITION, WM_IME_SETCONTEXT, WM_IME_STARTCOMPOSITION,
};

use crate::frame::FrameRenderer;
use crate::gpu::{Gpu, GpuError};
use crate::input::{map_vk, Input};
use crate::worker::WM_APP_WAKE;

/// Window class name. `nooksill-platform::single_instance` does not use it; it
/// is kept for a "focus the existing settings window" path.
pub const CLASS_NAME: &str = "NookSillSettings";

/// Window size in logical points (local coordinates). 893 = 193 navigation
/// pane + 700 content.
///
/// Public because `examples/settings_shot.rs` must render at the same size, or
/// the screenshot does not represent the real window.
pub const LOGICAL_SIZE: (f32, f32) = (893.0, 720.0);

/// Effective scale of the settings window in pixels per point.
///
/// `UiScale` acts as a custom DPI: 0 (and any stray value below 80) follows the
/// **dock monitor's** DPI, while 80..=300 applies directly as a percentage. It
/// replaces the system scale instead of composing with it: 125% on a 150%
/// system gives 1.25, not 1.875. `monitor_dpi` comes from the dock because the
/// system DPI only describes the primary screen; the settings window opens on
/// whichever monitor the dock sits on.
///
/// egui measures everything in points, so this single key scales fonts,
/// buttons, row heights and card spacing together.
#[must_use]
pub fn effective_ppp(cfg: &Config, monitor_dpi: i32) -> f32 {
    let pct = cfg.get_i32(Key::UI_SCALE);
    if (80..=300).contains(&pct) {
        pct as f32 / 100.0
    } else {
        let dpi = if monitor_dpi > 0 {
            monitor_dpi
        } else {
            nooksill_platform::display::system_dpi()
        };
        dpi as f32 / 96.0
    }
}

/// The settings window and its private renderer.
pub struct SettingsWindow {
    hwnd: HWND,
    renderer: FrameRenderer,
    input: Input,
    /// Screen top-left in physical pixels; `UpdateLayeredWindow` needs it.
    pos: (i32, i32),
    ppp: f32,
    /// UI-private state (page, monitor name, stats text), not part of `Config`.
    pub state: SettingsState,
    /// Previous frame output; the main loop takes it and handles commands.
    pub outcome: Outcome,
    /// Cursor shape egui asked for; `WM_SETCURSOR` carries no such info, so cached.
    cursor: egui::CursorIcon,
    /// Minimum frame interval when egui asks for a repaint soon.
    next_repaint: Option<Instant>,
    /// Interval that "repaint as soon as possible" is paced to: the display
    /// refresh. Cached because the query goes through DWM.
    repaint_cap: Duration,
    /// Whether the last frame was part of a continuous run (egui asked for an
    /// immediate repaint), i.e. the user is dragging or hovering.
    continuous: bool,
    /// Esc or the close button requested a close; consumed by the main loop.
    pub close_requested: bool,
    visible: bool,
    /// Last theme code written to visuals; only a change rewrites the style.
    ui_theme: Option<i32>,
    /// Frame-time statistics shown on the performance page.
    frame_ms: f32,
    samples: u64,
    /// TEMP: sample count at the last trace line.
    probe_samples: u64,
    last_stats_push: Instant,
    /// IME state; the system draws the candidate window, we only take committed text.
    ime: crate::ime::Ime,
}

impl std::fmt::Debug for SettingsWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsWindow")
            .field("hwnd", &(self.hwnd.0 as isize))
            .field("visible", &self.visible)
            .finish_non_exhaustive()
    }
}

/// Clear `GWLP_USERDATA` when the window is destroyed.
///
/// A destroyed window can no longer receive messages, but a later window
/// allocated at the same address would make the stale pointer look valid.
impl Drop for SettingsWindow {
    fn drop(&mut self) {
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
        }
    }
}

mod input;
mod render;
mod shell;
mod win;
mod window;

pub use self::shell::{shell, shell_with_frost};

#[cfg(test)]
mod tests {
    use super::*;

    /// `UiScale` rules: 0 follows the dock monitor, 80..=300 applies, anything
    /// below 80 follows the monitor (clamping guards the ini, the rule guards
    /// the clamp).
    #[test]
    fn 自定义缩放的取值规则() {
        let mut cfg = Config::defaults();
        // A non-primary monitor must win over the system (primary) DPI.
        let sys = nooksill_platform::display::system_dpi();

        // Default 0: follow the dock monitor, not the primary.
        assert!((effective_ppp(&cfg, 144) - 1.5).abs() < 1e-6);
        // A missing DPI (0) still falls back to the system value.
        assert!((effective_ppp(&cfg, 0) - sys as f32 / 96.0).abs() < 1e-6);

        // Active range: replaces, does not compose - 125% is 1.25.
        cfg.apply_i32(Key::UI_SCALE, 125);
        assert!((effective_ppp(&cfg, 144) - 1.25).abs() < 1e-6);
        cfg.apply_i32(Key::UI_SCALE, 300);
        assert!((effective_ppp(&cfg, 144) - 3.0).abs() < 1e-6);

        // Below 80: treated as follow-the-monitor.
        cfg.apply_i32(Key::UI_SCALE, 50);
        assert!((effective_ppp(&cfg, 192) - 2.0).abs() < 1e-6);
    }
}
