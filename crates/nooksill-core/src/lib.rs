//! Pure logic layer for NookSill.
//!
//! Must not depend on Win32 or GDI+. Config parsing, item serialization and
//! layout math are testable without a window; anything requiring a real window
//! or GPU resource lives in `nooksill-gpu`.

#![deny(missing_debug_implementations)]

pub mod calendar;
pub mod clock;
pub mod color;
pub mod config;
pub mod control;
pub mod group;
pub mod icon;
pub mod ini;
pub mod item;
pub mod launchpad;
pub mod layout;
pub mod note;
pub mod running;
pub mod smoke;
pub mod store;
pub mod tool;
pub mod units;

/// Process-wide name.
pub const APP_NAME: &str = "NookSill";

/// Version baked at build time: `X.Y.Z` for release, `X.Y.Z-dev.<n>` for debug.
/// Shared source with the exe VERSIONINFO and artifact names (`scripts/version.rs`).
pub const VERSION: &str = env!("NOOKSILL_VERSION");
/// Single-instance mutex name.
pub const SINGLETON_MUTEX: &str = "Local\\NookSill_Singleton";

/// Item limits. Must match ini parsing, the UI loop and drag indices; defined
/// here once. Mirrors the `MAXI` / `MAXK` macros.
pub const MAX_ITEMS: usize = 128;
pub const MAX_KIDS: usize = 16;
