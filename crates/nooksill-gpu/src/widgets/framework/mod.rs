//! Runtime for the declarative widget-panel registry.
//!
//! `nooksill-core` owns the policy; this module owns placement geometry and
//! the link between a panel's `FrameRenderer` and its background material.

pub mod placement;
pub mod prefs;
pub mod runtime;

pub use nooksill_core::config::panel::{spec, Backdrop, PanelKind, Placement, Prefs, REGISTRY};
pub use placement::{place, Request};
pub use prefs::{current, material, sync_prefs};
pub use runtime::{apply_backdrop, place_panel, refresh_backdrop, release_backdrop};
