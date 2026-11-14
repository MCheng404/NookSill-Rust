//! Parsing and serialising `launchpad.dat`; the format is in the module docs.

use super::*;

impl Launchpad {
    /// Parses `launchpad.dat`. Malformed lines are skipped, never reported.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut lp = Self::new();
        let mut cur: Option<usize> = None;
        // The defaults are dropped once, on the first `G` line. Comparing each
        // group against `DEFAULT_TAGS` instead would wipe the first three as
        // soon as a file holds exactly the defaults plus one more.
        let mut tags_seen = false;
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            let mut it = line.split('\t');
            let tag = it.next().unwrap_or("");
            match tag {
                "T" => {
                    let (Some(path), Some(name)) = (it.next(), it.next()) else {
                        continue;
                    };
                    if lp.top.len() >= MAX_TOP {
                        continue;
                    }
                    if let Some(i) = lp.ensure_app(path, name) {
                        lp.top.push(Top::App(i));
                    }
                }
                "F" => {
                    let Some(name) = it.next() else { continue };
                    if lp.folders.len() >= MAX_FOLDERS || lp.top.len() >= MAX_TOP {
                        cur = None;
                        continue;
                    }
                    lp.folders.push(Folder {
                        name: name.to_owned(),
                        apps: Vec::new(),
                    });
                    cur = Some(lp.folders.len() - 1);
                    lp.top.push(Top::Folder(lp.folders.len() - 1));
                }
                "f" => {
                    let (Some(path), Some(name)) = (it.next(), it.next()) else {
                        continue;
                    };
                    let Some(fi) = cur else { continue };
                    if lp.folders[fi].apps.len() >= MAX_IN_FOLDER {
                        continue;
                    }
                    if let Some(i) = lp.ensure_app(path, name) {
                        lp.folders[fi].apps.push(i);
                    }
                }
                "E" => cur = None,
                "H" => {
                    if let Some(path) = it.next() {
                        lp.hidden.push(path.to_owned());
                    }
                }
                // Tag groups: `G\t<name>`, numbered by appearance order.
                "G" => {
                    if let Some(name) = it.next() {
                        if !name.trim().is_empty() && lp.tags.len() < MAX_TAGS {
                            if !tags_seen {
                                lp.tags.clear();
                                tags_seen = true;
                            }
                            lp.tags.push(name.trim().to_owned());
                        }
                    }
                }
                // Manual category: `C\t<app name>\t<number>`.
                "C" => {
                    let (Some(name), Some(cat)) = (it.next(), it.next()) else {
                        continue;
                    };
                    if let Ok(c) = cat.trim().parse::<usize>() {
                        lp.cat_of.push((name.to_owned(), c));
                    }
                }
                _ => {}
            }
        }
        lp
    }

    /// Writes `launchpad.dat`; `parse` reads it back unchanged.
    ///
    /// Fields go through [`clean_field`]: the format is tab separated, so a
    /// stray tab or newline from a rename, an IME commit or a scanned name
    /// would otherwise split one entry into two on the next load.
    #[must_use]
    pub fn serialize(&self) -> String {
        use std::fmt::Write as _;
        let mut s = String::new();
        for t in &self.top {
            match *t {
                Top::App(i) => {
                    let a = &self.apps[i];
                    let _ = writeln!(s, "T\t{}\t{}", clean_field(&a.path), clean_field(&a.name));
                }
                Top::Folder(fi) => {
                    let f = &self.folders[fi];
                    let _ = writeln!(s, "F\t{}", clean_field(&f.name));
                    for &ai in &f.apps {
                        let a = &self.apps[ai];
                        let _ =
                            writeln!(s, "f\t{}\t{}", clean_field(&a.path), clean_field(&a.name));
                    }
                    let _ = writeln!(s, "E");
                }
            }
        }
        for h in &self.hidden {
            let _ = writeln!(s, "H\t{}", clean_field(h));
        }
        for t in &self.tags {
            let _ = writeln!(s, "G\t{}", clean_field(t));
        }
        for (name, cat) in &self.cat_of {
            let _ = writeln!(s, "C\t{}\t{cat}", clean_field(name));
        }
        s
    }
}

/// Replace the bytes that would break the tab/line format.
///
/// Only the write side needs this: `parse` never sees a delimiter inside a
/// field, and keeping the original name in memory avoids surprising the UI.
fn clean_field(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\t' | '\r' | '\n' | '\0' => ' ',
            other => other,
        })
        .collect()
}
