//! Drive the settings UI headlessly through kittest: no window, GPU or display.
//!
//! Widgets are queried through the AccessKit tree, so these assert that a
//! control is visible and operable, not that some code path ran.

mod about;
mod common;
mod items;
mod layout;
mod modal;
mod pages;
mod performance;
