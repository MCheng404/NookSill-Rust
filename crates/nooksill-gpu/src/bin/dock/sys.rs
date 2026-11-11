//! Win32 helpers: message pump, pointer recovery and drop-to-item conversion.

use super::*;

/// Drain the queued messages at once; waking per message makes frames jitter.
///
/// A free function because message handling never touches `App`.
pub(super) fn pump(msg: &mut MSG) -> Pumped {
    let mut saw = false;
    // Whether this batch held a real message; all wakes only interrupt the wait.
    let mut real = false;
    loop {
        let has = unsafe { PeekMessageW(msg, None, 0, 0, PM_REMOVE) };
        if !has.as_bool() {
            break;
        }
        saw = true;
        if msg.message == WM_QUIT {
            return Pumped::Quit;
        }
        if msg.message != WM_APP_WAKE {
            real = true;
        }
        unsafe {
            let _ = TranslateMessage(msg);
            DispatchMessageW(msg);
        }
    }
    if real {
        Pumped::Frames
    } else if saw {
        Pumped::Wake
    } else {
        Pumped::Idle
    }
}

/// Parse `NOOKSILL_FAKE_MOUSE="u,v"` into a pinned cursor position in
/// dock-internal units. A malformed value means unset.
pub(super) fn fake_mouse_from_env() -> Option<(f32, f32)> {
    let s = std::env::var("NOOKSILL_FAKE_MOUSE").ok()?;
    let (a, b) = s.split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// `NOOKSILL_FAKE_COVERED=0/1`: override the smart-hide occlusion verdict.
pub(super) fn fake_covered_from_env() -> Option<bool> {
    let s = std::env::var("NOOKSILL_FAKE_COVERED").ok()?;
    match s.trim() {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

/// With `NOOKSILL_GEOM=1`, print metrics and slots to locate geometry drift.
pub(super) fn dump_geometry(dock: &Dock, dpi: i32) {
    if !trace::geom() {
        return;
    }
    let m = &dock.metrics;
    eprintln!(
        "GEOM dpi={dpi} S={:.1} Z={:.1} g={:.1} sep={:.1} pad={:.1} padX={:.1} barH={:.1} \
         over={:.1} margin={:.1} clockW={} clockExtra={:.1} reflH={:.1} lw={} lh={}",
        m.s,
        m.z,
        m.gap,
        m.sep_w,
        m.pad,
        m.pad_x,
        m.bar_h,
        m.overhang,
        m.margin,
        m.clock_w,
        m.widget_extra,
        m.refl_h,
        m.lw,
        m.lh
    );
    eprintln!(
        "GEOM bar_l={:.1} bar_w={:.1} bar_top={:.1} bar_bottom={:.1} icon_bottom={:.1}",
        dock.geom.bar_l,
        dock.geom.bar_w,
        dock.geom.bar_top,
        dock.geom.bar_bottom,
        dock.geom.icon_bottom
    );
    eprintln!("GEOM slot_l={:?}", dock.geom.slot_l);
}

/// Local time as `core` structs, keeping Win32 out of `core`.
pub(super) fn now_parts() -> clock::ClockParts {
    let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    clock::ClockParts {
        year: st.wYear,
        month: st.wMonth as u8,
        day: st.wDay as u8,
        weekday: st.wDayOfWeek as u8,
        hour: st.wHour as u8,
        minute: st.wMinute as u8,
        second: st.wSecond as u8,
    }
}

/// Dock pointer from the window data; empty when unset, so callers check.
pub(super) fn dock_ptr(hwnd: HWND) -> *mut Dock {
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Dock }
}

/// Turn one dropped path into an item: folders, `.url` shortcuts and files.
///
/// `.lnk` needs no special handling (`ShellExecute` resolves it). A `.url` must
/// become `ItemKind::Url` or it would be matched against running exes; a failed
/// parse falls back to a file item so it still opens.
pub(super) fn item_from_drop(path: &str) -> Option<Item> {
    if path.is_empty() {
        return None;
    }
    let p = std::path::Path::new(path);
    let name = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| path.to_owned());

    if p.is_dir() {
        return Some(Item::new(ItemKind::Folder, &name, path));
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ext == "url" {
        if let Some(url) = nooksill_platform::shortcut::read_url_shortcut(path) {
            let mut item = Item::new(ItemKind::Url, &name, &url);
            // No favicon support yet; let the shell icon the `.url` file so it
            // does not degrade to the initial glyph.
            item.icon_src = path.to_owned();
            return Some(item);
        }
    }
    Some(Item::new(ItemKind::File, &name, path))
}

/// Client coordinates; `WM_*MOUSE*` already supplies physical pixels.
///
/// Do not divide by ppp: dock geometry is physical too, or hit tests shift by
/// one DPI factor.
pub(super) fn physical_xy(lparam: LPARAM) -> (f32, f32) {
    // Mouse coordinates are signed 16-bit, including while capture is active.
    let x = (lparam.0 & 0xFFFF) as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
    (x, y)
}

/// Request one `WM_MOUSELEAVE`; the system sends it once, so re-arm on each move.
pub(super) fn arm_mouse_leave(hwnd: HWND) {
    let mut tme = TRACKMOUSEEVENT {
        cbSize: u32::try_from(std::mem::size_of::<TRACKMOUSEEVENT>()).unwrap_or(0),
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    unsafe {
        let _ = TrackMouseEvent(&raw mut tme);
    }
}
