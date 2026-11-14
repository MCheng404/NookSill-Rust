//! Tag groups (categories) and the category of each app.

use super::*;

impl Launchpad {
    #[must_use]
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// Renames a group; blank or over-cap names are rejected.
    pub fn set_tag(&mut self, i: usize, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > TAG_NAME_CAP || i >= self.tags.len() {
            return false;
        }
        self.tags[i] = name.to_owned();
        true
    }

    /// Appends a group and returns its 1-based number, or `None` when full.
    pub fn add_tag(&mut self, name: &str) -> Option<usize> {
        if self.tags.len() >= MAX_TAGS {
            return None;
        }
        let name = name.trim();
        let name = if name.is_empty() {
            crate::i18n::fmt(
                "Tag group {0}",
                &[(self.tags.len() + 1).to_string().as_str()],
            )
        } else {
            name.chars().take(TAG_NAME_CAP).collect()
        };
        self.tags.push(name);
        Some(self.tags.len())
    }

    /// Removes a group; at least one always remains.
    ///
    /// Manual assignments to that group are dropped, later ones shift down.
    pub fn remove_tag(&mut self, i: usize) -> bool {
        if i >= self.tags.len() || self.tags.len() <= 1 {
            return false;
        }
        self.tags.remove(i);
        let gone = i + 1;
        self.cat_of.retain(|(_, c)| *c != gone);
        for (_, c) in &mut self.cat_of {
            if *c > gone {
                *c -= 1;
            }
        }
        true
    }

    /// Keyword classifier: 1 office, 2 system, 3 tools.
    ///
    /// Only `System32` / `SysWOW64` / `shell:` count as system. Matching a bare
    /// `Windows` would hit every Start Menu `.lnk`, whose path runs through
    /// `C:\ProgramData\Microsoft\Windows\Start Menu\...`.
    #[must_use]
    pub(super) fn classify(app: &App) -> usize {
        let path = app.path.to_lowercase();
        let name = app.name.to_lowercase();
        let any = |hay: &str, keys: &[&str]| keys.iter().any(|k| hay.contains(k));
        if any(
            &path,
            &["\\windows\\system32\\", "\\windows\\syswow64\\", "shell:"],
        ) || any(
            &name,
            &["开始", "设置", "回收站", "此电脑", "控制面板", "资源管理器"],
        ) {
            return 2;
        }
        if any(
            &name,
            &[
                "word",
                "excel",
                "powerpoint",
                "ppt",
                "office",
                "wps",
                "onenote",
                "outlook",
                "onedrive",
                "notion",
            ],
        ) {
            return 1;
        }
        3
    }

    /// Category of an app: 1..=tags.len(), manual assignment first.
    #[must_use]
    pub fn category_of(&self, ai: usize) -> usize {
        let n = self.tags.len().max(1);
        if let Some(app) = self.apps.get(ai) {
            if let Some((_, c)) = self
                .cat_of
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(&app.name))
            {
                return (*c).clamp(1, n);
            }
            return Self::classify(app).clamp(1, n);
        }
        1
    }

    /// Pins an app to a category; `None` returns it to the heuristic.
    pub fn set_category(&mut self, ai: usize, cat: Option<usize>) {
        let Some(app) = self.apps.get(ai) else {
            return;
        };
        let name = app.name.clone();
        self.cat_of.retain(|(n, _)| !n.eq_ignore_ascii_case(&name));
        if let Some(c) = cat {
            self.cat_of.push((name, c.clamp(1, self.tags.len().max(1))));
        }
    }

    /// Apps matching a category and a query, in the order the user placed them.
    ///
    /// `cat == 0` means all. Ordering follows the grid, not the scan order of
    /// `apps`: otherwise reordering under a filter would change nothing on
    /// screen. Folders have no category, so the filtered view lists apps only.
    #[must_use]
    pub fn visible_in(&self, cat: usize, query: &str) -> Vec<usize> {
        let q = query.to_lowercase();
        let mut out: Vec<usize> = self
            .apps
            .iter()
            .enumerate()
            .filter(|(i, a)| {
                !self.is_hidden(&a.path)
                    && (cat == 0 || self.category_of(*i) == cat)
                    && (q.is_empty() || a.name.to_lowercase().contains(&q))
            })
            .map(|(i, _)| i)
            .collect();
        out.sort_by_key(|ai| self.app_slot(*ai));
        out
    }

    /// Sort key by grid position: `(top index, 0)` for a top-level app,
    /// `(folder index, member index + 1)` inside a folder. Unplaced apps get
    /// `usize::MAX` so they sort last.
    pub(super) fn app_slot(&self, ai: usize) -> (usize, usize) {
        for (ti, t) in self.top.iter().enumerate() {
            match *t {
                Top::App(a) if a == ai => return (ti, 0),
                Top::Folder(fi) => {
                    if let Some(k) = self.folders[fi].apps.iter().position(|a| *a == ai) {
                        return (ti, k + 1);
                    }
                }
                _ => {}
            }
        }
        (usize::MAX, 0)
    }
}
