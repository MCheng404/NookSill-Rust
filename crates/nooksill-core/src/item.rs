//! Dock item model and `[Items]` / `[Groups]` serialization.
//!
//! An item line is `type|appmode|name|path|icon_src|fit|args`. Field limits
//! come from `lstrcpynW` calls (the NUL counts, so one unit less is usable)
//! and truncation counts UTF-16 units, not `char`s.

use crate::MAX_KIDS;

/// `name` buffer size including NUL. Mirrors `Item.name[128]`.
pub const NAME_CAP: usize = 128;
/// `path` buffer size. Mirrors `Item.path[1024]`.
pub const PATH_CAP: usize = 1024;
/// `icon_src` buffer size. Mirrors `Item.icon_src[512]`.
pub const ICON_CAP: usize = 512;
/// `args` buffer size. Mirrors `Item.args[260]`.
pub const ARGS_CAP: usize = 260;
/// Resolved launch target buffer size. Mirrors `Item.target[MAX_PATH]`.
pub const TARGET_CAP: usize = 260;

/// Fields per item line.
const FIELD_COUNT: usize = 7;

/// Item type. The value is the `type` field stored in the ini.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum ItemKind {
    #[default]
    File = 0,
    Folder = 1,
    Url = 2,
    Separator = 3,
    Group = 4,
    Trash = 5,
    Note = 6,
}

impl ItemKind {
    /// Returns `None` for out-of-range codes; the caller must drop the whole
    /// line rather than guess a type.
    #[must_use]
    pub const fn from_code(v: i32) -> Option<Self> {
        Some(match v {
            0 => Self::File,
            1 => Self::Folder,
            2 => Self::Url,
            3 => Self::Separator,
            4 => Self::Group,
            5 => Self::Trash,
            6 => Self::Note,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn code(self) -> i32 {
        self as i32
    }

    #[must_use]
    pub const fn is_separator(self) -> bool {
        matches!(self, Self::Separator)
    }

    /// Groups and notes have a dedicated draw path and skip the plate
    /// (mirrors the check in `RenderFrame`).
    #[must_use]
    pub const fn has_own_icon(self) -> bool {
        matches!(self, Self::Group | Self::Note)
    }
}

/// One dock item.
///
/// The first half holds persisted fields (written to the ini), the second half
/// runtime state (icon bitmap, animation interpolation, running flag). Runtime
/// state is never serialized.
#[derive(Clone, Debug)]
pub struct Item {
    pub kind: ItemKind,
    /// How a web app opens: 0 = default browser, 1 = `--app` window.
    pub app_mode: i32,
    /// Icon fit mode (0 = letterbox, other values match `FitBitmap`).
    pub fit: i32,
    /// Whether the icon is cached under `icons/` (the `fav` flag).
    pub fav: bool,
    /// Temporary item (running but not pinned). Never written to the ini.
    pub temp: bool,

    pub name: String,
    pub path: String,
    pub icon_src: String,
    pub args: String,
    /// Resolved launch target, after .lnk dereference and env expansion.
    pub target: String,

    /// Current icon edge length, interpolated by the grow animation.
    pub cur: f32,
    /// Horizontal offset during reordering (bounce animation).
    pub slide: f32,
    /// Previous frame's slot left edge, used to seed `slide`.
    pub prev_l: f32,
    /// Running state, filled by the window scanner.
    pub running: bool,
    /// Tick when the launch bounce started (`GetTickCount` semantics).
    pub bounce: u32,

    /// Children. Only used by `ItemKind::Group`, capped at [`MAX_KIDS`].
    pub kids: Vec<Item>,
}

impl Item {
    /// Build an item with only the persisted fields set.
    #[must_use]
    pub fn new(kind: ItemKind, name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            kind,
            app_mode: 0,
            fit: 0,
            fav: false,
            temp: false,
            name: truncate_units(&name.into(), NAME_CAP - 1),
            path: truncate_units(&path.into(), PATH_CAP - 1),
            icon_src: String::new(),
            args: String::new(),
            target: String::new(),
            cur: 0.0,
            slide: 0.0,
            prev_l: 0.0,
            running: false,
            bounce: 0,
            kids: Vec::new(),
        }
    }

    /// Parse a `type|appmode|name|path|icon_src|fit|args` line.
    ///
    /// Fails like `ParseItemLine`: fewer than 4 fields, or `type` outside
    /// 0..=6. An empty `icon_src` means no custom icon.
    #[must_use]
    pub fn parse_line(line: &str) -> Option<Self> {
        let mut f = line.splitn(FIELD_COUNT, '|');
        let f0 = f.next()?;
        let f1 = f.next();
        let f2 = f.next();
        let f3 = f.next()?;
        let kind = ItemKind::from_code(atoi_like(f0))?;
        let app_mode = f1.map_or(0, atoi_like);
        let name = f2.map_or("", |v| v);
        let path = f3;
        let icon_src = f.next().unwrap_or("");
        let fit = f.next().map_or(0, atoi_like);
        let args = f.next().unwrap_or("");

        let mut it = Self::new(kind, name, path);
        it.app_mode = app_mode;
        it.fit = fit;
        it.icon_src = truncate_units(icon_src, ICON_CAP - 1);
        it.args = truncate_units(args, ARGS_CAP - 1);
        Some(it)
    }

    /// Serialize to one line. Control chars and `|` are cleaned first, see
    /// [`Self::sanitize_for_ini`].
    #[must_use]
    pub fn to_line(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}",
            self.kind.code(),
            self.app_mode,
            Self::sanitize_for_ini(&self.name),
            Self::sanitize_for_ini(&self.path),
            Self::sanitize_for_ini(&self.icon_src),
            self.fit,
            Self::sanitize_for_ini(&self.args),
        )
    }

    /// Whether this item should be written to the ini. Only `temp` is skipped;
    /// built-in items (trash, note) persist, their path being a `shell:` URI.
    #[must_use]
    pub fn is_persistable(&self) -> bool {
        !self.temp
    }

    /// Add a child. Returns `false` past [`MAX_KIDS`].
    pub fn add_kid(&mut self, kid: Item) -> bool {
        if self.kids.len() >= MAX_KIDS {
            return false;
        }
        self.kids.push(kid);
        true
    }

    /// Clean a field so it cannot break the ini line or field structure.
    ///
    /// - `\r` `\n` `\0` become spaces: a profile API value cannot span lines,
    ///   a newline voids the whole section.
    /// - `|` becomes the full-width form: it is the field separator, and a
    ///   raw `|` in a name silently shifts every field on the next save.
    #[must_use]
    pub fn sanitize_for_ini(s: &str) -> String {
        s.chars()
            .map(|c| match c {
                '\r' | '\n' | '\0' => ' ',
                '|' => '｜',
                other => other,
            })
            .collect()
    }
}

/// Truncate to at most `max_units` UTF-16 units, like `lstrcpynW(dst, src,
/// cap)` (which leaves `cap - 1` units). Never splits a surrogate pair.
#[must_use]
pub fn truncate_units(s: &str, max_units: usize) -> String {
    let mut used = 0usize;
    let mut out = String::with_capacity(s.len().min(max_units));
    for ch in s.chars() {
        let n = ch.len_utf16();
        if used + n > max_units {
            break;
        }
        used += n;
        out.push(ch);
    }
    out
}

/// Emulate `_wtoi`: skip leading whitespace, take an optional sign and digits,
/// stop at the first non-digit.
///
/// `str::parse` is wrong here: it rejects `"12abc"`, `_wtoi` returns 12. The
/// ini is hand-edited, so such half-valid values are common.
#[must_use]
pub fn atoi_like(s: &str) -> i32 {
    let t = s.trim_start();
    let bytes = t.as_bytes();
    let mut i = 0;
    let mut sign = 1i64;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        if bytes[i] == b'-' {
            sign = -1;
        }
        i += 1;
    }
    let start = i;
    let mut acc: i64 = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        acc = (acc * 10 + i64::from(bytes[i] - b'0')).min(i64::from(i32::MAX) + 1);
        i += 1;
    }
    if i == start {
        return 0;
    }
    (sign * acc).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atoi_matches_wtoi() {
        assert_eq!(atoi_like("42"), 42);
        assert_eq!(atoi_like("-7"), -7);
        assert_eq!(atoi_like("  12abc"), 12);
        assert_eq!(atoi_like("abc"), 0);
        assert_eq!(atoi_like(""), 0);
        assert_eq!(atoi_like("+3"), 3);
        assert_eq!(atoi_like("99999999999999999999"), i32::MAX);
    }

    #[test]
    fn parses_a_full_line() {
        let it = Item::parse_line(r"2|1|GitHub|https://github.com|C:\icons\gh.png|1|--profile=x")
            .expect("valid");
        assert_eq!(it.kind, ItemKind::Url);
        assert_eq!(it.app_mode, 1);
        assert_eq!(it.name, "GitHub");
        assert_eq!(it.path, "https://github.com");
        assert_eq!(it.icon_src, r"C:\icons\gh.png");
        assert_eq!(it.fit, 1);
        assert_eq!(it.args, "--profile=x");
    }

    #[test]
    fn rejects_short_or_unknown_lines() {
        assert!(Item::parse_line("0|0|name").is_none()); // only 3 fields
        assert!(Item::parse_line("9|0|name|path").is_none()); // unknown type
        assert!(Item::parse_line("-1|0|name|path").is_none());
        assert!(Item::parse_line("").is_none());
    }

    #[test]
    fn minimal_line_defaults_missing_fields() {
        let it = Item::parse_line("3|0|sep|").expect("4 fields is enough");
        assert_eq!(it.kind, ItemKind::Separator);
        assert_eq!(it.icon_src, "");
        assert_eq!(it.fit, 0);
        assert_eq!(it.args, "");
    }

    #[test]
    fn line_roundtrip_is_stable() {
        let it = Item::parse_line(r"0|0|记事本|C:\Windows\notepad.exe||0|").unwrap();
        assert_eq!(Item::parse_line(&it.to_line()).unwrap().name, "记事本");
        assert_eq!(it.to_line(), r"0|0|记事本|C:\Windows\notepad.exe||0|");
    }

    #[test]
    fn separators_in_fields_do_not_corrupt_the_line() {
        let mut it = Item::new(ItemKind::File, "a|b", r"C:\x");
        it.args = "line1\nline2".into();
        let line = it.to_line();
        assert_eq!(
            line.matches('|').count(),
            6,
            "字段分隔符必须恰好 6 个: {line}"
        );
        assert!(!line.contains('\n'));
        let back = Item::parse_line(&line).unwrap();
        assert_eq!(back.name, "a｜b");
        assert_eq!(back.args, "line1 line2");
    }

    #[test]
    fn truncation_counts_utf16_units() {
        // BMP chars take 1 unit, emoji take 2.
        assert_eq!(truncate_units("abcdef", 4), "abcd");
        assert_eq!(truncate_units("😀😀😀", 4), "😀😀");
        assert_eq!(truncate_units("😀😀😀", 5), "😀😀");
        assert_eq!(truncate_units("中文名称", 3), "中文名");
    }

    #[test]
    fn kids_are_capped() {
        let mut g = Item::new(ItemKind::Group, "组", "");
        for i in 0..MAX_KIDS {
            assert!(g.add_kid(Item::new(ItemKind::File, format!("k{i}"), "")));
        }
        assert!(!g.add_kid(Item::new(ItemKind::File, "overflow", "")));
        assert_eq!(g.kids.len(), MAX_KIDS);
    }

    #[test]
    fn temp_items_are_not_persisted() {
        let mut it = Item::new(ItemKind::File, "x", "");
        assert!(it.is_persistable());
        it.temp = true;
        assert!(!it.is_persistable());
    }
}
