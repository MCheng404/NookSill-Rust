//! Launchpad data model: apps, folders, grid order, hidden list.
//!
//! Independent of the UI so that behaviour like "drop on an icon merges into a
//! folder" can be unit tested; it is the part that cannot be checked by eye.
//!
//! `launchpad.dat` layout:
//!
//! ```text
//! T\t<path>\t<name>     one cell = one app
//! F\t<name>            one cell = one folder, followed by f lines until E
//! f\t<path>\t<name>    app inside the current folder
//! E                     ends the folder
//! H\t<path>            hidden from the launchpad
//! G\t<name>            one tag group, numbered 1..=8 by appearance order
//! C\t<name>\t<cat>     manual category: app name -> 1-based group number
//! ```
//!
//! Files without `G` / `C` still load: they get the three default groups and
//! every category comes from the heuristic.

pub const MAX_APPS: usize = 700;
pub const MAX_FOLDERS: usize = 60;
pub const MAX_TOP: usize = 760;
pub const MAX_IN_FOLDER: usize = 24;
/// Also the highest category number: `G` groups are numbered from 1.
pub const MAX_TAGS: usize = 8;
/// Counted in characters, not bytes.
pub const TAG_NAME_CAP: usize = 16;

const DEFAULT_TAGS: [&str; 3] = ["办公", "系统", "工具"];

/// One scanned candidate, shared by the scanner and [`Launchpad::absorb`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppEntry {
    /// Usually the file name without its extension.
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub name: String,
    /// `.lnk`, exe, URL or folder.
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    /// Indices into [`Launchpad::apps`], in the order shown inside the folder.
    pub apps: Vec<usize>,
}

/// One top-level cell.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Top {
    /// Index into [`Launchpad::apps`].
    App(usize),
    /// Index into [`Launchpad::folders`].
    Folder(usize),
}

/// What happens to an app taken out of a folder.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TakeOut {
    /// Append a new cell at the end of the grid.
    ToTop,
    /// Write an `H` line; the scanner will skip it from now on.
    Hide,
    /// Only leave the folder, the app stays in the table unused.
    Drop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launchpad {
    apps: Vec<App>,
    folders: Vec<Folder>,
    top: Vec<Top>,
    hidden: Vec<String>,
    /// Index + 1 is the category number shown to the user.
    tags: Vec<String>,
    /// App name -> 1-based category number.
    ///
    /// Keyed by name, not path: the dat file is meant to be read by humans and
    /// the path holds a GUID that changes across reinstalls.
    cat_of: Vec<(String, usize)>,
}

impl Default for Launchpad {
    fn default() -> Self {
        Self {
            apps: Vec::new(),
            folders: Vec::new(),
            top: Vec::new(),
            hidden: Vec::new(),
            tags: DEFAULT_TAGS.iter().map(|s| (*s).to_owned()).collect(),
            cat_of: Vec::new(),
        }
    }
}

mod apps;
mod order;
mod persist;
mod tags;

#[cfg(test)]
mod tests;
