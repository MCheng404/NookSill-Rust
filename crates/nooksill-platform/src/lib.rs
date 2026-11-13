//! Win32 system services.
//!
//! This layer only reads system state and calls shell APIs; no dock logic
//! lives here. Every function returns safe types, `unsafe` never escapes.

#![deny(missing_debug_implementations)]

pub mod adapter;
pub mod app_window;
pub mod appbar;
pub mod audio;
pub mod display;
pub mod filedialog;
pub mod gpu;
pub mod hotkey;
pub mod icon;
pub mod image_icon;
pub mod json;
pub mod launch;
pub mod launchpad;
pub mod locale;
pub mod net;
pub mod occlusion;
pub mod paths;
pub mod shortcut;
pub mod single_instance;
pub mod sysact;
pub mod sysinfo;
pub mod theme;
pub mod tray;
pub mod update;
pub mod win;

pub use appbar::AppBar;
pub use audio::{Audio, Battery};
pub use display::{monitors, system_dpi, work_area, Monitor};
pub use gpu::{AdapterInfo, AdapterKind};
pub use icon::IconBitmap;
pub use paths::Paths;
pub use single_instance::SingleInstance;
pub use sysinfo::{CpuSampler, SysInfo};
