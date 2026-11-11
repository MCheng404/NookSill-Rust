//! GPU presentation layer: wgpu surface, egui painting, Win32 input and hit
//! testing.
//!
//! This layer only owns windows and submission. What to draw comes from
//! `nooksill-settings` (the settings UI) and `dock`; system information from
//! `nooksill-platform`; config and geometry from `nooksill-core`.
//!
//! There is no eframe/winit dependency: the message loop is owned here because
//! the tray, global hotkeys, AppBar reservation and per-pixel hit testing all
//! have to talk in `WndProc`. Input translation (`input.rs`) and IME are
//! written by hand as a result.

#![deny(missing_debug_implementations)]

pub mod accent;
pub mod shaders;

/// Load the icon embedded by `build.rs` (resource id 1).
/// Returns `None` when rc.exe was unavailable; a missing icon must not block startup.
#[must_use]
#[expect(
    clippy::manual_dangling_ptr,
    reason = "MAKEINTRESOURCE passes integer 1 as a resource id; never dereferenced"
)]
pub fn app_icon() -> Option<windows::Win32::UI::WindowsAndMessaging::HICON> {
    use windows::core::PCWSTR;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::LoadIconW;

    let hinst = unsafe { GetModuleHandleW(None) }.ok()?;
    let icon = unsafe {
        LoadIconW(
            Some(windows::Win32::Foundation::HINSTANCE(hinst.0)),
            PCWSTR(1usize as *const u16), // MAKEINTRESOURCE(1)
        )
    }
    .ok()?;
    (!icon.0.is_null()).then_some(icon)
}

pub mod cursor;
pub mod dock;
pub mod drop_target;
pub mod frame;
pub mod frost;
pub mod glyph;
pub mod gpu;
pub mod ime;
pub mod input;
pub mod launchpad_win;
pub mod mem;
pub mod menu;
pub mod offscreen;
pub mod perf;
pub mod popup;
pub mod present;
pub mod settings_win;
pub mod smoke;
pub(crate) mod text;
pub mod widgets;
pub mod worker;

/// Compatibility re-exports for the old widget module paths.
///
/// The files moved to [`widgets`], but `bin/dock.rs` and `examples/*.rs` still
/// refer to names like `nooksill_gpu::calendar_win::CalendarWindow`. The
/// re-exports keep the old paths alive so the directory split did not have to
/// touch dozens of `use` statements; new code should use `widgets::calendar`
/// and friends.
pub use widgets::calendar as calendar_win;
pub use widgets::control as cc_win;
pub use widgets::editor as edit_win;
pub use widgets::group as group_win;
pub use widgets::notes as note_win;
pub use widgets::resource as resmon_win;
pub use widgets::style as panel_style;
pub use widgets::timer as tool_win;

pub use dock::{Dock, DockItem};
pub use frame::{FrameOutcome, FrameRenderer};
pub use gpu::{Gpu, GpuError, Selection};
pub use group_win::{GroupAction, GroupKid, GroupWindow};
pub use mem::MemStats;
pub use menu::{Menu, MenuCmd, MenuEntry};
pub use offscreen::Offscreen;
pub use present::LayeredPresenter;
pub use settings_win::SettingsWindow;
pub use worker::{DiskWriter, IconLoader, Opener, Scanner};
