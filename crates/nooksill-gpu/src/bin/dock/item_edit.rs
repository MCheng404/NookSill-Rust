//! Item mutation: settings edits, icon changes and external drops.

use super::sys::item_from_drop;
use super::*;

impl App {
    /// Change one item's fields and handle regrouping.
    ///
    /// A group is not a field but the item's position: joining one removes the
    /// item from the bar. So remove it from its old slot first; the reverse
    /// order loses the item.
    pub(super) fn apply_item_edit(&mut self, index: usize, edit: ItemEdit) {
        if index >= self.raw.len() {
            return;
        }
        // Remove from the bar first (the item may sit inside a group). The
        // clone keeps its children, so a rename never drops them.
        let mut item = self.raw[index].clone();
        item.name = edit.name;
        item.path = edit.path;
        item.args = edit.args;
        self.raw.remove(index);

        // Re-insert: stay at the original slot when no group is chosen.
        match edit.group {
            None => {
                let at = index.min(self.raw.len());
                self.raw.insert(at, item);
            }
            Some(g) => {
                // The group index may have shifted left by the removal.
                let g = if index < g { g - 1 } else { g };
                if g >= self.raw.len() || self.raw[g].kind != ItemKind::Group {
                    let at = index.min(self.raw.len());
                    self.raw.insert(at, item);
                } else if item.kind == ItemKind::Group {
                    // Groups cannot nest.
                    self.status(nooksill_core::i18n::t(
                        "A group cannot contain another group",
                    ));
                    let at = index.min(self.raw.len());
                    self.raw.insert(at, item);
                } else {
                    // `add_kid` consumes the item on failure, so ask
                    // `can_add_kid` first.
                    match group::can_add_kid(&self.raw[g], &item) {
                        Err(e) => {
                            self.status(nooksill_core::i18n::fmt(
                                "Could not group: {0}",
                                &[e.to_string().as_str()],
                            ));
                            let at = index.min(self.raw.len());
                            self.raw.insert(at, item);
                        }
                        Ok(()) => {
                            let _ = group::add_kid(&mut self.raw[g], item);
                        }
                    }
                }
            }
        }
        self.apply_item_change();
        self.status(nooksill_core::i18n::t("Saved"));
    }

    /// Move an item by `delta` (-1 or +1).
    pub(super) fn move_item(&mut self, index: usize, delta: i32) {
        if index >= self.raw.len() || delta == 0 {
            return;
        }
        let target = index as i32 + delta;
        if target < 0 || target as usize >= self.raw.len() {
            return;
        }
        let item = self.raw.remove(index);
        self.raw.insert(target as usize, item);
        self.apply_item_change();
    }

    /// Pick a custom icon for an item.
    ///
    /// Still images decode through WIC; `.ico` / `.exe` / `.dll` go through the
    /// shell image list. The filter lists both groups.
    pub(super) fn change_icon(&mut self, index: usize) {
        if index >= self.raw.len() {
            return;
        }
        let owner = self.settings_hwnd();
        let filters = [
            (
                nooksill_core::i18n::t("Images, icons and executables"),
                "*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.webp;*.ico;*.exe;*.dll",
            ),
            (nooksill_core::i18n::t("All files"), "*.*"),
        ];
        let Some(path) = nooksill_platform::filedialog::open_file(
            nooksill_core::i18n::t("Change icon"),
            owner,
            &filters,
        ) else {
            return;
        };
        self.raw[index].icon_src = path.to_string_lossy().into_owned();
        // `replace_items` keys textures by `path`, but this changes `icon_src`,
        // so clear explicitly or the old icon sticks.
        self.dock.clear_icon(index);
        // The path is unchanged but `icon_src` is not: drop the in-flight
        // suppression, or `request_missing_icons` assumes a request is pending.
        self.icons.forget(index, &self.raw[index].path);
        self.apply_item_change();
        self.status(nooksill_core::i18n::fmt(
            "Icon changed for {0}",
            &[self.raw[index].name.to_string().as_str()],
        ));
    }

    /// Restore the default icon.
    pub(super) fn reset_icon(&mut self, index: usize) {
        let Some(item) = self.raw.get_mut(index) else {
            return;
        };
        if item.icon_src.is_empty() {
            return;
        }
        let name = item.name.clone();
        item.icon_src.clear();
        self.dock.clear_icon(index);
        // Same as above: allow the default icon to be queued again.
        self.icons.forget(index, &self.raw[index].path);
        self.apply_item_change();
        self.status(nooksill_core::i18n::fmt(
            "Default icon restored for {0}",
            &[name.to_string().as_str()],
        ));
    }

    /// Move a child inside its group; returns whether it moved.
    pub(super) fn move_kid(&mut self, gi: usize, ki: usize, delta: i32) -> bool {
        let Some(g) = self.raw.get_mut(gi) else {
            return false;
        };
        let name = g.name.clone();
        if group::move_kid(g, ki, delta).is_err() {
            return false;
        }
        // A group icon tiles its first 4 children, so a new order needs a refetch.
        self.dock.clear_icon(gi);
        self.apply_item_change();
        self.status(nooksill_core::i18n::fmt(
            "Reordered children of {0}",
            &[name.to_string().as_str()],
        ));
        true
    }

    /// Handle paths dropped from Explorer.
    ///
    /// Directories become `Folder` items, the rest `File` items, and `.url`
    /// becomes a URL item. The drop position matters: insert at the hovered
    /// slot, group onto a group icon, and skip paths already on the dock.
    pub(super) fn step_drop(&mut self) -> bool {
        let batch = self.dock.take_dropped_paths();
        if batch.paths.is_empty() {
            return false;
        }
        // Dropped items are pinned and may not be placed after the running
        // segment, so clamp the drop index to the pinned range.
        let mut at = batch.index.unwrap_or(self.raw.len()).min(self.raw.len());
        let target_group = batch
            .group
            .filter(|&g| g < self.raw.len() && self.raw[g].kind == ItemKind::Group);

        let mut added = 0usize;
        let mut grouped = 0usize;
        let mut dup = 0usize;
        let mut failed = 0usize;
        for path in &batch.paths {
            let Some(item) = item_from_drop(path) else {
                failed += 1;
                continue;
            };
            // Dedupe by path case-insensitively, since Windows paths ignore case.
            if self
                .raw
                .iter()
                .any(|it| it.path.eq_ignore_ascii_case(&item.path))
            {
                dup += 1;
                continue;
            }

            // Re-validate each round: earlier inserts shift `raw`, so a cached
            // group index can point at a different item.
            let group_now = target_group
                .filter(|&g| self.raw.get(g).is_some_and(|it| it.kind == ItemKind::Group));
            if let Some(g) = group_now {
                // `add_kid` consumes the item on failure, so test first.
                let can = self
                    .raw
                    .get(g)
                    .is_some_and(|grp| group::can_add_kid(grp, &item).is_ok());
                if can {
                    if let Some(grp) = self.raw.get_mut(g) {
                        if group::add_kid(grp, item.clone()).is_ok() {
                            // A group icon tiles its children, so refetch it.
                            self.dock.clear_icon(g);
                            grouped += 1;
                            continue;
                        }
                    }
                }
                // Full or wrong type: insert into the bar rather than drop the file.
            }

            self.raw.insert(at, item);
            at += 1;
            added += 1;
        }

        if added == 0 && grouped == 0 {
            if dup > 0 {
                self.status(nooksill_core::i18n::fmt(
                    "{0} items already on the Dock",
                    &[dup.to_string().as_str()],
                ));
            } else if failed > 0 {
                self.status(nooksill_core::i18n::fmt(
                    "{0} paths could not be recognised",
                    &[failed.to_string().as_str()],
                ));
            }
            return dup > 0 || failed > 0;
        }
        self.apply_item_change();
        let mut parts = Vec::new();
        if added > 0 {
            parts.push(nooksill_core::i18n::fmt(
                "Inserted {1} items at position {0}",
                &[
                    (at.saturating_sub(added) + 1).to_string().as_str(),
                    added.to_string().as_str(),
                ],
            ));
        }
        if grouped > 0 {
            parts.push(nooksill_core::i18n::fmt(
                "Added {0} items to groups",
                &[grouped.to_string().as_str()],
            ));
        }
        if dup > 0 {
            parts.push(nooksill_core::i18n::fmt(
                "Skipped {0} duplicates",
                &[dup.to_string().as_str()],
            ));
        }
        self.status(parts.join(nooksill_core::i18n::t(", ")));
        true
    }
}
