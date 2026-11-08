//! Desktop work-area reservation through the AppBar protocol.
//! Registration is system wide and survives a killed process, so `Drop` removes
//! it. Position is re-queried with `ABM_QUERYPOS` instead of caching a key.

use nooksill_core::config::Edge;
use nooksill_core::layout::Rect;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETSTATE, ABM_GETTASKBARPOS,
    ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, ABS_AUTOHIDE, APPBARDATA,
};

/// AppBar callback message, carrying `ABN_*` notifications.
///
/// The notifications are ignored (geometry is re-checked anyway), but the value
/// is required: `ABM_NEW` uses it to tell AppBars apart. The high offset avoids
/// the dock's own `WM_APP` messages.
pub const WM_APP_AB: u32 = 0x8000 + 0x4142;

/// Current taskbar edge and autohide state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Taskbar {
    pub edge: Edge,
    pub autohide: bool,
}

/// Read the taskbar edge and autohide state; `None` when unavailable (explorer
/// down or replaced).
#[must_use]
pub fn taskbar() -> Option<Taskbar> {
    let mut pos = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        ..Default::default()
    };
    // SAFETY: `pos` is an APPBARDATA with cbSize set.
    if unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &raw mut pos) } == 0 {
        return None;
    }
    let edge = match pos.uEdge {
        ABE_LEFT => Edge::Left,
        ABE_TOP => Edge::Top,
        ABE_RIGHT => Edge::Right,
        _ => Edge::Bottom,
    };
    let mut state = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        ..Default::default()
    };
    // `ABM_GETSTATE` puts the flags in lParam; the return value only signals
    // success.
    let ok = unsafe { SHAppBarMessage(ABM_GETSTATE, &raw mut state) } != 0;
    let autohide = ok && (state.lParam.0 as u32 & ABS_AUTOHIDE) != 0;
    Some(Taskbar { edge, autohide })
}

/// Whether the taskbar sits on the same edge and is set to auto-hide.
///
/// Shell still reserves a strip for the hidden taskbar, so registering an AppBar
/// there pushes the dock off the screen edge and leaves a visible gap.
#[must_use]
pub fn taskbar_steals_edge(edge: Edge, taskbar: Option<Taskbar>) -> bool {
    matches!(taskbar, Some(t) if t.autohide && t.edge == edge)
}

/// Whether an AppBar reservation should be registered at all.
#[must_use]
pub fn should_reserve(
    cfg_reserve: bool,
    always: bool,
    edge: Edge,
    taskbar: Option<Taskbar>,
) -> bool {
    cfg_reserve && always && !taskbar_steals_edge(edge, taskbar)
}

/// A registered AppBar.
#[derive(Debug)]
pub struct AppBar {
    hwnd: HWND,
    /// Whether `ABM_NEW` succeeded.
    registered: bool,
    /// Last submitted edge and rect; identical values skip `ABM_SETPOS`.
    applied: Option<(Edge, Rect)>,
}

impl AppBar {
    /// Bind a window without registering; the first `apply` does that.
    #[must_use]
    pub fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            registered: false,
            applied: None,
        }
    }

    /// Reserve a `thickness`-wide strip on `edge` and return the rect granted.
    ///
    /// The caller must place the window on the returned rect, because
    /// `ABM_QUERYPOS` can move the request to where space is really available.
    ///
    /// `monitor` is the full `rcMonitor` rect, not the work area: an AppBar cuts
    /// from the whole screen.
    pub fn apply(&mut self, edge: Edge, thickness: i32, monitor: Rect) -> Option<Rect> {
        if !self.registered {
            let mut ab = self.base();
            ab.uCallbackMessage = WM_APP_AB;
            // ABM_NEW returning FALSE means the shell refused registration;
            // continuing to SETPOS would silently do nothing.
            if unsafe { SHAppBarMessage(ABM_NEW, &raw mut ab) } == 0 {
                return None;
            }
            self.registered = true;
        }

        let mut ab = self.base();
        let mut want = monitor;
        match edge {
            Edge::Top => {
                ab.uEdge = ABE_TOP;
                want.bottom = want.top + thickness;
            }
            Edge::Bottom => {
                ab.uEdge = ABE_BOTTOM;
                want.top = want.bottom - thickness;
            }
            Edge::Left => {
                ab.uEdge = ABE_LEFT;
                want.right = want.left + thickness;
            }
            Edge::Right => {
                ab.uEdge = ABE_RIGHT;
                want.left = want.right - thickness;
            }
        }
        ab.rc = to_win_rect(want);

        // The shell moves the rect to where space is actually free.
        unsafe { SHAppBarMessage(ABM_QUERYPOS, &raw mut ab) };

        // QUERYPOS may only adjust the coordinates; the thickness must be forced
        // back to ours, otherwise the granted strip can be a narrow sliver.
        match edge {
            Edge::Top => ab.rc.bottom = ab.rc.top + thickness,
            Edge::Bottom => ab.rc.top = ab.rc.bottom - thickness,
            Edge::Left => ab.rc.right = ab.rc.left + thickness,
            Edge::Right => ab.rc.left = ab.rc.right - thickness,
        }

        let got = from_win_rect(ab.rc);
        if self.applied == Some((edge, got)) {
            return Some(got);
        }

        unsafe { SHAppBarMessage(ABM_SETPOS, &raw mut ab) };
        self.applied = Some((edge, got));
        Some(got)
    }

    /// Deregister and give the work area back. Idempotent.
    pub fn remove(&mut self) {
        if !self.registered {
            return;
        }
        let mut ab = self.base();
        unsafe { SHAppBarMessage(ABM_REMOVE, &raw mut ab) };
        self.registered = false;
        self.applied = None;
    }

    fn base(&self) -> APPBARDATA {
        APPBARDATA {
            cbSize: u32::try_from(std::mem::size_of::<APPBARDATA>()).unwrap_or(40),
            hWnd: self.hwnd,
            ..Default::default()
        }
    }
}

impl Drop for AppBar {
    fn drop(&mut self) {
        // Not deregistering leaves the desktop short one strip until reboot.
        self.remove();
    }
}

fn to_win_rect(r: Rect) -> RECT {
    RECT {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

fn from_win_rect(r: RECT) -> Rect {
    Rect::new(r.left, r.top, r.right, r.bottom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autohide_taskbar_on_the_same_edge_blocks_reservation() {
        let tb = Taskbar {
            edge: Edge::Bottom,
            autohide: true,
        };
        assert!(!should_reserve(true, true, Edge::Bottom, Some(tb)));
        // A different edge does not conflict, reserve as usual.
        assert!(should_reserve(true, true, Edge::Left, Some(tb)));
    }

    #[test]
    fn visible_taskbar_does_not_block_reservation() {
        let tb = Taskbar {
            edge: Edge::Bottom,
            autohide: false,
        };
        assert!(should_reserve(true, true, Edge::Bottom, Some(tb)));
    }

    #[test]
    fn unreadable_taskbar_keeps_the_old_behaviour() {
        assert!(should_reserve(true, true, Edge::Bottom, None));
        assert!(!should_reserve(false, true, Edge::Bottom, None));
        assert!(!should_reserve(true, false, Edge::Bottom, None));
    }

    /// Read-only: prints the taskbar edge and autohide state.
    #[test]
    #[ignore = "queries the live taskbar; run with --ignored"]
    fn taskbar_state_is_readable() {
        let t = taskbar().expect("ABM_GETTASKBARPOS failed");
        eprintln!("taskbar: edge={:?} autohide={}", t.edge, t.autohide);
    }

    #[test]
    fn rect_roundtrips_through_win_rect() {
        let r = Rect::new(-100, 20, 300, 1040);
        assert_eq!(from_win_rect(to_win_rect(r)), r);
    }

    /// `remove` must be a no-op before registration.
    ///
    /// `Drop` calls it unconditionally; without the guard a never-registered
    /// instance would send `ABM_REMOVE` and could hit another AppBar's slot.
    #[test]
    fn remove_is_idempotent_before_new() {
        // An invalid handle: nothing crashes as long as SHAppBarMessage is not
        // reached.
        let mut bar = AppBar::new(HWND::default());
        bar.remove();
        bar.remove();
        assert!(!bar.registered);
    }
}
