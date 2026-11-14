//! Notes: plain data model plus `[Notes]` section IO.
//!
//! A note is a title plus a body stored as one escaped line. A profile API
//! value cannot contain real newlines: they swallow every later line and
//! void the section. So `\` becomes `\\` and a newline becomes a literal
//! `\n`, and the round trip must be exact.

use crate::ini::Ini;

/// `[Notes]` section name.
pub const SECTION: &str = "Notes";

/// Maximum number of notes.
pub const MAX_NOTES: usize = 32;
/// Title limit in chars, not bytes.
pub const MAX_TITLE: usize = 64;
/// Body limit in chars.
pub const MAX_BODY: usize = 4096;

/// One note.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Note {
    pub title: String,
    pub body: String,
}

impl Note {
    #[must_use]
    pub fn new(title: &str, body: &str) -> Self {
        Self {
            title: clean_one_line(title, MAX_TITLE),
            body: clean_body(body),
        }
    }

    /// List preview: first non-empty body line, or a placeholder.
    #[must_use]
    pub fn preview(&self) -> String {
        self.first_line()
            .unwrap_or_else(|| crate::i18n::t("(Empty)").to_owned())
    }

    /// Display name: falls back to the first body line when the title is empty.
    #[must_use]
    pub fn display_title(&self) -> String {
        if self.title.trim().is_empty() {
            // Ask for the line, not for `preview()`: comparing against the
            // translated placeholder would misread a note whose own text
            // happens to equal it.
            self.first_line()
                .unwrap_or_else(|| crate::i18n::t("Untitled note").to_owned())
        } else {
            self.title.clone()
        }
    }

    #[must_use]
    fn first_line(&self) -> Option<String> {
        self.body
            .lines()
            .find(|l| !l.trim().is_empty())
            .map(|l| truncate_chars(l, 48))
    }
}

/// A whole notebook.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NoteBook {
    pub notes: Vec<Note>,
}

impl NoteBook {
    /// Load from the ini. `N` is clamped; bad rows are skipped, not fatal.
    #[must_use]
    pub fn load(ini: &Ini) -> Self {
        let n = ini
            .get_i32(SECTION, "N")
            .unwrap_or(0)
            .clamp(0, MAX_NOTES as i32);
        let mut notes = Vec::new();
        for i in 0..n {
            let title = ini.get(SECTION, &format!("T{i}")).unwrap_or_default();
            let body = ini.get(SECTION, &format!("B{i}")).unwrap_or_default();
            // Rows with neither title nor body are meaningless; skip them, and
            // also the empty notes produced by a hand-edited `N`.
            if title.is_empty() && body.is_empty() {
                continue;
            }
            notes.push(Note {
                title: clean_one_line(title, MAX_TITLE),
                body: clean_body(&unescape(body)),
            });
        }
        Self { notes }
    }

    /// Write `[Notes]` back.
    ///
    /// The section is removed and rewritten so a shrinking list leaves no stale
    /// `T{i}` / `B{i}` behind.
    pub fn save(&self, ini: &mut Ini) {
        ini.remove_section(SECTION);
        let n = self.notes.len().min(MAX_NOTES);
        ini.set_i32(SECTION, "N", n as i32);
        for (i, note) in self.notes.iter().take(n).enumerate() {
            ini.set(SECTION, &format!("T{i}"), &note.title);
            ini.set(SECTION, &format!("B{i}"), escape(&note.body));
        }
    }

    /// Append a note and return its index, or `None` when full.
    pub fn add(&mut self, title: &str, body: &str) -> Option<usize> {
        if self.notes.len() >= MAX_NOTES {
            return None;
        }
        self.notes.push(Note::new(title, body));
        Some(self.notes.len() - 1)
    }

    /// Replace a note. Returns `false` when the index is out of range.
    pub fn update(&mut self, index: usize, title: &str, body: &str) -> bool {
        let Some(n) = self.notes.get_mut(index) else {
            return false;
        };
        *n = Note::new(title, body);
        true
    }

    /// Remove a note. Returns `false` when the index is out of range.
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.notes.len() {
            return false;
        }
        self.notes.remove(index);
        true
    }
}

/// Escape a body: backslash and newline. Other control chars become spaces.
#[must_use]
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Unescape. An unknown `\x` pair is kept as-is rather than dropping the char.
#[must_use]
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Clean a title: drop newlines and control chars, truncate by chars.
fn clean_one_line(s: &str, max: usize) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    truncate_chars(t.trim(), max)
}

/// Clean a body: keep newlines, drop other control chars, truncate by chars.
fn clean_body(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect();
    truncate_chars(&t, MAX_BODY)
}

fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_roundtrips_newlines_and_backslashes() {
        let s = "第一行\n含\\反斜杠\n\\n 是字面量";
        assert_eq!(unescape(&escape(s)), s);
    }

    #[test]
    fn escape_never_emits_a_real_newline() {
        let out = escape("a\nb\r\nc\td");
        assert!(!out.contains('\n'));
        assert!(!out.contains('\r'));
        assert!(out.contains("\\n"));
    }

    #[test]
    fn unknown_escape_is_kept_not_swallowed() {
        assert_eq!(unescape(r"a\qb"), r"a\qb");
        assert_eq!(unescape("末尾\\"), "末尾\\");
    }

    #[test]
    fn notebook_roundtrips_through_ini() {
        let mut book = NoteBook::default();
        book.add("购物清单", "牛奶\n面包\\坚果").unwrap();
        book.add("想法", "第二行\n第四行").unwrap();
        let mut ini = Ini::new();
        book.save(&mut ini);
        assert_eq!(ini.get_i32(SECTION, "N"), Some(2));
        assert!(!ini.get(SECTION, "B0").unwrap().contains('\n'));

        let back = NoteBook::load(&ini);
        assert_eq!(back, book);
    }

    #[test]
    fn save_clears_stale_entries() {
        let mut book = NoteBook::default();
        for i in 0..3 {
            book.add(&format!("n{i}"), "x").unwrap();
        }
        let mut ini = Ini::new();
        book.save(&mut ini);
        assert!(book.remove(0));
        assert!(book.remove(0));
        book.save(&mut ini);
        assert_eq!(ini.get_i32(SECTION, "N"), Some(1));
        assert!(ini.get(SECTION, "T1").is_none());
        assert!(ini.get(SECTION, "B2").is_none());
        assert_eq!(NoteBook::load(&ini).notes.len(), 1);
    }

    #[test]
    fn load_clamps_a_hostile_count() {
        let mut ini = Ini::new();
        ini.set(SECTION, "N", "999999");
        ini.set(SECTION, "T0", "only one");
        ini.set(SECTION, "B0", "body");
        let book = NoteBook::load(&ini);
        assert_eq!(book.notes.len(), 1);
        assert!(book.notes.len() <= MAX_NOTES);
    }

    #[test]
    fn titles_are_one_line_and_capped() {
        let n = Note::new("坏\n标题\u{1}带控制符", "正文");
        assert!(!n.title.contains('\n'));
        assert!(!n.title.contains('\u{1}'));
        let long = "字".repeat(MAX_TITLE + 20);
        assert_eq!(Note::new(&long, "").title.chars().count(), MAX_TITLE);
    }

    /// The placeholder is not a sentinel: a note whose body literally says
    /// "(empty)" must show that text instead of being renamed.
    #[test]
    fn display_title_does_not_compare_against_the_placeholder() {
        let placeholder = crate::i18n::t("(Empty)");
        let n = Note::new("", placeholder);
        assert_eq!(n.preview(), placeholder);
        assert_eq!(n.display_title(), placeholder);
    }

    #[test]
    fn display_title_falls_back_to_the_first_body_line() {
        let n = Note::new("", "第一行\n第二行");
        assert_eq!(n.display_title(), "第一行");
        assert_eq!(n.preview(), "第一行");
        assert_eq!(
            Note::new("", "").display_title(),
            crate::i18n::t("Untitled note")
        );
    }

    #[test]
    fn add_and_update_respect_bounds() {
        let mut book = NoteBook::default();
        for i in 0..MAX_NOTES {
            assert!(book.add(&format!("n{i}"), "x").is_some());
        }
        assert!(book.add("overflow", "x").is_none());
        assert!(book.update(0, "改过", "新正文"));
        assert_eq!(book.notes[0].title, "改过");
        assert!(!book.update(999, "x", "y"));
        assert!(!book.remove(999));
    }
}
