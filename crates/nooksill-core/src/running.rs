//! Decide whether an item's app is running.
//!
//! Match rules, in priority order:
//!
//! 1. non-empty `item.target` -- the real exe path from `.lnk` resolution;
//! 2. else `item.path` ending in `.exe`;
//! 3. otherwise (unresolved `.lnk`, folder, url, separator) no match.
//!
//! Rule 3 is deliberate: never guess by file name. A shortcut named after
//! the app usually points at a differently named exe, so a guess lights the
//! indicator on the wrong item; a dark indicator is the lesser failure.

use std::collections::HashSet;

use crate::item::{Item, ItemKind};

/// Key this item uses to look itself up in the running-exe set.
///
/// Always lowercase; Windows paths are case-insensitive.
#[must_use]
pub fn running_key(item: &Item) -> Option<String> {
    if item.kind != ItemKind::File {
        return None;
    }
    if !item.target.is_empty() {
        return Some(item.target.to_lowercase());
    }
    if item.path.to_lowercase().ends_with(".exe") {
        return Some(item.path.to_lowercase());
    }
    None
}

/// Refresh every item's `running` flag from the lowercase exe path set.
///
/// Returns whether any item changed, so the main loop can skip a redraw.
/// Redrawing on every scan drops idle CPU from 0% to tens of frames per second.
pub fn refresh_running(items: &mut [Item], running: &HashSet<String>) -> bool {
    let mut changed = false;
    for item in items.iter_mut() {
        let now = running_key(item).is_some_and(|k| running.contains(&k));
        if item.running != now {
            item.running = now;
            changed = true;
        }
    }
    changed
}

/// Compute which unpinned running apps should appear.
///
/// * `running` -- every running exe, lowercase.
/// * `pinned` -- exes already pinned to the dock, not repeated.
/// * `prev` -- previous round's temp items. Preserve their relative order
///   and keep only the ones still alive.
/// * `limit` -- cap; the bar scales with item count and gets squashed.
/// * `skip` -- exe to ignore (the dock itself), empty means none.
///
/// Order is old first, new appended sorted by name: `EnumWindows` returns a Z
/// order that changes every round, and reordering by it makes icons shuffle and
/// forces a texture re-upload per slot.
#[must_use]
pub fn temp_keys(
    running: &HashSet<String>,
    pinned: &HashSet<String>,
    prev: &[String],
    limit: usize,
    skip: &str,
) -> Vec<String> {
    let keep =
        |e: &str| running.contains(e) && !pinned.contains(e) && !e.eq_ignore_ascii_case(skip);
    let mut out: Vec<String> = prev.iter().filter(|e| keep(e)).cloned().collect();
    let mut fresh: Vec<String> = running
        .iter()
        .filter(|e| {
            !pinned.contains(*e)
                && !prev.contains(*e)
                && !e.eq_ignore_ascii_case(skip)
                && !e.is_empty()
        })
        .cloned()
        .collect();
    fresh.sort();
    out.extend(fresh);
    out.truncate(limit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_item(name: &str, path: &str) -> Item {
        Item::new(ItemKind::File, name, path)
    }

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    /// `target` beats `path`: a `.lnk` path is not an exe, only target matches.
    #[test]
    fn target_wins_over_path() {
        let mut it = file_item(
            "Excel",
            r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Excel.lnk",
        );
        it.target = r"C:\Program Files\Microsoft Office\root\Office16\EXCEL.EXE".to_owned();
        assert_eq!(
            running_key(&it),
            Some(r"c:\program files\microsoft office\root\office16\excel.exe".to_owned())
        );
    }

    /// An `.exe` item without a target uses its own path.
    #[test]
    fn exe_path_is_used_when_no_target() {
        let it = file_item("记事本", r"C:\Windows\System32\notepad.exe");
        assert_eq!(
            running_key(&it),
            Some(r"c:\windows\system32\notepad.exe".to_owned())
        );
    }

    /// An unresolved `.lnk` never matches: a dark indicator beats a guess.
    #[test]
    fn unresolved_lnk_never_matches() {
        let it = file_item("网易云音乐", r"C:\Users\X\Desktop\网易云音乐.lnk");
        assert_eq!(running_key(&it), None);
        // Even an identically named exe in the set must not match.
        let mut items = [it];
        let changed = refresh_running(&mut items, &set(&[r"c:\x\网易云音乐.exe"]));
        assert!(!changed);
        assert!(!items[0].running);
    }

    /// Non-file kinds (separator, url, folder, trash) never participate.
    #[test]
    fn non_file_kinds_never_match() {
        for kind in [
            ItemKind::Separator,
            ItemKind::Url,
            ItemKind::Folder,
            ItemKind::Trash,
        ] {
            let mut it = Item::new(kind, "x", r"C:\Windows\System32\notepad.exe");
            // Set target too, to prove the kind check runs first.
            it.target = r"C:\Windows\System32\notepad.exe".to_owned();
            assert_eq!(running_key(&it), None, "{kind:?} 不该参与匹配");
        }
    }

    /// Case must be ignored: config and system casing differ.
    #[test]
    fn matching_is_case_insensitive() {
        let mut items = [file_item("记事本", r"C:\Windows\System32\NOTEPAD.EXE")];
        let changed = refresh_running(&mut items, &set(&[r"c:\windows\system32\notepad.exe"]));
        assert!(changed);
        assert!(items[0].running);
    }

    /// Return true only on an actual state change.
    ///
    /// Performance constraint: the main loop scans every 1.2 s, and returning
    /// true unconditionally forces a redraw that breaks idle CPU at 0%.
    #[test]
    fn reports_change_only_when_state_flips() {
        let mut items = [file_item("记事本", r"C:\Windows\System32\notepad.exe")];
        let running = set(&[r"c:\windows\system32\notepad.exe"]);

        assert!(refresh_running(&mut items, &running), "第一次应当算变化");
        assert!(!refresh_running(&mut items, &running), "状态没变不该再报");
        assert!(refresh_running(&mut items, &set(&[])), "退出后应当算变化");
        assert!(!items[0].running);
    }

    /// Separators among other items stay untouched.
    #[test]
    fn separator_is_left_alone() {
        let mut items = [
            file_item("记事本", r"C:\Windows\System32\notepad.exe"),
            Item::new(ItemKind::Separator, "", ""),
        ];
        let changed = refresh_running(&mut items, &set(&[r"c:\windows\system32\notepad.exe"]));
        assert!(changed);
        assert!(items[0].running);
        assert!(!items[1].running, "分割线的 running 必须保持 false");
    }

    /// Temp items: existing ones keep their slot, new ones append.
    ///
    /// Stable order matters because `EnumWindows` Z-order changes every round;
    /// reordering by it makes icons move between scans and hard to click.
    #[test]
    fn temp_keeps_existing_order_and_appends_new() {
        let running = set(&[r"c:.exe", r"c:.exe", r"c:\c.exe"]);
        let prev = vec![r"c:\c.exe".to_owned(), r"c:.exe".to_owned()];
        let got = temp_keys(&running, &HashSet::new(), &prev, 8, "");
        assert_eq!(got, vec![r"c:\c.exe", r"c:.exe", r"c:.exe"]);
    }

    /// Exited apps disappear from the list.
    #[test]
    fn temp_drops_vanished_apps() {
        let running = set(&[r"c:.exe"]);
        let prev = vec![r"c:.exe".to_owned(), r"c:.exe".to_owned()];
        let got = temp_keys(&running, &HashSet::new(), &prev, 8, "");
        assert_eq!(got, vec![r"c:.exe"]);
    }

    /// Pinned apps do not repeat, even when still in `prev`.
    #[test]
    fn temp_excludes_pinned() {
        let running = set(&[r"c:.exe", r"c:.exe"]);
        let pinned = set(&[r"c:.exe"]);
        let prev = vec![r"c:.exe".to_owned()];
        let got = temp_keys(&running, &pinned, &prev, 8, "");
        assert_eq!(got, vec![r"c:.exe"]);
    }

    /// The cap must apply, and truncation is at the tail so old entries stay.
    #[test]
    fn temp_honours_limit() {
        let running = set(&[r"c:.exe", r"c:.exe", r"c:\c.exe"]);
        let prev = vec![r"c:.exe".to_owned()];
        let got = temp_keys(&running, &HashSet::new(), &prev, 2, "");
        assert_eq!(got, vec![r"c:.exe", r"c:.exe"]);
    }

    /// The dock itself must not appear, or it sees its own window and adds a
    /// phantom icon.
    #[test]
    fn temp_skips_self() {
        let running = set(&[r"c:\nooksill-dock.exe", r"c:.exe"]);
        let got = temp_keys(&running, &HashSet::new(), &[], 8, r"c:\nooksill-dock.exe");
        assert_eq!(got, vec![r"c:.exe"]);
    }
}
