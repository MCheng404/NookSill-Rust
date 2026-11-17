//! Plain data views shared by settings pages and the host commands.

/// Editable fields of an item; name, path and arguments are applied together.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemEdit {
    pub name: String,
    pub path: String,
    pub args: String,
    /// Group index to merge into; `None` keeps the item on the bar.
    pub group: Option<usize>,
}

/// One row of the icon-library preview.
#[derive(Clone)]
pub struct LibIcon {
    /// Item name the row belongs to.
    pub name: String,
    /// File that would be used; empty when nothing matched.
    pub path: String,
    /// Decoded icon for the preview; `None` when nothing matched.
    pub tex: Option<egui::TextureHandle>,
}

impl std::fmt::Debug for LibIcon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LibIcon")
            .field("name", &self.name)
            .field("path", &self.path)
            .field("has_icon", &self.tex.is_some())
            .finish()
    }
}

/// The updater as seen by the UI.
///
/// Strings and flags only: this layer does not know the network types, the GPU
/// layer translates `update::Release` into them.
#[derive(Clone, Debug, Default)]
pub struct UpdateView {
    /// One status line ("checking", "up to date", "found 1.0.1", or an error).
    pub status: String,
    /// A newer version can be downloaded.
    pub available: bool,
    /// Downloaded and verified, waiting for a restart.
    pub ready: bool,
    /// A request is in flight; buttons are disabled.
    pub busy: bool,
    /// Version text of the offered release, for the popup.
    pub version: String,
    /// Release notes, trimmed by the popup.
    pub notes: String,
    /// The offered release belongs to the beta channel.
    pub prerelease: bool,
    /// Bytes received by the download in progress.
    pub bytes_done: u64,
    /// Asset size from release metadata; 0 means unknown.
    pub bytes_total: u64,
    /// Smoothed download rate in bytes per second; 0 means unknown yet.
    pub bytes_per_sec: u64,
    /// Seconds left at that rate; 0 means unknown.
    pub eta_secs: u32,
}

impl UpdateView {
    /// `None` outside an active download, or when the server gave no size.
    #[must_use]
    pub fn progress(&self) -> Option<f32> {
        if !self.busy || self.bytes_total == 0 {
            return None;
        }
        Some((self.bytes_done as f32 / self.bytes_total as f32).clamp(0.0, 1.0))
    }
}
