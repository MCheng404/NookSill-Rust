//! System brightness: WMI for internal panels, DDC/CI for external monitors.
//!
//! Windows has no single API for every panel: laptop panels sit on eDP and are
//! driven through WMI (`WmiMonitorBrightness`), external monitors accept DDC/CI
//! VCP 0x10. WMI is tried first, DDC/CI is the fallback.
//!
//! A DDC write is an I2C transaction (10-50 ms); flooding the controller can
//! wedge it, so callers throttle. Both backends stay synchronous.

use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitor, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness, PHYSICAL_MONITOR,
};
use windows::Win32::Foundation::{HANDLE, POINT};
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTOPRIMARY};

/// How the current level is read and written.
enum Backend {
    /// Windows brightness stack (internal laptop panel).
    Wmi,
    /// DDC/CI physical monitor (external display).
    Ddc { monitor: HANDLE, min: u32, max: u32 },
}

/// A brightness control that actually answered a probe.
pub struct Brightness {
    backend: Backend,
    percent: u32,
}

impl std::fmt::Debug for Brightness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Brightness")
            .field("backend", &std::mem::discriminant(&self.backend))
            .field("percent", &self.percent)
            .finish_non_exhaustive()
    }
}

impl Brightness {
    /// Probes the internal panel first, then the primary external monitor.
    ///
    /// `None` means neither system interface is usable (desktop without DDC,
    /// OEM driver hiding the WMI class, dock that drops DDC traffic).
    #[must_use]
    pub fn probe() -> Option<Self> {
        if let Some(percent) = wmi::read() {
            return Some(Self {
                backend: Backend::Wmi,
                percent,
            });
        }
        ddc_probe()
    }

    #[must_use]
    pub const fn percent(&self) -> u32 {
        self.percent
    }

    /// Applies a 0..100 level through the backend that answered the probe.
    pub fn set(&mut self, percent: u32) {
        let percent = percent.min(100);
        match &self.backend {
            Backend::Wmi => {
                wmi::write(percent);
            }
            Backend::Ddc { monitor, min, max } => {
                let value = min + (max - min) * percent / 100;
                // SAFETY: the handle came from `probe` and lives until `Drop`.
                let _ = unsafe { SetMonitorBrightness(*monitor, value) };
            }
        }
        self.percent = percent;
    }

    /// Which backend answered the probe; used for diagnostics.
    #[must_use]
    pub const fn is_wmi(&self) -> bool {
        matches!(self.backend, Backend::Wmi)
    }
}

impl Drop for Brightness {
    fn drop(&mut self) {
        if let Backend::Ddc { monitor, .. } = &self.backend {
            // SAFETY: this handle is owned by `self` and released exactly once.
            unsafe {
                let _ = DestroyPhysicalMonitor(*monitor);
            }
        }
    }
}

fn ddc_probe() -> Option<Brightness> {
    // SAFETY: `MonitorFromPoint` only reads the desktop topology.
    let hmon = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    if hmon.is_invalid() {
        return None;
    }
    let mut count = 0_u32;
    // SAFETY: the out-pointer is valid for the call.
    if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut count) }.is_err() || count == 0 {
        return None;
    }
    let mut monitors = vec![PHYSICAL_MONITOR::default(); count as usize];
    // SAFETY: the slice has exactly `count` entries.
    if unsafe { GetPhysicalMonitorsFromHMONITOR(hmon, &mut monitors) }.is_err() {
        return None;
    }

    let mut chosen: Option<(HANDLE, u32, u32, u32)> = None;
    for m in &monitors {
        let (mut min, mut current, mut max) = (0_u32, 0_u32, 0_u32);
        // SAFETY: `m` comes from a successful enumeration.
        let rc =
            unsafe { GetMonitorBrightness(m.hPhysicalMonitor, &mut min, &mut current, &mut max) };
        if rc != 0 && max > min {
            chosen = Some((m.hPhysicalMonitor, min, current, max));
            break;
        }
    }
    for m in &monitors {
        // PHYSICAL_MONITOR is packed: copy the field before comparing.
        let handle = m.hPhysicalMonitor;
        if chosen.is_none_or(|(kept, ..)| kept != handle) {
            // SAFETY: every entry was returned by the enumeration.
            unsafe {
                let _ = DestroyPhysicalMonitor(handle);
            }
        }
    }
    let (monitor, min, current, max) = chosen?;
    Some(Brightness {
        backend: Backend::Ddc { monitor, min, max },
        percent: to_percent(current, min, max),
    })
}

/// Maps a hardware reading onto 0..100; out-of-range readings are clamped.
fn to_percent(current: u32, min: u32, max: u32) -> u32 {
    if max <= min {
        return 0;
    }
    let span = max - min;
    let value = current.saturating_sub(min).min(span);
    (value * 100 / span).min(100)
}

/// WMI automation interface for the Windows brightness stack.
mod wmi {
    use std::sync::OnceLock;
    use windows::core::{BSTR, GUID};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::Variant::{VARIANT, VT_I4, VT_UI1};
    use windows::Win32::System::Wmi::{ISWbemLocator, ISWbemObject, ISWbemServices};

    const CLSID_SWBEMLOCATOR: GUID = GUID::from_values(
        0x76a6_4158,
        0xcb41,
        0x11d1,
        [0x8b, 0x02, 0x00, 0x60, 0x08, 0x06, 0xd9, 0xb6],
    );

    fn com_init() {
        static INIT: OnceLock<()> = OnceLock::new();
        INIT.get_or_init(|| {
            // SAFETY: COM init is idempotent; an already-initialized apartment
            // returns an error that is expected and ignored.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
        });
    }

    fn services() -> Option<ISWbemServices> {
        com_init();
        // SAFETY: documented SWbemLocator CLSID; `None` means no aggregation.
        let locator: ISWbemLocator =
            unsafe { CoCreateInstance(&CLSID_SWBEMLOCATOR, None, CLSCTX_INPROC_SERVER).ok()? };
        let empty = BSTR::from("");
        // SAFETY: all BSTRs are valid and outlive the call.
        unsafe {
            locator
                .ConnectServer(
                    &BSTR::from("."),
                    &BSTR::from(r"ROOT\WMI"),
                    &empty,
                    &empty,
                    &empty,
                    &empty,
                    0,
                    None,
                )
                .ok()
        }
    }

    fn first_object(services: &ISWbemServices, query: &str) -> Option<ISWbemObject> {
        // SAFETY: both BSTRs are valid; `None` is a valid named-value set.
        let set = unsafe {
            services
                .ExecQuery(&BSTR::from(query), &BSTR::from("WQL"), 0, None)
                .ok()?
        };
        // SAFETY: the provider bounds-checks the index.
        unsafe { set.ItemIndex(0).ok() }
    }

    /// Current level from the ACPI panel, or `None` when the class is hidden.
    pub(super) fn read() -> Option<u32> {
        let services = services()?;
        let object = first_object(
            &services,
            "SELECT CurrentBrightness FROM WmiMonitorBrightness",
        )?;
        // SAFETY: the object comes from a successful WMI query.
        let property = unsafe {
            object
                .Properties_()
                .ok()?
                .Item(&BSTR::from("CurrentBrightness"), 0)
                .ok()?
        };
        // SAFETY: `CurrentBrightness` is VT_UI1 per the WMI class definition.
        let value = unsafe { property.Value().ok()? };
        let percent = unsafe { value.Anonymous.Anonymous.Anonymous.bVal } as u32;
        (percent <= 100).then_some(percent)
    }

    /// Calls `WmiSetBrightness(Timeout, Brightness)` on the first methods
    /// instance. Parameters come from the class' own method definition.
    pub(super) fn write(percent: u32) -> bool {
        let Some(services) = services() else {
            return false;
        };
        let Some(object) = first_object(&services, "SELECT * FROM WmiMonitorBrightnessMethods")
        else {
            return false;
        };
        // SAFETY: all interface calls use objects owned by this function.
        unsafe {
            let Ok(method) = object
                .Methods_()
                .and_then(|methods| methods.Item(&BSTR::from("WmiSetBrightness"), 0))
            else {
                return false;
            };
            let Ok(definition) = method.InParameters() else {
                return false;
            };
            let Ok(params) = definition.SpawnInstance_(0) else {
                return false;
            };
            let Ok(properties) = params.Properties_() else {
                return false;
            };
            let timeout = variant_i4(1);
            let level = variant_ui1(percent.min(100) as u8);
            let Ok(p) = properties.Item(&BSTR::from("Timeout"), 0) else {
                return false;
            };
            if p.SetValue(&timeout).is_err() {
                return false;
            }
            let Ok(p) = properties.Item(&BSTR::from("Brightness"), 0) else {
                return false;
            };
            if p.SetValue(&level).is_err() {
                return false;
            }
            // WMI reports success as S_OK with no out-object here; a returned
            // `Err` whose HRESULT is non-negative is still success.
            match object.ExecMethod_(&BSTR::from("WmiSetBrightness"), &params, 0, None) {
                Ok(_) => true,
                Err(e) => e.code().0 >= 0,
            }
        }
    }

    fn variant_ui1(value: u8) -> VARIANT {
        let mut variant = VARIANT::default();
        // SAFETY: the union field matches `vt` and the VARIANT stays on the
        // stack until the WMI call consumes it.
        unsafe {
            let inner = &mut *variant.Anonymous.Anonymous;
            inner.vt = VT_UI1;
            inner.Anonymous.bVal = value;
        }
        variant
    }

    fn variant_i4(value: i32) -> VARIANT {
        let mut variant = VARIANT::default();
        // SAFETY: the union field matches `vt`.
        unsafe {
            let inner = &mut *variant.Anonymous.Anonymous;
            inner.vt = VT_I4;
            inner.Anonymous.lVal = value;
        }
        variant
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_maps_across_the_hardware_range() {
        assert_eq!(to_percent(0, 0, 100), 0);
        assert_eq!(to_percent(50, 0, 100), 50);
        assert_eq!(to_percent(100, 0, 100), 100);
        assert_eq!(to_percent(0, 20, 80), 0, "below minimum reads as zero");
        assert_eq!(to_percent(50, 20, 80), 50);
        assert_eq!(to_percent(1000, 20, 80), 100, "above maximum reads as full");
        assert_eq!(
            to_percent(50, 50, 50),
            0,
            "degenerate range never divides by zero"
        );
    }

    #[test]
    fn probe_never_panics_without_hardware() {
        // Headless or locked-down machines simply have no usable interface.
        if let Some(mut brightness) = Brightness::probe() {
            let current = brightness.percent();
            assert!(current <= 100);
            brightness.set(current);
        }
    }
}
