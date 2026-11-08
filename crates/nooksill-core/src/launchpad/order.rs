//! Top-level order: sorting, drop targets, folder merge and split.

use super::*;

impl Launchpad {
    /// Sorts the grid by display name, case-insensitive.
    ///
    /// Ties keep their relative order. The result is what `serialize` writes,
    /// so a later drag overwrites it, matching the original behaviour.
    pub fn sort_az(&mut self) {
        let mut order: Vec<usize> = (0..self.top.len()).collect();
        order.sort_by(|a, b| {
            let na = self.name_of(self.top[*a]).to_lowercase();
            let nb = self.name_of(self.top[*b]).to_lowercase();
            na.cmp(&nb).then(a.cmp(b))
        });
        let old = self.top.clone();
        for (dst, src) in order.iter().enumerate() {
            self.top[dst] = old[*src];
        }
    }

    /// Display name of a cell; a folder shows its own name.
    #[must_use]
    pub fn name_of(&self, t: Top) -> &str {
        match t {
            Top::App(i) => &self.apps[i].name,
            Top::Folder(i) => &self.folders[i].name,
        }
    }

    /// Number of entries inside a folder.
    #[must_use]
    pub fn folder_len(&self, fi: usize) -> usize {
        self.folders[fi].apps.len()
    }

    /// Page count for `count` cells; zero items still needs one page.
    #[must_use]
    pub fn pages(count: usize, per: usize) -> usize {
        if per == 0 {
            return 1;
        }
        count.div_ceil(per).max(1)
    }

    /// Drops cell `src` onto cell `target`.
    ///
    /// `on_icon` means the pointer was on the icon itself rather than the cell
    /// padding; the UI decides that from the distance to the cell centre.
    ///
    /// Onto a folder appends to it (ignored when full). App onto app makes a
    /// new folder. Anything else reorders. Returns whether to save.
    pub fn drop_at(&mut self, src: usize, target: usize, on_icon: bool) -> bool {
        if src == target || src >= self.top.len() {
            return false;
        }
        let target = target.min(self.top.len() - 1);

        if on_icon {
            if let (Top::App(ai), Top::Folder(fi)) = (self.top[src], self.top[target]) {
                if self.folders[fi].apps.len() < MAX_IN_FOLDER {
                    self.folders[fi].apps.push(ai);
                    self.top.remove(src);
                    return true;
                }
            }
            if let (Top::App(ai), Top::App(ti)) = (self.top[src], self.top[target]) {
                if self.folders.len() < MAX_FOLDERS {
                    self.folders.push(Folder {
                        name: "文件夹".to_owned(),
                        apps: vec![ti, ai],
                    });
                    let fi = self.folders.len() - 1;
                    self.top[target] = Top::Folder(fi);
                    self.top.remove(src);
                    return true;
                }
            }
        }

        let t = self.top.remove(src);
        // Removing the source shifts the target left by one.
        let dst = if src < target { target - 1 } else { target };
        let dst = dst.min(self.top.len());
        self.top.insert(dst, t);
        true
    }

    /// Moves an app in front of another app in the top list.
    ///
    /// The filtered view addresses cells by app index, not by top index. Only
    /// reorders: two apps adjacent under a filter may be far apart in the full
    /// grid, and merging there would be hard to undo. Returns false if either
    /// app is inside a folder.
    pub fn move_app_before(&mut self, src_ai: usize, dst_ai: usize) -> bool {
        if src_ai == dst_ai {
            return false;
        }
        let slot = |lp: &Self, ai: usize| lp.top.iter().position(|t| *t == Top::App(ai));
        let (Some(src), Some(dst)) = (slot(self, src_ai), slot(self, dst_ai)) else {
            return false;
        };
        let t = self.top.remove(src);
        // Removing the source shifts the target left by one.
        let dst = if src < dst { dst - 1 } else { dst };
        self.top.insert(dst.min(self.top.len()), t);
        true
    }

    /// Unfolds a folder into the slot it occupied, keeping the member order.
    pub fn dissolve(&mut self, k: usize) -> bool {
        let Some(Top::Folder(fi)) = self.top.get(k).copied() else {
            return false;
        };
        let members = std::mem::take(&mut self.folders[fi].apps);
        self.top.remove(k);
        for (n, ai) in members.into_iter().enumerate() {
            self.top.insert(k + n, Top::App(ai));
        }
        true
    }

    /// Takes one app out of a folder, then normalises the folder.
    pub fn take_from_folder(&mut self, fi: usize, idx: usize, how: TakeOut) -> bool {
        let Some(fo) = self.folders.get(fi) else {
            return false;
        };
        if idx >= fo.apps.len() {
            return false;
        }
        let ai = self.folders[fi].apps.remove(idx);
        match how {
            TakeOut::ToTop => {
                if self.top.len() < MAX_TOP {
                    self.top.push(Top::App(ai));
                }
            }
            TakeOut::Hide => {
                let path = self.apps[ai].path.clone();
                if !self.is_hidden(&path) {
                    self.hidden.push(path);
                }
            }
            TakeOut::Drop => {}
        }
        self.cleanup_folders();
        true
    }

    /// Drops empty folders and unfolds those left with a single member.
    pub fn cleanup_folders(&mut self) {
        let mut k = self.top.len();
        while k > 0 {
            k -= 1;
            let Top::Folder(fi) = self.top[k] else {
                continue;
            };
            match self.folders[fi].apps.len() {
                0 => {
                    self.top.remove(k);
                }
                1 => {
                    let only = self.folders[fi].apps[0];
                    self.top[k] = Top::App(only);
                }
                _ => {}
            }
        }
    }

    /// Renames an app or folder cell; blank input is rejected.
    pub fn rename(&mut self, k: usize, name: &str) -> bool {
        if name.trim().is_empty() {
            return false;
        }
        match self.top.get(k).copied() {
            Some(Top::App(i)) => self.apps[i].name = name.to_owned(),
            Some(Top::Folder(i)) => self.folders[i].name = name.to_owned(),
            None => return false,
        }
        true
    }

    /// Removes a cell; the apps stay in the table and may come back.
    pub fn remove_top(&mut self, k: usize) -> bool {
        if k >= self.top.len() {
            return false;
        }
        self.top.remove(k);
        true
    }
}
