//! Tray icon, the only way back after the dock is closed.
//!
//! Context menus reuse the custom drawn `nooksill_gpu::menu` window because
//! `TrackPopupMenu` needs a foreground owner and the dock never takes focus.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    LoadIconW, HICON, IDI_APPLICATION, WM_APP, WM_LBUTTONUP, WM_RBUTTONUP,
};

/// Tray callback message.
///
/// It must be `WM_APP + 2`: `worker::WM_APP_WAKE` owns `WM_APP + 1` and both
/// arrive on the dock hwnd. A collision would read a tray click as a worker
/// wake, breaking both.
pub const WM_APP_TRAY: u32 = WM_APP + 2;

/// Tray icon id (one per process).
const TRAY_ID: u32 = 1;

/// Why the tray icon could not be created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayError {
    /// `Shell_NotifyIconW(NIM_ADD)` failed (Explorer not ready or blocked).
    Add,
    /// Even the system default icon is unavailable.
    NoIcon,
}

impl std::fmt::Display for TrayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Add => f.write_str(nooksill_core::i18n::t("The system tray is unavailable")),
            Self::NoIcon => f.write_str(nooksill_core::i18n::t(
                "The application icon could not be loaded",
            )),
        }
    }
}

impl std::error::Error for TrayError {}

/// Tray icon. RAII: removed on drop, so no dead icon is left in the tray.
#[derive(Debug)]
pub struct Tray {
    hwnd: HWND,
    added: bool,
}

impl Tray {
    /// Register the icon. `icon` may be invalid (rc.exe not run), in which case
    /// the system default icon is used.
    pub fn add(hwnd: HWND, icon: HICON) -> Result<Self, TrayError> {
        let icon = if icon.is_invalid() {
            // SAFETY: a system shared icon; it must not be destroyed.
            unsafe { LoadIconW(None, IDI_APPLICATION) }.map_err(|_| TrayError::NoIcon)?
        } else {
            icon
        };
        let mut data = base_data(hwnd);
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        data.uCallbackMessage = WM_APP_TRAY;
        data.hIcon = icon;
        // Tooltip shown on hover: UTF-16 without the NUL, sized by the array.
        let tip: Vec<u16> = "NookSill".encode_utf16().collect();
        for (i, c) in tip.into_iter().enumerate().take(data.szTip.len() - 1) {
            data.szTip[i] = c;
        }
        // SAFETY: `data` lives on this frame and `base_data` filled `cbSize`.
        let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool();
        if !ok {
            return Err(TrayError::Add);
        }
        Ok(Self { hwnd, added: true })
    }

    /// Remove the icon. Safe to call repeatedly.
    pub fn remove(&mut self) {
        if !self.added {
            return;
        }
        self.added = false;
        let data = base_data(self.hwnd);
        // SAFETY: as above; delete only reads cbSize, hWnd and uID.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        }
    }

    /// Map a `WM_APP_TRAY` `lParam` to an action.
    ///
    /// `Some(true)` is a left click (toggle visibility), `Some(false)` a right
    /// click (open menu), `None` anything else (mouse move, balloon dismiss).
    #[must_use]
    pub fn parse(lparam: isize) -> Option<bool> {
        match (lparam & 0xFFFF) as u32 {
            WM_LBUTTONUP => Some(true),
            WM_RBUTTONUP => Some(false),
            _ => None,
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.remove();
    }
}

fn base_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: u32::try_from(std::mem::size_of::<NOTIFYICONDATAW>()).unwrap_or(0),
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_message_does_not_collide_with_worker_wake() {
        // Both messages land on the dock hwnd; a collision makes `pump` treat a
        // tray click as a worker wake and drop the event.
        assert_ne!(WM_APP_TRAY, WM_APP + 1);
        assert_eq!(WM_APP_TRAY, WM_APP + 2);
    }

    #[test]
    fn parses_left_and_right_click_only() {
        assert_eq!(Tray::parse(WM_LBUTTONUP as isize), Some(true));
        assert_eq!(Tray::parse(WM_RBUTTONUP as isize), Some(false));
        assert_eq!(Tray::parse(0x0200), None, "鼠标移动不算");
        assert_eq!(Tray::parse(0x0202), Some(true), "WM_LBUTTONUP");
    }

    #[test]
    fn base_data_has_a_valid_size_and_id() {
        let d = base_data(HWND::default());
        assert_eq!(d.cbSize as usize, std::mem::size_of::<NOTIFYICONDATAW>());
        assert_eq!(d.uID, TRAY_ID);
    }
}
