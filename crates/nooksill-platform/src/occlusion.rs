//! Occlusion detection for smart hide: whether a normal window covers a rect.
//! `WindowFromPoint` returns the dock itself (layered windows win hit testing),
//! so windows are enumerated and filtered instead.

use nooksill_core::layout::Rect;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindow, GetWindowLongW, GetWindowRect,
    GetWindowTextLengthW, IsIconic, IsWindowVisible, IsZoomed, GWL_EXSTYLE, GW_OWNER,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

/// Whether the foreground window is maximized. Distinct from
/// [`is_fullscreen_foreground`]: a maximized window only feeds the auto-hide
/// sensitivity, it never hides the dock. Any failure returns `false`.
#[must_use]
pub fn maximized_foreground() -> bool {
    // SAFETY: both are read-only queries; `IsZoomed` returns FALSE for a stale
    // handle, but the validity check comes first anyway.
    unsafe {
        let hwnd = GetForegroundWindow();
        !hwnd.is_invalid() && IsZoomed(hwnd).as_bool()
    }
}

/// Whether DWM has cloaked the window.
///
/// Public because `app_window` needs it too: a cloaked UWP ghost is neither an
/// occluder nor a running app.
#[must_use]
pub fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked: i32 = 0;
    // SAFETY: the third argument points at a local i32 and the fourth is its size.
    let ok = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&raw mut cloaked).cast(),
            u32::try_from(std::mem::size_of::<i32>()).unwrap_or(4),
        )
    };
    ok.is_ok() && cloaked != 0
}

/// Whether this window must be excluded from the occlusion check.
fn should_skip(hwnd: HWND, exclude: &[HWND]) -> bool {
    if exclude.contains(&hwnd) {
        return true;
    }
    // SAFETY: read-only queries on handles from the system enumeration.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return true;
        }
        // Tool windows and no-activate overlays: IME candidates, floating
        // toolbars. They float on top but must not hide the dock.
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) != 0 {
            return true;
        }
        // An owned window is a dialog or popup host, not a standalone app.
        if GetWindow(hwnd, GW_OWNER).is_ok() {
            return true;
        }
        // Untitled windows are nearly always invisible containers. This also
        // skips a few untitled fullscreen surfaces (games, players), which is
        // acceptable: not hiding beats hiding for no reason.
        if GetWindowTextLengthW(hwnd) == 0 {
            return true;
        }
        if is_cloaked(hwnd) {
            return true;
        }
        // Desktop and taskbar cover the screen and would always count.
        let mut buf = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut buf);
        if n > 0 {
            let cls = String::from_utf16_lossy(&buf[..n as usize]);
            if matches!(cls.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd") {
                return true;
            }
        }
    }
    false
}

fn window_rect(hwnd: HWND) -> Option<Rect> {
    let mut r = RECT::default();
    // SAFETY: the output buffer is a local RECT.
    unsafe { GetWindowRect(hwnd, &raw mut r) }.ok()?;
    Some(Rect::new(r.left, r.top, r.right, r.bottom))
}

/// Whether two rects overlap with positive area.
///
/// Touching edges do not count: a maximized window above the dock ends exactly
/// where the dock begins and must not be treated as coverage.
#[must_use]
pub fn overlaps(a: Rect, b: Rect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

struct Ctx<'a> {
    target: Rect,
    exclude: &'a [HWND],
    hit: bool,
    /// Covering window title (only filled on the diagnostic path).
    culprit: Option<String>,
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    // SAFETY: `lparam` points at a `Ctx` on the caller's stack and `EnumWindows`
    // runs synchronously, so the reference stays valid for the whole callback.
    let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
    if should_skip(hwnd, ctx.exclude) {
        return true.into();
    }
    let Some(r) = window_rect(hwnd) else {
        return true.into();
    };
    if overlaps(r, ctx.target) {
        ctx.hit = true;
        ctx.culprit = window_title(hwnd);
        // Return FALSE to stop the enumeration early.
        return false.into();
    }
    true.into()
}

/// Whether a normal window covers `target` (screen coordinates, physical pixels).
///
/// `exclude` holds our own windows (dock, settings, preview); without it the dock
/// covers itself and the answer is always true.
#[must_use]
pub fn any_window_covers(target: Rect, exclude: &[HWND]) -> bool {
    if target.width() <= 0 || target.height() <= 0 {
        return false;
    }
    let mut ctx = Ctx {
        target,
        exclude,
        hit: false,
        culprit: None,
    };
    // SAFETY: see `enum_proc`; the enumeration is synchronous and the stack
    // pointer stays valid for the call.
    let _ = unsafe { EnumWindows(Some(enum_proc), LPARAM((&raw mut ctx) as isize)) };
    ctx.hit
}

fn window_title(hwnd: HWND) -> Option<String> {
    let mut buf = [0u16; 128];
    // SAFETY: the output buffer is a local array.
    let n = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut buf) };
    if n > 0 {
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    } else {
        None
    }
}

/// Diagnostic variant: `(covered, title of the covering window)`.
///
/// Runs the same filtering path as [`any_window_covers`], so the name it reports
/// cannot drift from the real verdict.
#[must_use]
pub fn covering_window(target: Rect, exclude: &[HWND]) -> (bool, Option<String>) {
    let mut ctx = Ctx {
        target,
        exclude,
        hit: false,
        culprit: None,
    };
    // SAFETY: as in `any_window_covers`; synchronous enumeration, valid stack
    // pointer.
    let _ = unsafe { EnumWindows(Some(enum_proc), LPARAM((&raw mut ctx) as isize)) };
    (ctx.hit, ctx.culprit)
}

/// Result of a foreground fullscreen check, including the evidence.
///
/// `verdict` is the answer the dock needs; the other fields exist for
/// diagnostics. Sharing one struct keeps [`is_fullscreen_foreground`] and
/// [`fullscreen_trace`] from ever disagreeing.
#[derive(Debug)]
pub struct FullscreenCheck {
    /// Foreground handle (diagnostics).
    pub hwnd: isize,
    /// Foreground window class name.
    pub class: String,
    /// Foreground window rect.
    pub rect: Rect,
    /// Full area of the monitor the foreground window is on.
    pub monitor: Rect,
    /// Whether the window has a caption.
    pub has_caption: bool,
    /// Whether it is the desktop or shell window.
    pub is_shell: bool,
    /// Final verdict: fullscreen without a caption.
    pub verdict: bool,
}

/// Query the foreground window once and return the fullscreen verdict.
#[must_use]
pub fn check_fullscreen() -> FullscreenCheck {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowLongW, GWL_STYLE,
        WS_CAPTION,
    };

    // SAFETY: all read-only queries; output buffers are local structs.
    unsafe {
        let fg = GetForegroundWindow();
        let mut cls = [0u16; 64];
        let n = GetClassNameW(fg, &mut cls);
        let class = String::from_utf16_lossy(&cls[..n.max(0) as usize]);
        let rect = window_rect(fg).unwrap_or(Rect::new(0, 0, 0, 0));

        let mut mi = MONITORINFO {
            cbSize: u32::try_from(std::mem::size_of::<MONITORINFO>()).unwrap_or(40),
            ..Default::default()
        };
        let _ = GetMonitorInfoW(MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST), &raw mut mi);
        let m = mi.rcMonitor;
        let monitor = Rect::new(m.left, m.top, m.right, m.bottom);

        let style = GetWindowLongW(fg, GWL_STYLE) as u32;
        let has_caption = style & WS_CAPTION.0 != 0;
        let is_shell = fg.is_invalid() || fg == GetShellWindow() || fg == GetDesktopWindow();

        // All four must hold: not shell, not a desktop host, covering the whole
        // monitor, and no caption.
        let covers = rect.left <= monitor.left
            && rect.top <= monitor.top
            && rect.right >= monitor.right
            && rect.bottom >= monitor.bottom;
        let desktop_host = matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd");
        let verdict = !is_shell && !desktop_host && covers && !has_caption;

        FullscreenCheck {
            hwnd: fg.0 as isize,
            class,
            rect,
            monitor,
            has_caption,
            is_shell,
            verdict,
        }
    }
}

/// Grace period after launching an app, in milliseconds.
///
/// A freshly launched app often appears maximized or fullscreen first (browsers,
/// game launchers). Judging fullscreen at that moment would hide the dock and
/// bring it back a second later, a visible flicker.
pub const LAUNCH_GRACE_MS: u64 = 1500;

/// Whether the launch grace period is still running.
///
/// Both values are milliseconds since process start. A clock that appears to go
/// backwards counts as inside the grace period.
pub fn in_launch_grace(last_launch_ms: Option<u64>, now_ms: u64) -> bool {
    last_launch_ms.is_some_and(|t| now_ms.saturating_sub(t) < LAUNCH_GRACE_MS)
}

/// Whether the foreground window is fullscreen without a caption (video, games,
/// fullscreen slides).
///
/// Such a window hides the dock completely; a strip of dock under fullscreen
/// content is glaring. The monitor rect used is `rcMonitor`, not `rcWork`,
/// because fullscreen video covers the taskbar.
///
/// The missing `WS_CAPTION` is the deciding test: a merely maximized browser
/// must not hide the dock, or users think the dock crashed.
#[must_use]
pub fn is_fullscreen_foreground() -> bool {
    check_fullscreen().verdict
}

/// One-line diagnostic, only called when `NOOKSILL_FS_TRACE` is set.
#[must_use]
pub fn fullscreen_trace() -> String {
    let c = check_fullscreen();
    format!(
        "fg={:#x} cls={:?} rect={:?} mon=({},{},{},{}) caption={} shell={}",
        c.hwnd,
        c.class,
        c.rect,
        c.monitor.left,
        c.monitor.top,
        c.monitor.right,
        c.monitor.bottom,
        c.has_caption,
        c.is_shell
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Grace period edges: never launched, exactly 1500 ms, clock going back.
    #[test]
    fn launch_grace_covers_the_edges() {
        // Never launched: not in the grace period, judge normally.
        assert!(!in_launch_grace(None, 10_000));
        // Just launched: inside.
        assert!(in_launch_grace(Some(1_000), 1_000));
        assert!(in_launch_grace(Some(1_000), 2_499));
        // At exactly 1500 ms the check resumes (not <=, or it hides one extra
        // second).
        assert!(!in_launch_grace(Some(1_000), 2_500));
        // Clock going backwards counts as inside the grace period.
        assert!(in_launch_grace(Some(5_000), 4_000));
    }

    #[test]
    fn overlap_requires_positive_area() {
        let bar = Rect::new(100, 900, 500, 1000);
        // Fully inside.
        assert!(overlaps(Rect::new(200, 950, 300, 990), bar));
        // Partial.
        assert!(overlaps(Rect::new(0, 0, 150, 950), bar));
        // Touching only: the bottom edge equals the top edge, not coverage.
        assert!(!overlaps(Rect::new(0, 0, 1920, 900), bar));
        // Touching the right edge only.
        assert!(!overlaps(Rect::new(500, 900, 900, 1000), bar));
        // Fully apart.
        assert!(!overlaps(Rect::new(0, 0, 50, 50), bar));
        // Fully containing.
        assert!(overlaps(Rect::new(0, 0, 1920, 1080), bar));
    }

    /// A zero-area target returns false without enumerating.
    #[test]
    fn empty_target_is_never_covered() {
        assert!(!any_window_covers(Rect::new(0, 0, 0, 100), &[]));
        assert!(!any_window_covers(Rect::new(0, 0, 100, 0), &[]));
    }

    /// An offscreen rect is never covered.
    ///
    /// Looks trivial, but it guards against an intersection bug that always
    /// returns true: smart hide would then collapse into "always hidden", which
    /// users read as "the dock never appears" and is hard to trace back here.
    #[test]
    fn offscreen_target_is_not_covered() {
        assert!(!any_window_covers(
            Rect::new(20_000, 20_000, 20_100, 20_100),
            &[]
        ));
        assert!(!any_window_covers(
            Rect::new(-5_000, -5_000, -4_900, -4_900),
            &[]
        ));
    }

    /// The exclude list works: excluded handles are skipped during enumeration.
    ///
    /// Pins only machine-independent facts: both directions of the coverage
    /// test, and that an excluded handle is really skipped (forgetting `exclude`
    /// makes the dock cover itself and hide as soon as it animates).
    #[test]
    fn excluding_everything_yields_no_coverage() {
        let target = Rect::new(0, 0, 1920, 1080);
        assert!(
            overlaps(Rect::new(-10, -10, 1930, 1090), target),
            "覆盖必须判真"
        );
        assert!(
            !overlaps(Rect::new(2000, 0, 2100, 100), target),
            "不相交必须判假"
        );

        let desk = unsafe { windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow() };
        assert!(should_skip(desk, &[desk]), "被排除的句柄必须跳过");
    }
}
