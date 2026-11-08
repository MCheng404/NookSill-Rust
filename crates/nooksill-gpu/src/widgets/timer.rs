//! Timer and todo panel, opened from the stopwatch card on the bar.
//!
//! Unlike the resource monitor panel this one is interactive. A `RECT` array
//! records the hit region of every button and todo row, and `ToolMove` walks
//! it; the same approach is used here, except the panel positions everything
//! explicitly with the painter and stores the rects in `Hits` while painting.
//!
//! egui widgets are not used: `with_layout(right_to_left)` in a `horizontal`
//! gets no remaining width and the right-hand items vanish. Panel-like UIs
//! always compute their own rects and hit-test them.

use std::pin::Pin;
use std::time::{Duration, Instant};

use crate::widgets::style::PanelStyle;
use egui::{Align2, Color32, CornerRadius, FontId, Rect as EguiRect};
use nooksill_core::tool::{format_ms, ToolMode, ToolState, MAX_TODOS};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetWindowLongPtrW, RegisterClassW, SetWindowLongPtrW,
    ShowWindow, GWLP_USERDATA, SW_HIDE, SW_SHOW, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_PAINT, WM_SETCURSOR, WNDCLASSW, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_TOPMOST,
    WS_POPUP,
};

use crate::frame::FrameRenderer;
use crate::gpu::{Gpu, GpuError};
use crate::ime::Ime;

/// Window class name.
pub const CLASS_NAME: &str = "NookSillTool";

/// Panel width, `Sc(360)`.
const PANEL_W: f32 = 360.0;

/// Panel height without any todo.
///
/// The fixed `Sc(470)` was sized for 16 todos, so fewer todos left a large
/// blank area. The height is computed from the real count, see [`panel_h`].
const PANEL_H_BASE: f32 = 330.0;

/// Height of one todo row in logical px.
const TODO_ROW_H: f32 = 25.0;

/// Panel height: base plus one row per todo, capped at 640 to stay on screen.
fn panel_h(todos: usize) -> f32 {
    (PANEL_H_BASE + todos as f32 * TODO_ROW_H).min(640.0)
}

/// Repaint interval while running.
///
/// The stopwatch shows hundredths, so it cannot wait a whole second; a
/// repaint here only redraws one number and costs almost nothing.
const TICK: Duration = Duration::from_millis(50);

/// Clickable regions of the panel, computed while painting and used by the
/// mouse handlers.
#[derive(Clone, Debug)]
struct Hits {
    /// 0 = countdown, 1 = stopwatch.
    mode: [EguiRect; 2],
    /// 0 = start/pause, 1 = reset, 2 = minus one minute, 3 = plus one minute.
    action: [EguiRect; 4],
    /// Todo rows (the last one is "add").
    todos: Vec<EguiRect>,
    /// Delete buttons at the right of the todo rows.
    deletes: Vec<EguiRect>,
    /// The "add todo" row.
    add: EguiRect,
}

impl Default for Hits {
    fn default() -> Self {
        // `egui::Rect` has no `Default`, so empty rects stand in; an empty
        // rect never contains a point and cannot produce a false hit.
        let n = EguiRect::NOTHING;
        Self {
            mode: [n; 2],
            action: [n; 4],
            todos: Vec::new(),
            deletes: Vec::new(),
            add: n,
        }
    }
}

impl Hits {
    /// Which clickable region is under the mouse.
    fn at(&self, p: egui::Pos2) -> Option<Hit> {
        let modes = [ToolMode::Countdown, ToolMode::Stopwatch];
        for (i, r) in self.mode.iter().enumerate() {
            if r.contains(p) {
                return Some(Hit::Mode(modes[i]));
            }
        }
        let actions = [
            Action::ToggleRun,
            Action::Reset,
            Action::MinusMinute,
            Action::PlusMinute,
        ];
        for (i, r) in self.action.iter().enumerate() {
            if r.contains(p) {
                return Some(Hit::Action(actions[i]));
            }
        }
        for (i, r) in self.deletes.iter().enumerate() {
            if r.contains(p) {
                return Some(Hit::Delete(i));
            }
        }
        for (i, r) in self.todos.iter().enumerate() {
            if r.contains(p) {
                return Some(Hit::Todo(i));
            }
        }
        if self.add.contains(p) {
            return Some(Hit::Add);
        }
        None
    }
}

/// Action buttons.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Action {
    /// Start or pause.
    ToggleRun,
    /// Reset.
    Reset,
    /// Take one minute off the total.
    MinusMinute,
    /// Add one minute to the total.
    PlusMinute,
}

/// A click target.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Hit {
    Mode(ToolMode),
    Action(Action),
    Todo(usize),
    Delete(usize),
    Add,
}

/// Timer and todo panel.
pub struct ToolWindow {
    hwnd: HWND,
    renderer: FrameRenderer,
    input: crate::input::Input,
    pos: (i32, i32),
    /// Current window size (physical px), used to detect a relayout.
    size: (i32, i32),
    ppp: f32,
    /// Timer and todo state, kept in sync with the ini file.
    pub state: ToolState,
    /// Hit rects computed in the previous frame.
    hits: Hits,
    /// Pointer position in logical points, used to paint the hover highlight.
    hover: Option<egui::Pos2>,
    /// Instant of the next repaint.
    next_repaint: Instant,
    visible: bool,
    dark: bool,
    /// The state changed and must be written to disk.
    pub dirty: bool,
    /// Close requested (Esc or click outside).
    pub close_requested: bool,
    /// Keystroke buffer, filled character by character while adding a todo.
    pub typed: String,
    /// Whether a todo text is being typed.
    typing: bool,
    /// IME state (todo text may be Chinese).
    ime: Ime,
}

impl std::fmt::Debug for ToolWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolWindow")
            .field("hwnd", &(self.hwnd.0 as isize))
            .field("visible", &self.visible)
            .field("running", &self.state.running)
            .finish_non_exhaustive()
    }
}

mod input;
mod paint;
mod render;
mod win;
mod window;

pub use self::win::{now_ms, summary};
