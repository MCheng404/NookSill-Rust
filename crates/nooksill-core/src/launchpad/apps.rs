//! Queries and edits over apps and top-level cells.

use super::*;

impl Launchpad {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn apps(&self) -> &[App] {
        &self.apps
    }

    #[must_use]
    pub fn folders(&self) -> &[Folder] {
        &self.folders
    }

    #[must_use]
    pub fn top(&self) -> &[Top] {
        &self.top
    }

    /// Number of top-level cells, not of apps.
    #[must_use]
    pub fn len(&self) -> usize {
        self.top.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.top.is_empty()
    }

    /// Same app when either the path or the name matches, case-insensitive.
    ///
    /// The name is checked as well because a moved install keeps its name but
    /// not its path.
    pub(super) fn find_app(&self, path: &str, name: &str) -> Option<usize> {
        self.apps
            .iter()
            .position(|a| a.path.eq_ignore_ascii_case(path) || a.name.eq_ignore_ascii_case(name))
    }

    /// Reuses an existing entry, otherwise appends one up to [`MAX_APPS`].
    pub(super) fn ensure_app(&mut self, path: &str, name: &str) -> Option<usize> {
        if let Some(i) = self.find_app(path, name) {
            return Some(i);
        }
        if self.apps.len() >= MAX_APPS {
            return None;
        }
        self.apps.push(App {
            name: name.to_owned(),
            path: path.to_owned(),
        });
        Some(self.apps.len() - 1)
    }

    /// Case-insensitive path lookup in the hidden list.
    #[must_use]
    pub fn is_hidden(&self, path: &str) -> bool {
        self.hidden.iter().any(|h| h.eq_ignore_ascii_case(path))
    }

    /// Whether the app is on screen, either on top or inside a folder.
    pub(super) fn is_placed(&self, idx: usize) -> bool {
        self.top.contains(&Top::App(idx)) || self.folders.iter().any(|f| f.apps.contains(&idx))
    }

    /// Appends scanned apps that are neither hidden nor already placed.
    ///
    /// Stops silently at the caps, keeping whatever was already on the grid.
    pub fn absorb(&mut self, scanned: &[AppEntry]) {
        for e in scanned {
            if self.is_hidden(&e.path) {
                continue;
            }
            if let Some(i) = self.find_app(&e.path, &e.name) {
                if self.is_placed(i) {
                    continue;
                }
            }
            let Some(i) = self.ensure_app(&e.path, &e.name) else {
                break;
            };
            if self.top.len() >= MAX_TOP {
                break;
            }
            self.top.push(Top::App(i));
        }
    }

    /// Adds an app by hand (dropped file, URL) and un-hides it.
    ///
    /// Returns `None` when it already owns a cell, so no cell is duplicated.
    pub fn add(&mut self, name: &str, path: &str) -> Option<usize> {
        if self.is_hidden(path) {
            self.hidden.retain(|h| !h.eq_ignore_ascii_case(path));
        }
        let i = self.ensure_app(path, name)?;
        if self.is_placed(i) || self.top.len() >= MAX_TOP {
            return None;
        }
        self.top.push(Top::App(i));
        Some(i)
    }

    /// Case-insensitive substring search over names, hidden apps skipped.
    ///
    /// An empty query returns nothing: an empty box is not a search.
    #[must_use]
    pub fn filter(&self, query: &str) -> Vec<usize> {
        if query.is_empty() {
            return Vec::new();
        }
        let q = query.to_lowercase();
        self.apps
            .iter()
            .enumerate()
            .filter(|(_, a)| !self.is_hidden(&a.path) && a.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }
}
