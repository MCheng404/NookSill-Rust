//! Launchpad: a full-screen application grid.
//!
//! The data model and the "drop onto an icon to merge into a folder" rules
//! live in `nooksill_core::launchpad`; this module only draws, hit-tests and
//! collects input.
//!
//! Unlike the other panels, this window takes focus: the search box reads
//! `WM_CHAR` directly, so `WS_EX_NOACTIVATE` is not an option. It covers the
//! whole monitor on open and hands focus back on close.
//!
//! Icons go through `worker::IconLoader` (the same thread the Dock uses), and
//! only cells on the visible page are queued: fetching a hundred icons at once
//! would stall the open animation for seconds.
//!
//! Known limit: IME composition is not wired, so only characters already
//! committed reach `WM_CHAR`. Latin search works; CJK names cannot be typed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect as EguiRect, Stroke};
use nooksill_core::launchpad::{Launchpad, TakeOut, Top};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetWindowLongPtrW, RegisterClassW, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, GWLP_USERDATA, SW_SHOW, WM_APP, WM_CHAR,
    WM_DROPFILES, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_PAINT, WM_RBUTTONUP, WM_SETCURSOR, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

use crate::frame::FrameRenderer;
use crate::gpu::{Gpu, GpuError};
use crate::ime::Ime;
use crate::worker::{IconLoader, IconRequest, Waker};

pub const CLASS_NAME: &str = "NookSillLaunchpad";

/// Wake message from the icon thread, same convention as the Dock.
const WM_APP_WAKE: u32 = WM_APP + 1;

/// Search box size (logical points).
const SEARCH_W: f32 = 380.0;
const SEARCH_H: f32 = 44.0;
/// Grid side inset, then top and bottom insets (logical points).
const SIDE: f32 = 70.0;
const TOP_H: f32 = 124.0;
const BOT_H: f32 = 146.0;

/// Work the launchpad hands to the main loop; it cannot edit Dock config itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchpadCmd {
    /// Add an application to the Dock bar.
    AddToDock {
        /// Display name.
        name: String,
        /// Executable or target path.
        path: String,
    },
}

/// Which context menu is open, and on what.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum MenuState {
    /// Menu of a top-level cell (unfiltered view).
    Cell(usize),
    /// Menu of one cell in the filtered view (the cell is an app).
    App(usize),
    /// Second level: pick a tag group for this app.
    Category(usize),
    /// Menu of one tag-bar group. The index is 0-based, not the 1-based label.
    Tag(usize),
}

/// What a rename applies to.
///
/// One rename field serves both targets (cell name and tag-group name); the
/// enum avoids a parallel field that Esc, commit and click-away would each
/// have to handle twice.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum RenameTarget {
    /// Top-level cell index.
    Cell(usize),
    /// Tag group index, 0-based.
    Tag(usize),
}

/// Things drawn above the grid: the context menu and the rename field.
#[derive(Copy, Clone, Debug, Default)]
struct Overlay<'a> {
    /// Menu labels; empty = draw no menu.
    menu_items: &'a [String],
    /// Menu top-left (logical points).
    menu_at: (f32, f32),
    /// Keyboard-selected cell, index within the current page. `None` = never used.
    sel: Option<usize>,
    /// Active filter. `0` = all, `1..=tags.len()` = that tag group.
    cat: usize,
    /// Rename target. `None` = idle; the search box then acts as the field.
    renaming: Option<RenameTarget>,
    /// Rename text buffer.
    rename_buf: &'a str,
}

/// Hit targets as `(target, logical rect)`, built while painting and read by
/// the mouse handlers.
type HitList = Vec<(Hit, EguiRect)>;

/// One hit target.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Hit {
    /// k-th grid cell, index within the current page.
    Cell(usize),
    /// k-th context-menu entry.
    MenuItem(usize),
    /// The `+` button in the top-right corner.
    Plus,
    /// k-th tab: 0 = all, 1..=tags.len() = a tag, the last one is `+`.
    Tab(usize),
    /// A-Z sort button in the top-left corner.
    SortAz,
    /// k-th cell inside the folder popup.
    FolderCell(usize),
    /// The remove button of the k-th cell inside the folder popup.
    FolderOut(usize),
}

/// The launchpad window.
pub struct LaunchpadWindow {
    hwnd: HWND,
    renderer: FrameRenderer,
    input: crate::input::Input,
    pos: (i32, i32),
    ppp: f32,
    /// Window size in logical points (= monitor physical size / ppp).
    size: (f32, f32),

    lp: Launchpad,
    /// Grid columns and rows, from `LpCols` / `LpRows`.
    grid: (usize, usize),
    /// Persistence file of the launchpad model.
    dat: PathBuf,
    page: usize,
    query: String,
    open_folder: Option<usize>,

    hover: Option<Hit>,
    hits: HitList,
    /// Items placed on the current page or folder, for testing drops on icons.
    icon_rects: HitList,
    down: (f32, f32),
    cur: (f32, f32),
    press: Option<Hit>,
    dragging: bool,
    drag_src: Option<usize>,
    /// Page index at drag start; the drop resolves against the same page.
    drag_page: usize,

    icons: HashMap<usize, egui::TextureHandle>,
    /// Paths already queued to the icon thread, so nothing is requested twice.
    asked: HashMap<String, usize>,
    loader: Option<IconLoader>,

    /// IME state for the search box and the rename field.
    ime: Ime,
    /// Current context-menu state. `None` = closed.
    menu: Option<MenuState>,
    /// Menu top-left (logical points).
    menu_at: (f32, f32),
    /// Keyboard-selected cell, index within the current page. `None` = never used.
    sel: Option<usize>,
    /// Active filter. `0` = all, `1..=tags.len()` = that tag group.
    cat: usize,
    /// What is being renamed. `None` = the rename field is closed.
    renaming: Option<RenameTarget>,
    /// Rename text buffer.
    rename_buf: String,
    /// Commands waiting for the main loop.
    cmds: Vec<LaunchpadCmd>,
    visible: bool,
    dark: bool,
    dirty_save: bool,
    next_repaint: Instant,
}

impl std::fmt::Debug for LaunchpadWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaunchpadWindow")
            .field("hwnd", &(self.hwnd.0 as isize))
            .field("visible", &self.visible)
            .field("items", &self.lp.len())
            .finish_non_exhaustive()
    }
}

mod commands;
mod compose;
mod input;
mod lifecycle;
mod nav;
mod paint;
mod paint_folder;
mod win;

pub use self::win::is_launchpad_window;

#[cfg(test)]
mod tests {
    use super::paint_folder::{hit_cell_in, hsl_to_rgb};
    use super::*;

    /// A few key points on the hue wheel.
    #[test]
    fn hsl_hits_primary_colors() {
        assert_eq!(hsl_to_rgb(0.0, 1.0, 0.5), (255, 0, 0));
        assert_eq!(hsl_to_rgb(120.0, 1.0, 0.5), (0, 255, 0));
        assert_eq!(hsl_to_rgb(240.0, 1.0, 0.5), (0, 0, 255));
        // Grey: with zero saturation all three channels agree.
        let (r, g, b) = hsl_to_rgb(200.0, 0.0, 0.5);
        assert_eq!((r, g), (b, b));
    }

    /// Drop target: near the cell centre counts as on the icon, cell corners
    /// count as empty space.
    #[test]
    fn drop_hit_test_maps_to_a_cell() {
        // 2 rows, 3 columns, origin (70,124), cell 100x120.
        let h = hit_cell_in(6, 3, 70.0, 124.0, 100.0, 120.0, (175.0, 250.0));
        assert_eq!(h, Some(Hit::Cell(4)));
        // Not in the left inset.
        assert_eq!(
            hit_cell_in(6, 3, 70.0, 124.0, 100.0, 120.0, (10.0, 200.0)),
            None
        );
        // Not below the grid.
        assert_eq!(
            hit_cell_in(6, 3, 70.0, 124.0, 100.0, 120.0, (175.0, 400.0)),
            None
        );
    }
}
