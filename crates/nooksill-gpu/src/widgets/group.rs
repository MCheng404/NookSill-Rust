//! Group panel: opens a Dock folder and shows what is inside.
//!
//! Like the other panels (calendar, control centre, resource monitor, timer)
//! it is a second layered window inside the same process, sharing the GPU
//! device and owning its own `FrameRenderer`.
//!
//! Icons
//!
//! The panel has its own `egui::Context` and textures cannot cross contexts,
//! so it runs its own icon thread (same approach as the launchpad). The
//! four-square group icon on the Dock comes from a different path (the
//! `worker` composites the children into one image).
//!
//! Interaction
//!
//! | Action | Result |
//! |---|---|
//! | Left click a child | Launch it |
//! | Hover the x in a cell corner | Remove it from the group |
//! | "Dissolve group" in the header | All children go back to the bar |
//! | Click outside the panel | Dismiss (governed by `[widgets] PanelDismiss`) |

use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::time::Instant;

use crate::widgets::style::PanelStyle;
use egui::{Align2, Color32, CornerRadius, FontId, Rect as EguiRect, Stroke};
use nooksill_core::item::ItemKind;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetWindowLongPtrW, RegisterClassW, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, GWLP_USERDATA, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE,
    SW_SHOW, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WNDCLASSW, WS_EX_APPWINDOW, WS_EX_LAYERED,
    WS_EX_TOPMOST, WS_POPUP,
};

use crate::frame::FrameRenderer;
use crate::gpu::{Gpu, GpuError};
use crate::worker::{IconLoader, IconRequest, Waker, WM_APP_WAKE};

/// Window class name.
const CLASS_NAME: &str = "NookSillGroupPanel";

/// Cells per row.
pub const COLS: usize = 4;
/// Cell width in logical px.
const CELL_W: f32 = 104.0;
/// Cell height in logical px.
const CELL_H: f32 = 104.0;
/// Icon side length.
const ICON_S: f32 = 60.0;
/// Padding on all sides.
const PAD: f32 = 16.0;
/// Header height.
const HEADER_H: f32 = 40.0;

/// An action taken in the panel; the main loop executes it after taking it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupAction {
    /// Nothing happened.
    None,
    /// Launch child `k`.
    Launch(usize),
    /// Remove child `k` from the group.
    Remove(usize),
    /// Move child `k` up (`-1`) or down (`+1`) inside the group.
    Move(usize, i32),
    /// Dissolve the whole group.
    Dissolve,
}

/// One child in the panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupKid {
    pub name: String,
    pub path: String,
    pub kind: ItemKind,
    pub running: bool,
}

/// Logical size of the panel, from the child count.
#[must_use]
pub fn logical_size(count: usize) -> (f32, f32) {
    let rows = count.div_ceil(COLS).max(1);
    let w = (COLS as f32 * CELL_W + PAD * 2.0).min(CELL_W * count as f32 + PAD * 2.0);
    let h = HEADER_H + PAD + rows as f32 * CELL_H + PAD;
    (w.max(200.0), h)
}

/// Icon rect of child `k` (logical coords, origin at the panel top-left).
///
/// Returns `None` for an empty panel. Pure function: hit testing and painting
/// must share the same geometry.
#[must_use]
pub fn cell_rect(k: usize, count: usize) -> Option<(f32, f32, f32, f32)> {
    if k >= count {
        return None;
    }
    let cols = COLS.min(count.max(1));
    let col = k % cols;
    let row = k / cols;
    let x = PAD + col as f32 * CELL_W + (CELL_W - ICON_S) / 2.0;
    let y = HEADER_H + PAD + row as f32 * CELL_H;
    Some((x, y, ICON_S, ICON_S))
}

/// Group panel.
pub struct GroupWindow {
    hwnd: HWND,
    renderer: FrameRenderer,
    input: crate::input::Input,
    pos: (i32, i32),
    ppp: f32,
    dark: bool,
    visible: bool,
    /// Group name, drawn in the header.
    title: String,
    kids: Vec<GroupKid>,
    /// Child icons, indexed by child index.
    icons: HashMap<usize, egui::TextureHandle>,
    /// Paths already requested from the shell, never asked twice.
    asked: HashSet<String>,
    loader: Option<IconLoader>,
    /// Hovered child index.
    hover: Option<usize>,
    /// Pressed child index.
    press: Option<usize>,
    /// Child index hit at release (`None` = released outside every cell).
    released: Option<usize>,
    /// Pending action.
    action: GroupAction,
    /// Instant of the next repaint.
    next_repaint: Option<Instant>,
}

impl std::fmt::Debug for GroupWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GroupWindow")
            .field("hwnd", &(self.hwnd.0 as isize))
            .field("visible", &self.visible)
            .field("kids", &self.kids.len())
            .finish_non_exhaustive()
    }
}

mod input;
mod paint;
mod render;
mod win;
mod window;

pub use self::paint::paint_preview;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_grows_with_the_kid_count() {
        let one = logical_size(1);
        let many = logical_size(9);
        assert!(many.1 > one.1, "九个子项应当比一个高");
        assert!(many.0 >= one.0);
    }

    #[test]
    fn cells_lay_out_in_a_grid() {
        let n = 5;
        let (x0, y0, _, _) = cell_rect(0, n).expect("第一个格子");
        let (x1, y1, _, _) = cell_rect(1, n).expect("第二个格子");
        let (x4, y4, _, _) = cell_rect(4, n).expect("第五个格子");
        assert!(x1 > x0, "同一行的下一个格子在右边");
        assert!((y1 - y0).abs() < 0.01, "同一行 y 相同");
        assert!(y4 > y1, "换行后 y 变大");
        assert!((x4 - x0).abs() < 0.01, "换行后回到第一列");
    }

    #[test]
    fn out_of_range_cells_are_none() {
        assert!(cell_rect(3, 3).is_none());
        assert!(cell_rect(0, 0).is_none());
    }

    #[test]
    fn one_kid_gets_a_single_column() {
        let (x, _, w, _) = cell_rect(0, 1).expect("唯一格子");
        assert!(x >= 0.0);
        assert!(w > 0.0);
    }
}
