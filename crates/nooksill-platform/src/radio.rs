//! Radio and privacy toggles behind the control-centre tiles.
//!
//! WiFi uses the WLAN API (software radio state); Bluetooth has no Win32
//! switch and goes through the WinRT `Radio` class, which may prompt for
//! permission on first use. Location is read-only: Windows has no public API
//! to flip it, so the tile reports the consent store and opens Settings.

use std::time::Duration;
use windows::core::{Interface, GUID, PCWSTR};
use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};
use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE};
use windows::Win32::NetworkManagement::WiFi::{
    dot11_radio_state_off, dot11_radio_state_on, WlanCloseHandle, WlanEnumInterfaces,
    WlanFreeMemory, WlanOpenHandle, WlanQueryInterface, WlanSetInterface,
    WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST, WLAN_PHY_RADIO_STATE,
};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows_collections::IVectorView;
use windows_future::{AsyncStatus, IAsyncInfo, IAsyncOperation};

/// WiFi radio and connection snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WifiState {
    /// Software radio on (hardware switch still wins when off).
    pub on: bool,
    /// Connected SSID, `None` when not associated.
    pub ssid: Option<String>,
    /// 0..100 signal quality, `None` when not connected.
    pub signal: Option<u8>,
}

/// One WLAN client handle; closed on drop.
struct Wlan(HANDLE);

impl Wlan {
    fn open() -> Option<Self> {
        let mut negotiated = 0_u32;
        let mut handle = HANDLE::default();
        // SAFETY: out-parameters are valid; version 2 is the documented value.
        let rc = unsafe { WlanOpenHandle(2, None, &mut negotiated, &mut handle) };
        (rc == 0).then_some(Self(handle))
    }

    /// GUID of the first wireless interface.
    fn first_guid(&self) -> Option<GUID> {
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        // SAFETY: the out-pointer is valid; the list is freed below.
        if unsafe { WlanEnumInterfaces(self.0, None, &mut list) } != 0 || list.is_null() {
            return None;
        }
        let count = unsafe { (*list).dwNumberOfItems };
        let guid = if count == 0 {
            None
        } else {
            // SAFETY: `InterfaceInfo` has at least `count` entries.
            Some(unsafe { (*list).InterfaceInfo[0].InterfaceGuid })
        };
        // SAFETY: the list came from `WlanEnumInterfaces`.
        unsafe { WlanFreeMemory(list.cast()) };
        guid
    }

    fn radio_state(&self, guid: &GUID) -> Option<WLAN_PHY_RADIO_STATE> {
        let mut size = 0_u32;
        let mut data: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: the query allocates `data`, freed with `WlanFreeMemory`.
        let rc = unsafe {
            WlanQueryInterface(
                self.0,
                guid,
                windows::Win32::NetworkManagement::WiFi::wlan_intf_opcode_radio_state,
                None,
                &mut size,
                &mut data,
                None,
            )
        };
        if rc != 0 || data.is_null() {
            return None;
        }
        // SAFETY: the opcode returns a `WLAN_PHY_RADIO_STATE`.
        let state = unsafe { std::ptr::read(data.cast::<WLAN_PHY_RADIO_STATE>()) };
        // SAFETY: allocated by the query above.
        unsafe { WlanFreeMemory(data.cast()) };
        Some(state)
    }

    fn connection(&self, guid: &GUID) -> (Option<String>, Option<u8>) {
        let mut size = 0_u32;
        let mut data: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: the query allocates `data`, freed with `WlanFreeMemory`.
        let rc = unsafe {
            WlanQueryInterface(
                self.0,
                guid,
                windows::Win32::NetworkManagement::WiFi::wlan_intf_opcode_current_connection,
                None,
                &mut size,
                &mut data,
                None,
            )
        };
        if rc != 0 || data.is_null() {
            return (None, None);
        }
        // SAFETY: the opcode returns `WLAN_CONNECTION_ATTRIBUTES`.
        let attrs = unsafe { &*(data.cast::<WLAN_CONNECTION_ATTRIBUTES>()) };
        let assoc = &attrs.wlanAssociationAttributes;
        let ssid = ssid_text(&assoc.dot11Ssid.ucSSID, assoc.dot11Ssid.uSSIDLength);
        let signal = Some(assoc.wlanSignalQuality.min(100) as u8);
        // SAFETY: allocated by the query above.
        unsafe { WlanFreeMemory(data.cast()) };
        (ssid, signal)
    }
}

impl Drop for Wlan {
    fn drop(&mut self) {
        // SAFETY: the handle came from `WlanOpenHandle` and is closed once.
        unsafe {
            let _ = WlanCloseHandle(self.0, None);
        }
    }
}

/// Current WiFi state; `None` when there is no wireless interface.
#[must_use]
pub fn wifi_state() -> Option<WifiState> {
    let wlan = Wlan::open()?;
    let guid = wlan.first_guid()?;
    let radio = wlan.radio_state(&guid)?;
    let on = radio.dot11SoftwareRadioState != dot11_radio_state_off
        && radio.dot11HardwareRadioState != dot11_radio_state_off;
    let (ssid, signal) = if on {
        wlan.connection(&guid)
    } else {
        (None, None)
    };
    Some(WifiState { on, ssid, signal })
}

/// Turns the WiFi software radio on or off. Hardware switches cannot be
/// controlled and still win.
#[must_use]
pub fn set_wifi(on: bool) -> bool {
    let Some(wlan) = Wlan::open() else {
        return false;
    };
    let Some(guid) = wlan.first_guid() else {
        return false;
    };
    let state = WLAN_PHY_RADIO_STATE {
        dwPhyIndex: 0,
        dot11SoftwareRadioState: if on {
            dot11_radio_state_on
        } else {
            dot11_radio_state_off
        },
        dot11HardwareRadioState: dot11_radio_state_on,
    };
    // SAFETY: the struct outlives the call; hardware state is ignored.
    let rc = unsafe {
        WlanSetInterface(
            wlan.0,
            &guid,
            windows::Win32::NetworkManagement::WiFi::wlan_intf_opcode_radio_state,
            std::mem::size_of::<WLAN_PHY_RADIO_STATE>() as u32,
            std::ptr::from_ref(&state).cast(),
            None,
        )
    };
    rc == 0
}

/// Bluetooth radio state; `None` without an adapter or when access is denied.
#[must_use]
pub fn bluetooth_state() -> Option<bool> {
    let radio = bluetooth_radio()?;
    Some(radio.State().ok()? == RadioState::On)
}

/// Toggles Bluetooth through the WinRT radio stack.
#[must_use]
pub fn set_bluetooth(on: bool) -> bool {
    let Some(radio) = bluetooth_radio() else {
        return false;
    };
    let target = if on { RadioState::On } else { RadioState::Off };
    let Some(operation) = radio.SetStateAsync(target).ok() else {
        return false;
    };
    let Some(Ok(status)) = wait::<RadioAccessStatus>(operation) else {
        return false;
    };
    status == RadioAccessStatus::Allowed
}

/// Location consent; `None` when the key is missing (older builds).
#[must_use]
pub fn location_state() -> Option<bool> {
    let sub = wide("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\location");
    let value = wide("Value");
    let mut buf = [0_u16; 32];
    let mut size = (buf.len() * 2) as u32;
    let mut ty = windows::Win32::System::Registry::REG_SZ;
    // SAFETY: the buffer and its byte size are consistent.
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            Some(&mut ty),
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1).min(buf.len());
    let text = String::from_utf16_lossy(&buf[..len]);
    Some(text.eq_ignore_ascii_case("Allow"))
}

fn bluetooth_radio() -> Option<Radio> {
    winrt_init();
    let access = wait::<RadioAccessStatus>(Radio::RequestAccessAsync().ok()?)?.ok()?;
    if access != RadioAccessStatus::Allowed {
        return None;
    }
    let radios: IVectorView<Radio> =
        wait::<IVectorView<Radio>>(Radio::GetRadiosAsync().ok()?)?.ok()?;
    for i in 0..radios.Size().ok()? {
        let radio = radios.GetAt(i).ok()?;
        if radio.Kind().ok()? == RadioKind::Bluetooth {
            return Some(radio);
        }
    }
    None
}

fn winrt_init() {
    use std::sync::OnceLock;
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        // SAFETY: MTA init is idempotent; an already-initialized thread
        // returns an error that is expected and ignored.
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
    });
}

/// Blocks on a WinRT operation. The callers are UI-thread probes, so a bounded
/// spin is preferable to threading a completion handler through the panel.
fn wait<T>(operation: IAsyncOperation<T>) -> Option<windows::core::Result<T>>
where
    T: windows::core::RuntimeType + 'static,
{
    let info: IAsyncInfo = operation.cast().ok()?;
    for _ in 0..600 {
        match info.Status().ok()? {
            AsyncStatus::Completed => return Some(operation.GetResults()),
            AsyncStatus::Error | AsyncStatus::Canceled => return None,
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    None
}

/// SSID bytes to text; an empty or invalid payload reads as not connected.
fn ssid_text(bytes: &[u8], len: u32) -> Option<String> {
    let n = (len as usize).min(bytes.len());
    if n == 0 {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes[..n]).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssid_text_rejects_empty_and_overlong_payloads() {
        assert_eq!(ssid_text(b"Home", 4).as_deref(), Some("Home"));
        assert_eq!(ssid_text(b"Home", 99).as_deref(), Some("Home"));
        assert_eq!(ssid_text(b"", 0), None);
        assert_eq!(ssid_text(b"  ", 2), None);
        assert_eq!(
            ssid_text(&[0xFF, 0xFE], 2).as_deref(),
            Some("\u{fffd}\u{fffd}")
        );
    }

    #[test]
    fn probes_are_safe_without_hardware() {
        // Every probe must be total: a desktop without WiFi or Bluetooth
        // simply reports `None`.
        if let Some(wifi) = wifi_state() {
            assert!(wifi.signal.is_none_or(|s| s <= 100));
        }
        let _ = bluetooth_state();
        let _ = location_state();
    }
}
