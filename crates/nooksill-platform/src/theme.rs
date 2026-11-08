//! System dark mode, accent color and autostart (registry).
//!
//! Everything reads once and falls back to a default on failure, so a
//! policy-locked registry cannot stop the dock from starting.

use windows::core::w;
use windows::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE,
    REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_DWORD,
};

use crate::win::{last_error, to_wide};

/// Whether the system uses dark mode (`AppsUseLightTheme == 0`).
#[must_use]
pub fn is_dark() -> bool {
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(std::ptr::addr_of_mut!(value).cast()),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        // Treat unreadable as light, matching the system default look.
        return false;
    }
    value == 0
}

/// DWM colorization color as raw RGB, not lightness-normalized.
///
/// `None` when unavailable (remote session, policy). The theme wants the raw
/// value because custom and system colors share one COLORREF path; see
/// [`accent`] for the normalized variant.
#[must_use]
pub fn accent_raw() -> Option<[u8; 3]> {
    use windows::core::BOOL;
    use windows::Win32::Graphics::Dwm::DwmGetColorizationColor;

    let mut color: u32 = 0;
    let mut opaque = BOOL(0);
    // SAFETY: both pointers refer to live locals for the whole call.
    unsafe { DwmGetColorizationColor(&mut color, &mut opaque) }.ok()?;
    Some([
        ((color >> 16) & 0xFF) as u8,
        ((color >> 8) & 0xFF) as u8,
        (color & 0xFF) as u8,
    ])
}

/// Accent color normalized so the largest channel is 170.
///
/// DWM returns whatever the user picked, from near-white (200,230,255) to deep
/// blue (0,60,120). Fed straight into the fog color, the first blows out to
/// white and the second is invisible. Clamping to one ceiling leaves hue as the
/// only variable, which is what "follow the accent" needs.
#[must_use]
pub fn accent() -> Option<[f32; 3]> {
    let raw = accent_raw()?;
    let rgb = [raw[0] as f32, raw[1] as f32, raw[2] as f32];
    let max = rgb[0].max(rgb[1]).max(rgb[2]);
    if max < 1.0 {
        return None;
    }
    Some([
        rgb[0] * 170.0 / max,
        rgb[1] * 170.0 / max,
        rgb[2] * 170.0 / max,
    ])
}

/// Whether autostart is registered.
///
/// Presence of the value is enough, it is not compared against the current
/// path: a moved exe still reports true, and `set_autostart(true)` rewrites it.
#[must_use]
pub fn get_autostart() -> bool {
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Run"),
            None,
            KEY_QUERY_VALUE,
            &mut key,
        )
    };
    if status != ERROR_SUCCESS {
        return false;
    }
    let mut size = 0u32;
    let status =
        unsafe { RegQueryValueExW(key, w!("NookSill"), None, None, None, Some(&mut size)) };
    unsafe {
        let _ = RegCloseKey(key);
    };
    status == ERROR_SUCCESS
}

/// Register or remove the autostart entry. Returns whether the write succeeded.
pub fn set_autostart(on: bool) -> bool {
    let mut key = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Run"),
            None,
            w!(""),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return false;
    }

    let ok = if on {
        let cmd = format!(
            "\"{}\"",
            std::env::current_exe().unwrap_or_default().display()
        );
        let wide = to_wide(&cmd);
        let bytes =
            unsafe { std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2) };
        unsafe { RegSetValueExW(key, w!("NookSill"), None, REG_SZ, Some(bytes)) == ERROR_SUCCESS }
    } else {
        // A missing value counts as removed.
        let s = unsafe { RegDeleteValueW(key, w!("NookSill")) };
        s == ERROR_SUCCESS || s == WIN32_ERROR(2) // ERROR_FILE_NOT_FOUND
    };

    unsafe {
        let _ = RegCloseKey(key);
    };
    ok
}

/// Code of the last registry operation, for logging failed `set_autostart` calls.
#[must_use]
pub fn last_registry_error() -> u32 {
    last_error()
}
