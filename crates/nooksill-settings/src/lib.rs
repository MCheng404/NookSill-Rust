//! Settings UI layer: egui widgets and kittest-drivable state, no windowing.

#![deny(missing_debug_implementations)]

pub mod fonts;
pub mod pages;
pub mod text_field;

pub use pages::{
    settings_ui, Command, ItemEdit, LibIcon, Modal, ModalAction, NavGroup, NavItem, Outcome, Page,
    SettingsState, UpdateView,
};

/// Left navigation width in logical pixels, fixed regardless of label text.
///
/// The window layer reads the same constant for the brand area, so both stay
/// aligned when the width changes.
pub const NAV_W: f32 = 193.0;
