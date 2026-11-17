//! Runtime for the declarative widget-panel registry.
//!
//! `nooksill-core` owns the policy; this module owns placement geometry and
//! the link between a panel's `FrameRenderer` and its background material.

pub mod lease;
pub mod placement;
pub mod prefs;
pub mod runtime;

pub use lease::WindowLease;
pub use nooksill_core::config::panel::{spec, Backdrop, PanelKind, Placement, Prefs, REGISTRY};
pub use placement::{place, Request};
pub use prefs::{current, material, sync_prefs};
pub use runtime::{
    apply_backdrop, destroy_panel, hide_panel, place_panel, refresh_backdrop, release_backdrop,
    show_panel, show_panel_active,
};
