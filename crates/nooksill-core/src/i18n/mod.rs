//! UI text lookup. English text is the key and the fallback language.
//!
//! Each language has its own module. A missing entry falls back to English,
//! so a partially translated language never shows a blank or a key string.

mod en;
#[cfg(test)]
mod tests;
mod zh_cn;
mod zh_tw;

use std::sync::atomic::{AtomicU8, Ordering};

/// UI languages that own a translation table. English is the base language.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    En,
    ZhCn,
    ZhTw,
}

impl Language {
    pub const ALL: [Self; 3] = [Self::En, Self::ZhCn, Self::ZhTw];

    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::ZhCn => "zh-CN",
            Self::ZhTw => "zh-TW",
        }
    }

    /// `None` means "auto" or an unsupported tag; the caller then asks the OS.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        let t = tag.trim().to_ascii_lowercase();
        if t == "en" || t.starts_with("en-") {
            Some(Self::En)
        } else if t == "zh" || t == "zh-cn" || t == "zh-sg" || t.starts_with("zh-hans") {
            Some(Self::ZhCn)
        } else if t == "zh-tw" || t == "zh-hk" || t == "zh-mo" || t.starts_with("zh-hant") {
            Some(Self::ZhTw)
        } else {
            None
        }
    }

    /// Win32 LANGID: low 10 bits are the primary language, high 6 bits the
    /// sublanguage. Unsupported languages land on the base language.
    #[must_use]
    pub const fn from_lang_id(id: u16) -> Self {
        const LANG_CHINESE: u16 = 0x04;
        const SUB_CHINESE_TRADITIONAL: u16 = 0x01;
        if id & 0x03ff == LANG_CHINESE {
            if (id >> 10) & 0x3f == SUB_CHINESE_TRADITIONAL {
                Self::ZhTw
            } else {
                Self::ZhCn
            }
        } else {
            Self::En
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::ZhCn => "简体中文",
            Self::ZhTw => "繁體中文",
        }
    }

    const fn table(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::En => &[],
            Self::ZhCn => zh_cn::TEXT,
            Self::ZhTw => zh_tw::TEXT,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(Language::En as u8);

/// Process-wide language; UI code reads it on every text lookup.
pub fn set_language(lang: Language) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

#[must_use]
pub fn language() -> Language {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Language::ZhCn,
        2 => Language::ZhTw,
        _ => Language::En,
    }
}

/// Resolve the configured tag against the OS language id. An empty, "auto" or
/// unknown tag follows the system, matching the default behaviour.
pub fn configure(tag: &str, system_lang_id: u16) {
    let lang = Language::from_tag(tag).unwrap_or_else(|| Language::from_lang_id(system_lang_id));
    set_language(lang);
}

fn lookup(table: &'static [(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    table
        .binary_search_by(|(k, _)| (*k).cmp(key))
        .ok()
        .map(|i| table[i].1)
}

/// Translate a text key. The English text itself is the key, so an untranslated
/// entry and an unregistered entry both degrade to readable English.
#[must_use]
pub fn t(key: &'static str) -> &'static str {
    lookup(language().table(), key).unwrap_or_else(|| en::lookup(key).unwrap_or(key))
}

/// Translate a template whose `{0}`..`{9}` placeholders are supplied by the
/// caller. Placeholders let translations choose their own word order.
#[must_use]
pub fn fmt(key: &'static str, args: &[&str]) -> String {
    fmt_template(t(key), args)
}

fn fmt_template(template: &str, args: &[&str]) -> String {
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let mut start = 0;
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1].is_ascii_digit() && bytes[i + 2] == b'}' {
            out.push_str(&template[start..i]);
            let slot = usize::from(bytes[i + 1] - b'0');
            if let Some(value) = args.get(slot) {
                out.push_str(value);
            } else {
                // Keep an unmatched placeholder visible instead of dropping it.
                out.push_str(&template[i..i + 3]);
            }
            i += 3;
            start = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&template[start..]);
    out
}

/// Whether a key is registered in the English base table.
#[must_use]
pub fn has_key(key: &str) -> bool {
    en::lookup(key).is_some()
}
