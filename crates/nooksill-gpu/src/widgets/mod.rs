//! Popup panel widgets, one module per panel.
//! `lib.rs` re-exports the legacy `*_win` names as aliases.

pub mod anchor;
pub mod calendar;
pub mod control;
pub mod editor;
pub mod group;
pub mod notes;
pub mod resource;
pub mod style;
pub mod timer;

/// Shared visual style for every popup panel.
pub use style::PanelStyle;
