//! Monitor enumeration and work areas.
//!
//! The primary monitor is always first so the `Monitor` config index stays
//! stable. When enumeration fails the fallback is the monitor containing
//! (0,0), not an error: the dock must always start.

use nooksill_core::layout::Rect;
use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, HDC, HMONITOR, MONITORINFO,
    MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

/// A single monitor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Monitor {
    /// Native handle, used only for comparison and debugging.
    pub handle: isize,
    /// Full area, taskbar included.
    pub rect: Rect,
    /// Work area, taskbar excluded.
    pub work: Rect,
    pub primary: bool,
}

unsafe extern "system" fn enum_proc(
    monitor: HMONITOR,
    _dc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    // SAFETY: `data` comes from `monitors` and points at a live Vec<HMONITOR>;
    // the callback runs synchronously inside `EnumDisplayMonitors`.
    let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
    list.push(monitor);
    BOOL(1)
}

fn info_of(monitor: HMONITOR) -> Option<(Rect, Rect, bool)> {
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut mi) }.as_bool() {
        return None;
    }
    let r = mi.rcMonitor;
    let w = mi.rcWork;
    Some((
        Rect::new(r.left, r.top, r.right, r.bottom),
        Rect::new(w.left, w.top, w.right, w.bottom),
        mi.dwFlags & MONITORINFOF_PRIMARY != 0,
    ))
}

/// Enumerate all monitors, primary first.
#[must_use]
pub fn monitors() -> Vec<Monitor> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    let ok = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(enum_proc),
            LPARAM(std::ptr::addr_of_mut!(handles) as isize),
        )
    };
    if !ok.as_bool() && handles.is_empty() {
        // Degenerate sessions (0 or a remote desktop) still get the primary.
        let fallback = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        if let Some((rect, work, _)) = info_of(fallback) {
            return vec![Monitor {
                handle: fallback.0 as isize,
                rect,
                work,
                primary: true,
            }];
        }
        return Vec::new();
    }

    let mut out: Vec<Monitor> = handles
        .into_iter()
        .filter_map(|h| {
            info_of(h).map(|(rect, work, primary)| Monitor {
                handle: h.0 as isize,
                rect,
                work,
                primary,
            })
        })
        .collect();

    // Primary first, the rest keeps the system order.
    if let Some(pos) = out.iter().position(|m| m.primary) {
        out.swap(0, pos);
    }
    out
}

/// Work area of monitor `index`; out of range falls back to primary, then 1080p.
#[must_use]
pub fn work_area(index: i32) -> Rect {
    let list = monitors();
    if list.is_empty() {
        return Rect::new(0, 0, 1920, 1040);
    }
    let idx = if index >= 0 && (index as usize) < list.len() {
        index as usize
    } else {
        0
    };
    list[idx].work
}

/// Full area of monitor `index`, taskbar included.
///
/// AppBar matters here: `SHAppBarMessage` reserves an edge on the whole screen,
/// not inside `rcWork`. Passing the work area subtracts the taskbar twice and
/// yields a dock about half as tall as intended.
#[must_use]
pub fn monitor_rect(index: i32) -> Rect {
    let list = monitors();
    list.first().map_or_else(
        || Rect::new(0, 0, 1920, 1080),
        |_| {
            let idx = if index >= 0 && (index as usize) < list.len() {
                index as usize
            } else {
                0
            };
            list[idx].rect
        },
    )
}

/// System DPI, re-exported from [`crate::win::system_dpi`].
#[must_use]
pub fn system_dpi() -> i32 {
    crate::win::system_dpi()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_least_one_monitor_on_a_desktop() {
        // Needs a desktop session; the Windows CI runner has one.
        let list = monitors();
        assert!(!list.is_empty(), "至少应枚举到一个显示器");
        assert!(list[0].primary || list.len() == 1);
        assert!(list[0].work.width() > 0);
    }

    #[test]
    fn work_area_is_within_monitor() {
        let list = monitors();
        if list.is_empty() {
            return;
        }
        let m = list[0];
        assert!(m.work.left >= m.rect.left);
        assert!(m.work.top >= m.rect.top);
        assert!(m.work.right <= m.rect.right);
        assert!(m.work.bottom <= m.rect.bottom);
    }

    #[test]
    fn out_of_range_index_falls_back_to_primary() {
        let a = work_area(0);
        let b = work_area(99);
        assert_eq!(a, b);
    }
}
