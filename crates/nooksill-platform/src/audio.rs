//! System volume through Core Audio's `IAudioEndpointVolume`.
//!
//! COM binds to the calling thread, so `Audio` is neither `Send` nor `Sync`;
//! simulated `VK_VOLUME_*` presses are avoided because they never report the
//! real level. Construction fails when there is no audio endpoint.

use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};

/// Volume control for the default playback device.
pub struct Audio {
    epv: IAudioEndpointVolume,
}

impl std::fmt::Debug for Audio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Audio").finish_non_exhaustive()
    }
}

impl Audio {
    /// Open the default playback device. Returns `None` when no device exists
    /// or the audio service is not running.
    #[must_use]
    pub fn new() -> Option<Self> {
        unsafe {
            // A second init on this thread returns RPC_E_CHANGED_MODE; the
            // default endpoint still works, so the error is ignored.
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            let epv: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None).ok()?;
            Some(Self { epv })
        }
    }

    /// Current master volume, 0..=100. `None` when it cannot be read.
    #[must_use]
    pub fn volume(&self) -> Option<u32> {
        let v = unsafe { self.epv.GetMasterVolumeLevelScalar() }.ok()?;
        Some((v * 100.0 + 0.5).clamp(0.0, 100.0) as u32)
    }

    /// Set the master volume; `v` is clamped to 0..=100.
    pub fn set_volume(&self, v: u32) {
        let v = v.min(100) as f32 / 100.0;
        unsafe {
            let _ = self.epv.SetMasterVolumeLevelScalar(v, std::ptr::null());
        }
    }

    /// Whether the endpoint is muted.
    #[must_use]
    pub fn muted(&self) -> bool {
        unsafe { self.epv.GetMute() }
            .map(|m| m.as_bool())
            .unwrap_or(false)
    }

    /// Set the mute state.
    pub fn set_muted(&self, muted: bool) {
        unsafe {
            let _ = self.epv.SetMute(muted, std::ptr::null());
        }
    }

    /// Flip mute and return the new state.
    pub fn toggle_mute(&self) -> bool {
        let next = !self.muted();
        self.set_muted(next);
        next
    }
}

/// Battery state for the control center status line.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct Battery {
    /// Charge percentage; `None` when there is no battery (desktop).
    pub percent: Option<u8>,
    /// Whether the machine is on AC power.
    pub ac: bool,
}

impl Battery {
    /// Read the state once via `GetSystemPowerStatus`.
    #[must_use]
    pub fn read() -> Self {
        use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        let mut st = SYSTEM_POWER_STATUS::default();
        if unsafe { GetSystemPowerStatus(&mut st) }.is_err() {
            return Self::default();
        }
        Self {
            // 255 means unknown, which is what desktops report.
            percent: (st.BatteryLifePercent != 255).then_some(st.BatteryLifePercent),
            ac: st.ACLineStatus == 1,
        }
    }

    /// Status line text.
    #[must_use]
    pub fn text(self) -> String {
        match self.percent {
            Some(p) if self.ac => format!("电量 {p}% · 已接通电源"),
            Some(p) => format!("电量 {p}%"),
            None => "未检测到电池".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The status line must cover all three cases and never be empty.
    #[test]
    fn battery_text_covers_all_cases() {
        assert_eq!(
            Battery {
                percent: Some(80),
                ac: true
            }
            .text(),
            "电量 80% · 已接通电源"
        );
        assert_eq!(
            Battery {
                percent: Some(35),
                ac: false
            }
            .text(),
            "电量 35%"
        );
        assert_eq!(Battery::default().text(), "未检测到电池");
    }
}
