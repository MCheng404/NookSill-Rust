//! What counts as an app window, plus find, activate and close by exe.
//! Minimized windows are kept because `activate` restores them.
//! A `WS_EX_NOACTIVATE` dock cannot raise windows, so activation escalates.

use std::collections::HashMap;

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    keybd_event, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetAncestor, GetClassNameW, GetForegroundWindow, GetWindow,
    GetWindowLongW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow, WindowFromPoint, GA_ROOT,
    GWL_EXSTYLE, GW_OWNER, SW_MINIMIZE, SW_RESTORE, WS_EX_TOOLWINDOW,
};

/// Host process of UWP apps.
///
/// Settings, Calculator and Photos all run inside `ApplicationFrameHost.exe`, so
/// exe matching would merge them; telling them apart needs
/// `GetApplicationUserModelId` from `Win32_System_ApplicationModel`.
const UWP_HOST: &str = "applicationframehost.exe";

/// Whether this window belongs to a normal app.
///
/// Minimized windows are kept on purpose: they are what [`activate`] restores.
#[must_use]
pub fn is_app_window(hwnd: HWND) -> bool {
    // SAFETY: read-only queries on handles from enumeration or the caller.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
        // An owned window is a dialog or popup host.
        if GetWindow(hwnd, GW_OWNER).is_ok() {
            return false;
        }
        if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return false;
        }
        // Untitled windows are nearly always invisible containers.
        if GetWindowTextLengthW(hwnd) == 0 {
            return false;
        }
        if crate::occlusion::is_cloaked(hwnd) {
            return false;
        }
        if is_desktop_host(hwnd) {
            return false;
        }
    }
    true
}

/// Desktop and taskbar windows: full screen but not applications.
fn is_desktop_host(hwnd: HWND) -> bool {
    let mut buf = [0u16; 64];
    // SAFETY: the output buffer is a local array.
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    if n <= 0 {
        return false;
    }
    let cls = String::from_utf16_lossy(&buf[..n as usize]);
    matches!(cls.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd")
}

/// Map a process id to its executable path.
///
/// `PROCESS_QUERY_LIMITED_INFORMATION` opens higher-integrity processes that
/// `PROCESS_QUERY_INFORMATION` refuses, so a normal-user dock still sees an app
/// running elevated.
#[must_use]
pub fn exe_of_pid(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    // SAFETY: the handle is closed right below and `buf` is a local buffer.
    unsafe {
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let r = QueryFullProcessImageNameW(
            proc,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &raw mut len,
        );
        let _ = CloseHandle(proc);
        r.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Full executable path of the window's process.
#[must_use]
pub fn exe_of_window(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    // SAFETY: the output parameter is a local u32.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };
    exe_of_pid(pid)
}

/// One enumeration's context.
///
/// `by_pid` caches pid to exe for this pass: an app with five windows otherwise
/// costs five `OpenProcess` calls, and the scan runs every 1.2 s.
struct Collect {
    map: HashMap<String, Vec<AppWindow>>,
    by_pid: HashMap<u32, Option<String>>,
}

impl Collect {
    fn exe_at(&mut self, hwnd: HWND) -> Option<String> {
        let mut pid = 0u32;
        // SAFETY: the output parameter is a local u32.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };
        if let Some(hit) = self.by_pid.get(&pid) {
            return hit.clone();
        }
        let exe = exe_of_pid(pid);
        self.by_pid.insert(pid, exe.clone());
        exe
    }
}

/// Bring `hwnd` to the front, restoring it first when minimised.
///
/// Returns false when Windows refuses the foreground change (focus stealing
/// rules); the caller has nothing better to try.
pub fn activate_window(hwnd: isize) -> bool {
    let h = HWND(hwnd as *mut core::ffi::c_void);
    if h.is_invalid() {
        return false;
    }
    // SAFETY: `h` came from `EnumWindows`; a dead window simply fails.
    unsafe {
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        SetForegroundWindow(h).as_bool()
    }
}

/// One top-level window of an app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppWindow {
    /// Raw `HWND` as an integer; the caller casts back.
    pub hwnd: isize,
    pub title: String,
}

/// Every app window grouped by lowercase exe path, in `EnumWindows` (Z) order.
#[must_use]
pub fn running_windows() -> HashMap<String, Vec<AppWindow>> {
    let mut ctx = Collect {
        map: HashMap::new(),
        by_pid: HashMap::new(),
    };
    // SAFETY: `lparam` points at `ctx` on this frame and `EnumWindows` is
    // synchronous.
    unsafe {
        let _ = EnumWindows(Some(collect_proc), LPARAM((&raw mut ctx) as isize));
    }
    ctx.map
}

/// Enumerate app windows as `lowercase exe path -> first window`.
///
/// Keys are lowercase because Windows paths are case-insensitive and APIs differ
/// in casing. "First" is `EnumWindows` order, top-down in Z order, which is the
/// app's topmost window and the right click target.
#[must_use]
pub fn running_apps() -> HashMap<String, HWND> {
    running_windows()
        .into_iter()
        .filter_map(|(exe, windows)| {
            let first = windows.first()?;
            Some((exe, HWND(first.hwnd as *mut core::ffi::c_void)))
        })
        .collect()
}

unsafe extern "system" fn collect_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    // SAFETY: see `running_apps`.
    let ctx = unsafe { &mut *(lparam.0 as *mut Collect) };
    if !is_app_window(hwnd) {
        return true.into();
    }
    let Some(exe) = ctx.exe_at(hwnd) else {
        return true.into();
    };
    if is_uwp_host(&exe) {
        return true.into();
    }
    // Every window is kept: the menu offers the others when an app has more
    // than one. `running_apps` still takes the first for plain activation.
    ctx.map
        .entry(exe.to_lowercase())
        .or_default()
        .push(AppWindow {
            hwnd: hwnd.0 as isize,
            title: window_title(hwnd),
        });
    true.into()
}

/// Whether this is the UWP host process.
fn is_uwp_host(exe: &str) -> bool {
    std::path::Path::new(exe)
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case(UWP_HOST))
}

/// Find the window of `exe`.
#[must_use]
pub fn find_window_of(exe: &str) -> Option<HWND> {
    let want = exe.to_lowercase();
    running_apps().get(&want).copied()
}

/// Outcome of [`activate`].
///
/// [`Activation::is_running`] decides whether a new instance must be started; the
/// other variants only feed logs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    /// No window found; the caller should launch.
    NotRunning,
    /// Found and brought to the foreground.
    Foreground,
    /// Restored from minimized and brought to the foreground.
    Restored,
    /// Was already in front, so `toggle` minimized it.
    Minimized,
    /// The window exists but the foreground lock refused the switch. Still
    /// running, so do not launch a second instance.
    Refused,
}

impl Activation {
    /// Whether the app counts as running (including a refused activation).
    #[must_use]
    pub const fn is_running(self) -> bool {
        !matches!(self, Self::NotRunning)
    }
}

/// Activate, or with `toggle` minimize, the window of `exe`.
///
/// Toggle semantics: minimized restores, already in front minimizes, covered
/// raises. Restore and minimize play no transition animation.
pub fn activate(exe: &str, toggle: bool) -> Activation {
    let Some(hwnd) = find_window_of(exe) else {
        return Activation::NotRunning;
    };
    // SAFETY: hwnd comes from the system enumeration and is a valid top-level
    // window.
    unsafe {
        let minimized = IsIconic(hwnd).as_bool();
        if trace() {
            eprintln!(
                "[activate] 目标 {:?}（exe={exe}）最小化={minimized} 前台={} 同线程={}",
                window_title(hwnd),
                window_title(GetForegroundWindow()),
                in_front(hwnd)
            );
        }
        if minimized {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        // Already in front: minimize, toggle only. The test is [`in_front`], not
        // a handle compare: multi-window apps often have a different window in
        // front, so a handle compare would never minimize them.
        if toggle && !minimized && in_front(hwnd) {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
            return Activation::Minimized;
        }
        if force_foreground(hwnd) {
            return if minimized {
                Activation::Restored
            } else {
                Activation::Foreground
            };
        }
    }
    Activation::Refused
}

/// Whether `hwnd` is "in front", so a click should minimize it.
///
/// True if it is the foreground window, shares a process with it (a multi-window
/// app raises a sibling), or is unobstructed. The last rule covers
/// `GetForegroundWindow` being unreliable: it can be the desktop, a UIPI-blocked
/// elevated window, or some window that took focus after the click.
#[must_use]
pub fn in_front(hwnd: HWND) -> bool {
    // SAFETY: all read-only queries.
    unsafe {
        let fg = GetForegroundWindow();
        if fg == hwnd {
            return true;
        }
        if !fg.is_invalid() {
            let a = GetWindowThreadProcessId(fg, None);
            let b = GetWindowThreadProcessId(hwnd, None);
            if a != 0 && a == b {
                return true;
            }
        }
    }
    unobstructed(hwnd)
}

/// Whether the window is not covered at its center point.
///
/// `WindowFromPoint` returns the topmost window there, often a child (button,
/// edit), so its root window is compared instead. The center is used because a
/// corner of a rounded window can fall outside it.
#[must_use]
pub fn unobstructed(hwnd: HWND) -> bool {
    // SAFETY: all read-only; `RECT` and `POINT` are locals on this frame.
    unsafe {
        if hwnd.is_invalid() || IsIconic(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &raw mut r).is_err() {
            return false;
        }
        if r.right <= r.left || r.bottom <= r.top {
            return false;
        }
        let pt = POINT {
            x: (r.left + r.right) / 2,
            y: (r.top + r.bottom) / 2,
        };
        let hit = WindowFromPoint(pt);
        !hit.is_invalid() && GetAncestor(hit, GA_ROOT) == hwnd
    }
}

/// Whether `exe`'s window is already in front ([`find_window_of`] + [`in_front`]).
///
/// For probes and logging; callers usually only need "should I minimize".
#[must_use]
pub fn in_front_of(exe: &str) -> bool {
    find_window_of(exe).is_some_and(in_front)
}

/// Window title (diagnostics; empty when unavailable).
fn window_title(hwnd: HWND) -> String {
    if hwnd.is_invalid() {
        return String::new();
    }
    let n = unsafe { GetWindowTextLengthW(hwnd) };
    if n <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; n as usize + 1];
    let got = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..got.max(0) as usize])
}

/// `NOOKSILL_LAUNCH_TRACE=1` prints activation decisions (who is in front, whether
/// the target is minimized).
fn trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NOOKSILL_LAUNCH_TRACE").is_ok_and(|v| v.trim() != "0"))
}

/// Bring `hwnd` to the foreground, escalating. Returns whether it really got there.
///
/// Level 1 is plain `SetForegroundWindow`, usually refused for a
/// `WS_EX_NOACTIVATE` dock. Level 2 attaches to the foreground thread's input
/// queue so the caller counts as the foreground thread, and must detach at once
/// or both queues mix. Level 3 taps Alt: "the user just pressed a key" satisfies
/// the foreground lock, and a bare Alt sends no character. Each level re-checks
/// `GetForegroundWindow`; without the check the call may have done nothing.
unsafe fn force_foreground(hwnd: HWND) -> bool {
    // SAFETY: only window and thread APIs, with handles supplied by the caller.
    unsafe {
        if GetForegroundWindow() == hwnd {
            return true;
        }
        let _ = SetForegroundWindow(hwnd);
        if GetForegroundWindow() == hwnd {
            return true;
        }

        let fg = GetForegroundWindow();
        let fg_tid = if fg.is_invalid() {
            0
        } else {
            GetWindowThreadProcessId(fg, None)
        };
        let me = GetCurrentThreadId();
        let attached = fg_tid != 0 && fg_tid != me && AttachThreadInput(me, fg_tid, true).as_bool();
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        if attached {
            let _ = AttachThreadInput(me, fg_tid, false);
        }
        if GetForegroundWindow() == hwnd {
            return true;
        }

        // Last resort: Alt down and up. The extended-key flag is the keybd_event
        // convention; VK_MENU works without it but this matches a real keyboard.
        keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_EXTENDEDKEY, 0);
        keybd_event(
            VK_MENU.0 as u8,
            0,
            KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP,
            0,
        );
        let _ = SetForegroundWindow(hwnd);
        GetForegroundWindow() == hwnd
    }
}

struct CloseCtx {
    /// Target exe, lowercase.
    want: String,
    found: bool,
    by_pid: HashMap<u32, Option<String>>,
}

/// Make the app exit by posting `WM_CLOSE` to each of its windows.
///
/// Returns false when no window was found. Posting to several windows is
/// deliberate: a multi-window app does not exit when one window closes, and which
/// windows count as "closed" is the app's own semantics.
pub fn close_app(exe: &str) -> bool {
    let mut ctx = CloseCtx {
        want: exe.to_lowercase(),
        found: false,
        by_pid: HashMap::new(),
    };
    // SAFETY: `lparam` points at `ctx` on this frame and `EnumWindows` is
    // synchronous.
    unsafe {
        let _ = EnumWindows(Some(close_proc), LPARAM((&raw mut ctx) as isize));
    }
    ctx.found
}

unsafe extern "system" fn close_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    // SAFETY: see `close_app`.
    let ctx = unsafe { &mut *(lparam.0 as *mut CloseCtx) };
    if !is_app_window(hwnd) {
        return true.into();
    }
    let mut pid = 0u32;
    // SAFETY: the output parameter is a local u32.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };
    let exe = match ctx.by_pid.get(&pid) {
        Some(hit) => hit.clone(),
        None => {
            let e = exe_of_pid(pid);
            ctx.by_pid.insert(pid, e.clone());
            e
        }
    };
    let Some(exe) = exe else {
        return true.into();
    };
    if exe.to_lowercase() == ctx.want {
        // SAFETY: posts a close message, no local state is touched.
        let _ = unsafe {
            windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                Some(hwnd),
                windows::Win32::UI::WindowsAndMessaging::WM_CLOSE,
                Default::default(),
                Default::default(),
            )
        };
        ctx.found = true;
    }
    true.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The desktop window is not an app window; this guards `is_desktop_host`.
    #[test]
    fn desktop_window_is_not_an_app_window() {
        let desk = unsafe { windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow() };
        assert!(!is_app_window(desk), "桌面窗口不该算应用窗口");
    }

    /// A missing exe must report not running; `NotRunning` makes the caller
    /// launch, and a false positive would leave the click doing nothing.
    #[test]
    fn unknown_exe_is_not_running() {
        let key = r"C:\definitely\not\a\real\app.exe";
        assert!(find_window_of(key).is_none());
        let r = activate(key, false);
        assert_eq!(r, Activation::NotRunning);
        assert!(!r.is_running());
    }

    /// UWP hosts must be recognised. Otherwise every UWP window shares one exe:
    /// clicking Calculator would switch to Settings.
    #[test]
    fn uwp_host_is_recognised() {
        assert!(is_uwp_host(r"C:\Windows\System32\ApplicationFrameHost.exe"));
        assert!(is_uwp_host(r"c:\windows\system32\applicationframehost.EXE"));
        assert!(!is_uwp_host(r"C:\Windows\explorer.exe"));
    }

    /// Keys of `running_apps` must be lowercase. [`find_window_of`] lowercases its
    /// input, so a mixed-case key would be missed and the indicator never lights.
    #[test]
    fn running_apps_keys_are_lowercase() {
        for key in running_apps().keys() {
            assert_eq!(*key, key.to_lowercase(), "键 {key} 不是小写");
        }
    }

    /// Desktop and tool windows must not be listed, otherwise the running
    /// indicator lights with nothing open (the desktop exe is explorer.exe).
    #[test]
    fn desktop_hosts_are_not_listed_as_running() {
        for exe in running_apps().keys() {
            let name = exe.rsplit(['\\', '/']).next().unwrap_or("");
            assert!(
                !name.eq_ignore_ascii_case("progman.exe"),
                "桌面宿主不该出现在运行列表里"
            );
            assert!(
                !name.eq_ignore_ascii_case(UWP_HOST),
                "UWP 宿主不该出现在运行列表里"
            );
        }
    }

    /// `exe_of_pid(0)` returns `None` instead of opening a null handle.
    #[test]
    fn pid_zero_has_no_exe() {
        assert!(exe_of_pid(0).is_none());
    }
}
