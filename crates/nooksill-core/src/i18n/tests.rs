//! Guards the translation contract: key registration, table order, fallback
//! and the language resolution used at startup.

use super::*;
use std::path::{Path, PathBuf};

fn table_keys<'a>(table: &'a [(&'a str, &'a str)]) -> Vec<&'a str> {
    table.iter().map(|(k, _)| *k).collect()
}

fn is_ascending(keys: &[&str]) -> bool {
    keys.windows(2).all(|w| w[0] < w[1])
}

fn source_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|n| n == "i18n" || n == "target")
            {
                continue;
            }
            source_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn call_keys(text: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b't' && bytes[i + 1] == b'(' {
            // `fmt(` ends in `t(` too; other identifiers ending in t (exit,
            // set) must not be mistaken for a lookup.
            let prev = if i == 0 { b' ' } else { bytes[i - 1] };
            let boundary =
                prev == b'm' || prev == b':' || !(prev.is_ascii_alphanumeric() || prev == b'_');
            // rustfmt can put the string literal on the next line.
            let mut q = i + 2;
            while q < bytes.len() && bytes[q].is_ascii_whitespace() {
                q += 1;
            }
            if boundary && q < bytes.len() && bytes[q] == b'"' {
                let start = q + 1;
                if let Some(end) = text[start..].find('"') {
                    keys.push(text[start..start + end].to_owned());
                    i = start + end + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    keys
}

#[test]
fn english_keys_are_sorted_and_unique() {
    assert!(is_ascending(en::TEXT), "en::TEXT must be sorted and unique");
    assert!(!en::TEXT.is_empty());
}

#[test]
fn translation_tables_are_sorted_and_only_use_known_keys() {
    for table in [zh_cn::TEXT, zh_tw::TEXT] {
        let keys = table_keys(table);
        assert!(
            is_ascending(&keys),
            "translation table must be sorted by key"
        );
        for key in keys {
            assert!(has_key(key), "unknown translation key: {key}");
        }
        for (_, value) in table {
            assert!(!value.is_empty(), "an empty translation hides the text");
        }
    }
    assert!(
        zh_cn::TEXT.len() >= en::TEXT.len() * 8 / 10,
        "Simplified Chinese must cover the visible UI"
    );
}

#[test]
fn every_call_site_key_is_registered() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core lives under crates/");
    let mut files = Vec::new();
    source_files(root, &mut files);
    assert!(files.len() > 100, "source scan found too few files");
    let mut found = 0;
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        // Strip comments but keep the file as one string: a call may wrap.
        let code: String = text
            .lines()
            .map(|line| line.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        for key in call_keys(&code) {
            assert!(
                has_key(&key),
                "{}: unregistered i18n key {key:?}",
                path.display()
            );
            found += 1;
        }
    }
    assert!(found > 400, "source scan found too few keys: {found}");
}

#[test]
fn language_resolution_follows_tag_and_lang_id() {
    assert_eq!(Language::from_tag(""), None);
    assert_eq!(Language::from_tag("auto"), None);
    assert_eq!(Language::from_tag("EN-us"), Some(Language::En));
    assert_eq!(Language::from_tag(" zh-CN "), Some(Language::ZhCn));
    assert_eq!(Language::from_tag("zh-Hant-TW"), Some(Language::ZhTw));
    assert_eq!(Language::from_tag("fr-FR"), None);
    // 0x0409 en-US, 0x0804 zh-CN, 0x0404 zh-TW, 0x0411 ja-JP.
    assert_eq!(Language::from_lang_id(0x0409), Language::En);
    assert_eq!(Language::from_lang_id(0x0804), Language::ZhCn);
    assert_eq!(Language::from_lang_id(0x0404), Language::ZhTw);
    assert_eq!(Language::from_lang_id(0x0411), Language::En);
    assert_eq!(Language::ALL.len(), 3);
    assert_eq!(Language::ZhCn.tag(), "zh-CN");
}

#[test]
fn switch_language_and_fall_back_to_english() {
    set_language(Language::En);
    assert_eq!(language(), Language::En);
    assert_eq!(t("Settings"), "Settings");

    set_language(Language::ZhCn);
    assert_eq!(t("Settings"), "设置");
    assert_eq!(t("NookSill Settings"), "NookSill 设置");

    set_language(Language::ZhTw);
    assert_eq!(t("Settings"), "設定");
    // Absent from zh_tw::TEXT, so the base language is used.
    assert_eq!(
        t("Prevent accidental dragging and removal"),
        "Prevent accidental dragging and removal"
    );
    assert_eq!(t("Not registered yet"), "Not registered yet");

    configure("auto", 0x0804);
    assert_eq!(language(), Language::ZhCn);
    configure("zh-TW", 0x0409);
    assert_eq!(language(), Language::ZhTw);
    configure("unknown", 0x0411);
    assert_eq!(language(), Language::En);

    set_language(Language::En);
}

#[test]
fn fmt_replaces_indexed_placeholders() {
    // Uses the template helper directly: the global language is shared with
    // the switching test and parallel tests must not observe each other.
    assert_eq!(fmt_template("{0} items", &["3"]), "3 items");
    assert_eq!(fmt_template("{1} of {0}", &["8", "3"]), "3 of 8");
    assert_eq!(fmt_template("no args", &[]), "no args");
    assert_eq!(fmt_template("{0} {2}", &["a", "b"]), "a {2}");
    assert_eq!(fmt_template("literal {x}", &["a"]), "literal {x}");
    assert_eq!(fmt_template("{0}{0}", &["a"]), "aa");
}

fn placeholders(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1].is_ascii_digit() && bytes[i + 2] == b'}' {
            out.push(bytes[i + 1]);
            i += 3;
        } else {
            i += 1;
        }
    }
    out.sort_unstable();
    out
}

#[test]
fn translations_keep_every_placeholder() {
    for table in [zh_cn::TEXT, zh_tw::TEXT] {
        for (key, value) in table {
            assert_eq!(
                placeholders(key),
                placeholders(value),
                "placeholder mismatch for {key:?}"
            );
        }
    }
}
