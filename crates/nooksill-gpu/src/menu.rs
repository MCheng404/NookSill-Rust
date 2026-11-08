//! Context menu: item model, per-kind construction, layout and painting.
//!
//! The menu pops up above the bar (or towards the screen centre), outside the
//! Dock window. A layered window clips nothing useful: what is drawn outside
//! its rect is simply invisible, so the menu needs a window of its own.
//!
//! This module owns what the menu contains and how big it is (pure logic,
//! testable). Window creation, the modal message loop and command execution
//! live in `bin/dock.rs`, since they need Win32.
//!
//! The model is a blocking self-drawn menu: open, pick an entry or cancel,
//! return a command.

use nooksill_core::item::{Item, ItemKind};

use egui::epaint::{CornerRadius, StrokeKind};
use egui::Rect as EguiRect;
use egui::{Align2, Color32, FontId, Stroke};

/// A menu command. Execution lives in the caller (`bin/dock.rs`).
///
/// The three that change the item count (remove, add separator, delete
/// separator) rely on the config hot-reload chain: count -> bar width ->
/// window size -> rebuilt offscreen target and DIB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCmd {
    /// Open / launch; the caller decides between activating and a new instance.
    Open,
    /// Reveal in Explorer (folders only).
    OpenInExplorer,
    /// Ask the app to quit (running non-item apps only).
    QuitApp,
    /// Insert a separator after this item.
    AddSepAfter,
    /// Pin a running temporary item into the Dock (writes config).
    PinItem,
    /// Remove from the Dock (normal items).
    RemoveItem,
    /// Delete this separator.
    RemoveSep,
    /// Open the settings window.
    Settings,
    /// Toggle run-at-login (the checked entry).
    ToggleAutostart,
    /// Blank-area `New`: add an application or file.
    AddFile,
    /// Blank-area `New`: add an existing folder.
    AddFolder,
    /// Blank-area `New`: create a folder on disk and add it to the Dock.
    NewFolder,
    /// Blank-area `New`: create a group (directory).
    NewGroup,
    /// Rename, or edit target and arguments (edit dialog).
    Rename,
    /// Duplicate this item.
    Duplicate,
    /// Open the group (shows its contents).
    OpenGroup,
    /// Dissolve the group: children return to the bar body.
    DissolveGroup,
    /// Pick a custom icon file for this item.
    ChangeIcon,
    /// Drop the custom icon of this item.
    ResetIcon,
    /// Show / hide the Dock (tray menu).
    ToggleDock,
    /// Open the notes panel.
    Notes,
    /// Open the launchpad.
    Launchpad,
    /// Exit the Dock process.
    ExitDock,
}

/// A single menu entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    pub label: String,
    pub cmd: MenuCmd,
    /// Red label (destructive); the warning colour hints the step is hard to undo.
    pub danger: bool,
    /// Draw a separator after this entry.
    pub sep_after: bool,
    /// Has a check slot; one such entry makes the whole menu reserve it.
    pub checkable: bool,
    /// Currently checked.
    pub checked: bool,
}

impl MenuEntry {
    /// Entry with a check slot; `on` is the current state.
    fn toggle(label: &str, cmd: MenuCmd, on: bool) -> Self {
        Self {
            checkable: true,
            checked: on,
            ..Self::new(label, cmd, false, false)
        }
    }

    fn new(label: &str, cmd: MenuCmd, danger: bool, sep_after: bool) -> Self {
        Self {
            label: label.to_owned(),
            cmd,
            danger,
            checkable: false,
            checked: false,
            sep_after,
        }
    }
}

/// Menu layout in logical pixels, shared by painting and hit-testing.
const ITEM_H: f32 = 31.0;
const SEP_H: f32 = 9.0;
const PAD_Y: f32 = 6.0;
const PAD_X: f32 = 12.0;
/// Minimum menu width; narrower squeezes long labels together.
const MIN_W: f32 = 190.0;
/// Menu background corner radius (logical pixels).
pub const RADIUS: f32 = 10.0;

/// Build switches, taken from the `Mn*` config keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuOpts {
    /// `MnAuto`: show `Run at login`, value = currently enabled. `None` = hidden.
    pub autostart: Option<bool>,
    /// `MnAdd`: blank-area menu offers add file / folder / new.
    pub add_items: bool,
    /// `MnNotes`: blank-area menu offers the notes panel.
    pub notes: bool,
}

impl Default for MenuOpts {
    fn default() -> Self {
        // Must match the ini defaults of `MnAdd` / `MnNotes` (1 = shown).
        Self {
            autostart: None,
            add_items: true,
            notes: true,
        }
    }
}

/// One open menu.
#[derive(Debug, Clone)]
pub struct Menu {
    pub entries: Vec<MenuEntry>,
    /// Highlighted entry, an index into `entries`.
    pub hover: Option<usize>,
}

mod build;
mod layout;
mod paint;

#[cfg(test)]
mod tests;
