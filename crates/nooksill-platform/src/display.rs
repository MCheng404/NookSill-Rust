//! Monitor enumeration and work areas.
//!
//! The primary monitor is always first so the `Monitor` config index stays
//! stable. When enumeration fails the fallback is the monitor containing
//! (0,0), not an error: the dock must always start.

use core::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use nooksill_core::layout::Rect;
use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, HDC, HMONITOR, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
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

/// Clamp a config index into `0..len`; index 0 is always the primary monitor.
fn clamp_index(index: i32, len: usize) -> usize {
    if index >= 0 && (index as usize) < len {
        index as usize
    } else {
        0
    }
}

/// Work area of monitor `index`; out of range falls back to primary, then 1080p.
#[must_use]
pub fn work_area(index: i32) -> Rect {
    let list = monitors();
    if list.is_empty() {
        return Rect::new(0, 0, 1920, 1040);
    }
    list[clamp_index(index, list.len())].work
}

/// Full area of monitor `index`, taskbar included.
///
/// AppBar matters here: `SHAppBarMessage` reserves an edge on the whole screen,
/// not inside `rcWork`. Passing the work area subtracts the taskbar twice and
/// yields a dock about half as tall as intended.
#[must_use]
pub fn monitor_rect(index: i32) -> Rect {
    let list = monitors();
    if list.is_empty() {
        return Rect::new(0, 0, 1920, 1080);
    }
    list[clamp_index(index, list.len())].rect
}

/// Full rect of the monitor under `(x, y)`, nearest when the point is outside
/// every monitor.
///
/// Popups clamp against this instead of the primary: the anchor is where the
/// card is, so it names the right screen without threading a config index
/// through every window.
#[must_use]
pub fn monitor_at(x: i32, y: i32) -> Rect {
    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    if let Some((rect, _, _)) = info_of(monitor) {
        return rect;
    }
    monitor_rect(0)
}

/// Work area of the monitor under `(x, y)`, nearest when outside every monitor.
#[must_use]
pub fn work_area_at(x: i32, y: i32) -> Rect {
    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    if let Some((_, work, _)) = info_of(monitor) {
        return work;
    }
    work_area(0)
}

/// Work area of the monitor this process draws its dock on.
///
/// Process-global on purpose: one process runs one dock on one monitor, and the
/// panels that are not anchored to a card (settings, launchpad, notes, group,
/// timers, resource monitor) all centre themselves on it.
static TARGET_WORK: [AtomicI32; 4] = [const { AtomicI32::new(0) }; 4];
static TARGET_SET: AtomicBool = AtomicBool::new(false);

/// Publish the work area the dock occupies; see [`TARGET_WORK`].
///
/// Called at boot and again when `[Settings] Monitor` changes.
pub fn set_target_work_area(rect: Rect) {
    for (slot, v) in TARGET_WORK
        .iter()
        .zip([rect.left, rect.top, rect.right, rect.bottom])
    {
        slot.store(v, Ordering::Relaxed);
    }
    TARGET_SET.store(true, Ordering::Relaxed);
}

/// Work area set by [`set_target_work_area`]; the primary's until then.
#[must_use]
pub fn target_work_area() -> Rect {
    if !TARGET_SET.load(Ordering::Relaxed) {
        return work_area(0);
    }
    Rect::new(
        TARGET_WORK[0].load(Ordering::Relaxed),
        TARGET_WORK[1].load(Ordering::Relaxed),
        TARGET_WORK[2].load(Ordering::Relaxed),
        TARGET_WORK[3].load(Ordering::Relaxed),
    )
}

/// Top-left corner that centres a `w` x `h` window on the dock's monitor.
#[must_use]
pub fn centered_on_target(w: i32, h: i32) -> (i32, i32) {
    nooksill_core::layout::centered_in(target_work_area(), w, h)
}

/// Full rect of the monitor the dock is on, taskbar included.
///
/// The launchpad covers the whole screen, so it must not stop at the work area.
#[must_use]
pub fn target_monitor_rect() -> Rect {
    let work = target_work_area();
    monitor_at((work.left + work.right) / 2, (work.top + work.bottom) / 2)
}

/// Effective DPI of monitor `index`; out of range falls back to primary.
///
/// A per-monitor-v2 process gets the real per-monitor value here; [`system_dpi`]
/// only ever answers for the primary, so a dock sent to a 150% second screen
/// would otherwise keep drawing at 100%.
#[must_use]
pub fn monitor_dpi(index: i32) -> i32 {
    let list = monitors();
    let Some(m) = list.get(clamp_index(index, list.len())) else {
        return system_dpi();
    };
    let (mut x, mut y) = (0u32, 0u32);
    let monitor = HMONITOR(m.handle as *mut c_void);
    // SAFETY: `monitor` came from `EnumDisplayMonitors`; both out-pointers are
    // valid for the call.
    let ok = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &raw mut x, &raw mut y) };
    if ok.is_ok() {
        i32::try_from(x).unwrap_or_else(|_| system_dpi())
    } else {
        system_dpi()
    }
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

    #[test]
    fn every_monitor_reports_a_usable_dpi() {
        let list = monitors();
        for i in 0..list.len() {
            let dpi = monitor_dpi(i as i32);
            // 48 is the smallest scale Windows offers (50%), 480 the largest.
            assert!((48..=480).contains(&dpi), "显示器 {i} DPI 异常: {dpi}");
        }
        // Out of range must clamp instead of returning 0.
        assert_eq!(monitor_dpi(-1), monitor_dpi(0));
        assert_eq!(monitor_dpi(99), monitor_dpi(0));
    }

    /// The dock publishes its monitor here; panels read it back unchanged.
    #[test]
    fn target_work_area_roundtrips() {
        let want = Rect::new(1920, 0, 3840, 1040);
        set_target_work_area(want);
        assert_eq!(target_work_area(), want);
        assert_eq!(centered_on_target(800, 600), (2480, 220));
        // On a single-monitor machine the point is outside every screen, so
        // `target_monitor_rect` answers with the nearest one instead of failing.
        assert!(target_monitor_rect().width() > 0);
        // Leave the process-wide value at the primary for the other tests.
        set_target_work_area(work_area(0));
    }
}
