//! Thin wrappers for wide strings, DPI and COM initialization.

use windows::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, ReleaseDC, LOGPIXELSX};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

/// Convert `&str` to NUL-terminated UTF-16, the input every `*W` API expects.
#[must_use]
pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Convert a UTF-16 buffer to `String`, stopping at the first NUL.
#[must_use]
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Return the system DPI of the primary display (96 fallback).
///
/// `GetDeviceCaps(LOGPIXELSX)` yields the system DPI, not per-monitor DPI.
/// The process is not DPI aware, so Windows bitmap-stretches the whole window
/// and one scale factor applies to the entire program.
#[must_use]
pub fn system_dpi() -> i32 {
    let dc = unsafe { GetDC(None) };
    if dc.0.is_null() {
        return 96;
    }
    let dpi = unsafe { GetDeviceCaps(Some(dc), LOGPIXELSX) };
    unsafe { ReleaseDC(None, dc) };
    if dpi <= 0 {
        96
    } else {
        dpi
    }
}

/// Raw code of the last Win32 error (debugging aid).
#[must_use]
pub fn last_error() -> u32 {
    unsafe { windows::Win32::Foundation::GetLastError().0 }
}

thread_local! {
    /// Whether this thread's COM apartment is initialized. Must be per thread.
    ///
    /// A process-wide `Once` would initialize only the first thread that asks;
    /// `CoCreateInstance` on any other thread then fails with
    /// `CO_E_NOTINITIALIZED`, silently yielding no icon instead of an error.
    ///
    /// STA (`COINIT_APARTMENTTHREADED`) is chosen because shell `IShellLink` and
    /// `IImageList` behave most reliably in it. There is deliberately no
    /// matching `CoUninitialize`: uninitializing early would break later COM
    /// calls, so the thread exit reclaims the apartment instead.
    static COM_READY: () = {
        // SAFETY: lazily run once per thread by `thread_local!`, no matching
        // uninitialize (see above).
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
    };
}

/// Ensure COM is initialized on the calling thread. Safe to call repeatedly
/// from any thread.
///
/// If the caller already moved the thread to MTA, `CoInitializeEx` returns
/// `RPC_E_CHANGED_MODE`; shell objects still work there, so the error is ignored.
pub fn ensure_com() {
    COM_READY.with(|()| {});
}
