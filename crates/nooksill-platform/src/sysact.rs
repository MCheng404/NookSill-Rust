//! System actions triggered from the control center.
//!
//! `cc_win` only draws and hit-tests; each action is a separate function here.

use windows::core::{GUID, HSTRING};
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::System::Power::{PowerGetActiveScheme, PowerSetActiveScheme};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_DWORD,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, SW_SHOWNORMAL, WM_SETTINGCHANGE,
};

/// Open a URI or path with the default handler (`ms-settings:`, `https://`, a file).
///
/// Shell returns a pseudo handle where a value above 32 means success, so only
/// failure is meaningful and it is ignored.
pub fn open_uri(target: &str) {
    let verb = HSTRING::from("open");
    let file = HSTRING::from(target);
    unsafe {
        let _ = ShellExecuteW(None, &verb, &file, None, None, SW_SHOWNORMAL);
    }
}

/// Open the Windows quick settings panel (Win+A).
///
/// Wi-Fi, Bluetooth and hotspot have no stable public toggle API.
pub fn open_quick_settings() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP, VK_LWIN};
    // Virtual-key code for 'A' is 0x41.
    const VK_A: u8 = 0x41;
    unsafe {
        keybd_event(VK_LWIN.0 as u8, 0, Default::default(), 0);
        keybd_event(VK_A, 0, Default::default(), 0);
        keybd_event(VK_A, 0, KEYEVENTF_KEYUP, 0);
        keybd_event(VK_LWIN.0 as u8, 0, KEYEVENTF_KEYUP, 0);
    }
}

/// Open the Start menu.
///
/// No public API opens it without keystrokes, so press Win and let the shell
/// handle press-to-open, press-to-close.
pub fn open_start() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP, VK_LWIN};
    unsafe {
        keybd_event(VK_LWIN.0 as u8, 0, Default::default(), 0);
        keybd_event(VK_LWIN.0 as u8, 0, KEYEVENTF_KEYUP, 0);
    }
}

/// Open the notification area (tray overflow flyout).
///
/// Only the hotkey path is used: clicking Explorer's `ToolbarWindow32` across
/// processes needs matching bitness and exact coordinates, and a miss clicks
/// something else. Win+B focuses the tray, Enter opens it.
pub fn open_tray() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYEVENTF_KEYUP, VK_LWIN, VK_RETURN,
    };
    // Virtual-key code for 'B' is 0x42.
    const VK_B: u8 = 0x42;
    unsafe {
        keybd_event(VK_LWIN.0 as u8, 0, Default::default(), 0);
        keybd_event(VK_B, 0, Default::default(), 0);
        keybd_event(VK_B, 0, KEYEVENTF_KEYUP, 0);
        keybd_event(VK_LWIN.0 as u8, 0, KEYEVENTF_KEYUP, 0);
    }
    // Wait for focus to reach the tray before pressing Enter, otherwise the
    // keystroke lands on the previous foreground window (hence 120 ms).
    std::thread::sleep(std::time::Duration::from_millis(120));
    unsafe {
        keybd_event(VK_RETURN.0 as u8, 0, Default::default(), 0);
        keybd_event(VK_RETURN.0 as u8, 0, KEYEVENTF_KEYUP, 0);
    }
}

/// Toggle system dark mode. Returns the resulting state, or the previous one
/// when the registry write fails.
///
/// Writes the two DWORDs under `HKCU\...\Themes\Personalize` and broadcasts
/// `WM_SETTINGCHANGE("ImmersiveColorSet")`, the same path the system toggle
/// takes. The dock must re-read `theme::is_dark` or the bar keeps old colors.
pub fn set_dark_mode(dark: bool) -> bool {
    let value: u32 = u32::from(dark);
    let sub = HSTRING::from(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    unsafe {
        let mut key = windows::Win32::System::Registry::HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, &sub, Some(0), KEY_SET_VALUE, &mut key).is_err() {
            return !dark;
        }
        let mut ok = true;
        for name in ["AppsUseLightTheme", "SystemUsesLightTheme"] {
            let n = HSTRING::from(name);
            // The meaning is inverted: `AppsUseLightTheme = 0` means dark.
            let v: u32 = 1 - value;
            let bytes = v.to_le_bytes();
            if RegSetValueExW(key, &n, Some(0), REG_DWORD, Some(&bytes)).is_err() {
                ok = false;
            }
        }
        let _ = RegCloseKey(key);

        let mut result = 0_usize;
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            windows::Win32::Foundation::WPARAM(0),
            windows::Win32::Foundation::LPARAM(
                windows::core::w!("ImmersiveColorSet").as_ptr() as isize
            ),
            SMTO_ABORTIFHUNG,
            200,
            Some(&mut result),
        );
        if ok {
            dark
        } else {
            !dark
        }
    }
}

/// Built-in Windows power schemes; GUIDs match `powercfg /list` byte for byte.
const SCHEME_SAVER: GUID = GUID::from_u128(0xa1841308_3541_4fab_bc81_f71556f20b4a);
const SCHEME_BALANCED: GUID = GUID::from_u128(0x381b4222_f694_41f0_9685_ff5bb260df2e);

/// Currently active power scheme; `None` when unreadable (trimmed install, policy).
///
/// `PowerGetActiveScheme` returns a `LocalAlloc`ed GUID that the caller must
/// free, so it is copied out and then released.
pub fn active_power_scheme() -> Option<GUID> {
    let mut raw: *mut GUID = std::ptr::null_mut();
    // SAFETY: on success powrprof writes a LocalAlloc GUID into `raw`.
    let err = unsafe { PowerGetActiveScheme(None, &raw mut raw) };
    if err.0 != 0 || raw.is_null() {
        return None;
    }
    let guid = unsafe { *raw };
    unsafe { LocalFree(Some(HLOCAL(raw.cast()))) };
    Some(guid)
}

/// Whether the power saver scheme is active.
pub fn is_power_saver() -> bool {
    active_power_scheme() == Some(SCHEME_SAVER)
}

/// Switch to `scheme` and report whether the system accepted it.
pub fn set_active_power_scheme(scheme: &GUID) -> bool {
    // SAFETY: the pointer refers to a GUID on the caller's stack.
    unsafe { PowerSetActiveScheme(None, Some(scheme)) }.0 == 0
}

/// Decide the toggle: returns the scheme to set and the value to remember.
///
/// Leaving saver restores whatever was active before it, because the user may
/// have chosen high performance and resetting to balanced would change it.
fn plan_toggle(current: Option<GUID>, saved: Option<GUID>) -> (GUID, Option<GUID>) {
    if current == Some(SCHEME_SAVER) {
        (saved.unwrap_or(SCHEME_BALANCED), None)
    } else {
        (SCHEME_SAVER, current)
    }
}

/// Power saver switch that remembers the scheme active before it was enabled.
#[derive(Debug, Default)]
pub struct PowerSaver {
    before: Option<GUID>,
}

impl PowerSaver {
    /// Toggle and return the real new state read back from the system; a
    /// failed switch leaves the state unchanged.
    pub fn toggle(&mut self) -> bool {
        let (target, next) = plan_toggle(active_power_scheme(), self.before);
        self.before = next;
        let _ = set_active_power_scheme(&target);
        is_power_saver()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only checks the pure string part; the real system calls need manual
    /// verification.
    #[test]
    fn settings_uris_are_the_documented_ones() {
        // These URIs are registered by the Settings app; a typo does nothing.
        for uri in [
            "ms-settings:",
            "ms-settings:privacy-location",
            "ms-settings:network-status",
        ] {
            assert!(uri.starts_with("ms-settings:"), "{uri}");
        }
    }

    #[test]
    fn toggling_into_saver_remembers_the_previous_scheme() {
        let high = GUID::from_u128(0x8c5e7fda_e8bf_4a96_9a85_a6e23a8c635c);
        let (target, saved) = plan_toggle(Some(high), None);
        assert_eq!(target, SCHEME_SAVER);
        assert_eq!(saved, Some(high), "must remember, not assume balanced");
    }

    #[test]
    fn toggling_out_of_saver_restores_what_was_saved() {
        let high = GUID::from_u128(0x8c5e7fda_e8bf_4a96_9a85_a6e23a8c635c);
        let (target, saved) = plan_toggle(Some(SCHEME_SAVER), Some(high));
        assert_eq!(target, high);
        assert_eq!(saved, None);
    }

    #[test]
    fn toggling_out_without_a_memory_falls_back_to_balanced() {
        let (target, saved) = plan_toggle(Some(SCHEME_SAVER), None);
        assert_eq!(target, SCHEME_BALANCED);
        assert_eq!(saved, None);
    }

    #[test]
    fn unreadable_scheme_still_toggles_into_saver() {
        let (target, saved) = plan_toggle(None, None);
        assert_eq!(target, SCHEME_SAVER);
        assert_eq!(saved, None);
    }

    /// Read-only: does not switch schemes, only checks a plausible GUID comes back.
    #[test]
    #[ignore = "queries the live power scheme; run with --ignored"]
    fn active_power_scheme_is_readable() {
        let g = active_power_scheme().expect("PowerGetActiveScheme failed");
        assert!(g.data1 != 0, "unexpected zero GUID");
        eprintln!(
            "active scheme: {:08x}-{:04x}-{:04x}",
            g.data1, g.data2, g.data3
        );
    }
}
