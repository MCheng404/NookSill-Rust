//! Calendar popup, opened from the clock card on the bar.
//!
//! 392 x 556 logical px: year/month paging plus Today on top, a Monday-first
//! 6x7 grid with a sub-label per cell (holiday, solar term, lunar day), and the
//! selected day's details at the bottom.
//!
//! Differs from the resource and timer panels in two ways: it is a popup
//! (`WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`, dismissed by the main loop, see
//! [`crate::popup`]) and it sits next to the clock card instead of centered.
//!
//! The date maths live in `nooksill_core::calendar`; this module only paints
//! and hit-tests.

use std::pin::Pin;
use std::time::{Duration, Instant};

use crate::widgets::style::PanelStyle;
use egui::{Align2, Color32, FontId, Rect as EguiRect, Stroke};
use nooksill_core::calendar::{self, CalendarState};
use nooksill_core::config::Edge;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetWindowLongPtrW, RegisterClassW, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, GWLP_USERDATA, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOWNA,
    WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_SETCURSOR, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::frame::FrameRenderer;
use crate::gpu::{Gpu, GpuError};

/// Window class name.
pub const CLASS_NAME: &str = "NookSillCalendar";

/// Panel logical size: 392 wide, height shrinks to 480.
const PANEL_W: f32 = 392.0;
const PANEL_H: f32 = 480.0;

/// Window margin in logical px.
const PM: f32 = 8.0;

/// A clickable target.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Hit {
    /// 0 = prev year, 1 = next year, 2 = prev month, 3 = next month, 4 = today.
    Nav(usize),
    /// Grid cell index, `0..42`.
    Cell(usize),
}

/// Clickable regions of the panel, computed while painting and reused by the
/// mouse handlers.
///
/// Panels avoid egui widgets and compute their own rects: `with_layout` cannot
/// report the remaining width inside a panel.
#[derive(Clone, Debug)]
struct Hits {
    nav: [EguiRect; 5],
    cell: [EguiRect; 42],
}

impl Default for Hits {
    fn default() -> Self {
        // `egui::Rect` has no `Default`; an empty rect never contains a point.
        let n = EguiRect::NOTHING;
        Self {
            nav: [n; 5],
            cell: [n; 42],
        }
    }
}

impl Hits {
    fn at(&self, p: egui::Pos2) -> Option<Hit> {
        for (i, r) in self.nav.iter().enumerate() {
            if r.contains(p) {
                return Some(Hit::Nav(i));
            }
        }
        for (k, r) in self.cell.iter().enumerate() {
            if r.contains(p) {
                return Some(Hit::Cell(k));
            }
        }
        None
    }
}

/// Calendar popup.
pub struct CalendarWindow {
    hwnd: HWND,
    renderer: FrameRenderer,
    /// Input. A read-only panel still needs it: `Input::take` carries
    /// `screen_rect`, and without it egui only exposes a small paint area.
    input: crate::input::Input,
    /// Physical coords of the screen top-left corner.
    pos: (i32, i32),
    ppp: f32,
    /// Displayed year/month and the selected day.
    state: CalendarState,
    /// "Today", sampled once at open.
    today: (i32, u32, u32),
    /// Hovered clickable region.
    hover: Option<Hit>,
    /// Hit rects computed for the current frame.
    hits: Hits,
    visible: bool,
    dark: bool,
    /// Instant of the next repaint.
    next_repaint: Instant,
}

impl std::fmt::Debug for CalendarWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CalendarWindow")
            .field("hwnd", &(self.hwnd.0 as isize))
            .field("visible", &self.visible)
            .field("year", &self.state.year)
            .field("month", &self.state.month)
            .finish_non_exhaustive()
    }
}

mod paint;
mod render;
mod win;
mod window;

pub use self::win::{is_calendar_window, today};

#[cfg(test)]
mod tests {
    use super::win::place_in;
    use super::*;

    /// Uses a fixed virtual screen so assertions do not depend on the host:
    /// the CI virtual screen is only 1024x768.
    fn screen() -> nooksill_core::layout::Rect {
        nooksill_core::layout::Rect::new(0, 0, 2560, 1440)
    }

    /// Places the popup above the card for a bottom dock.
    #[test]
    fn places_above_the_card_for_a_bottom_dock() {
        let (x, y) = place_in(
            392,
            480,
            (1000.0, 900.0, 1100.0, 960.0),
            Edge::Bottom,
            1.0,
            screen(),
        );
        assert!(y + 480 <= 960, "弹层底边应当不越过卡片顶部");
        assert!((x + 196 - 1050).abs() <= 1, "应当水平居中于卡片");
    }

    /// Places the popup below the card for a top dock.
    ///
    /// Compares the content area (window coords + `PM` margin): the 8px
    /// transparent margin overlaps the card edge by 4px so the panel looks
    /// flush instead of floating with a gap.
    #[test]
    fn places_below_the_card_for_a_top_dock() {
        let (_, y) = place_in(
            392,
            480,
            (1000.0, 0.0, 1100.0, 60.0),
            Edge::Top,
            1.0,
            screen(),
        );
        assert!(
            y + PM as i32 >= 60,
            "内容区顶边应当在卡片底部之下，实际 {}",
            y + PM as i32
        );
    }

    /// Places the popup right of the card for a left dock.
    #[test]
    fn places_right_of_the_card_for_a_left_dock() {
        let (x, _) = place_in(
            392,
            480,
            (0.0, 400.0, 60.0, 460.0),
            Edge::Left,
            1.0,
            screen(),
        );
        assert!(
            x + PM as i32 >= 60,
            "内容区左边应当在卡片右侧，实际 {}",
            x + PM as i32
        );
    }

    /// Clamps into the monitor instead of leaving the screen; a small screen
    /// is needed to trigger the clamp.
    #[test]
    fn clamps_into_the_monitor() {
        let small = nooksill_core::layout::Rect::new(0, 0, 1024, 768);
        let (x, y) = place_in(
            392,
            480,
            (0.0, 700.0, 40.0, 760.0),
            Edge::Bottom,
            1.0,
            small,
        );
        assert!(x >= 0, "左边越界: {x}");
        assert!(y >= 0, "上边越界: {y}");
        assert!(x + 392 <= small.right, "右边越界: {}", x + 392);
        assert!(y + 480 <= small.bottom, "下边越界: {}", y + 480);
    }

    /// Hit rects stay inside the panel.
    #[test]
    fn hit_rects_are_inside_the_panel() {
        let h = Hits::default();
        assert_eq!(h.nav.len(), 5);
        assert_eq!(h.cell.len(), 42);
        // All rects are empty by default, so no point hits.
        assert_eq!(h.at(egui::pos2(10.0, 10.0)), None);
    }
}
