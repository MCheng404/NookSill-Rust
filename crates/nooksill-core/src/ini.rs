//! Read, write and preserve `NookSill.ini`.
//!
//! Not a generic ini file: it is UTF-16LE with a BOM, keys keep a stable
//! order, and unknown keys survive so an older build cannot wipe a newer
//! config. Implements the profile API subset it needs: sections, `k=v`, `;`
//! comments, case-insensitive lookup. No escaping; item values are
//! `|`-separated.

use std::fs;
use std::io;
use std::path::Path;

/// File encoding. Whatever was read is what gets written back.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Encoding {
    /// `FF FE` plus two-byte little-endian units. The default.
    #[default]
    Utf16Le,
    /// `FE FF` plus big-endian units.
    Utf16Be,
    /// UTF-8 without a BOM. Common for hand-edited files.
    Utf8,
}

/// One section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub entries: Vec<(String, String)>,
}

/// A whole ini file.
#[derive(Clone, Debug, Default)]
pub struct Ini {
    pub encoding: Encoding,
    pub sections: Vec<Section>,
}

impl Ini {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Decode raw bytes. The BOM picks the encoding; without one, UTF-8 is
    /// tried first and invalid input falls back to UTF-16LE, since Windows
    /// sometimes emits BOM-less wide files.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> (String, Encoding) {
        if bytes.starts_with(&[0xFF, 0xFE]) {
            let (pairs, _) = bytes[2..].as_chunks::<2>();
            let units: Vec<u16> = pairs.iter().map(|c| u16::from_le_bytes(*c)).collect();
            (String::from_utf16_lossy(&units), Encoding::Utf16Le)
        } else if bytes.starts_with(&[0xFE, 0xFF]) {
            let (pairs, _) = bytes[2..].as_chunks::<2>();
            let units: Vec<u16> = pairs.iter().map(|c| u16::from_be_bytes(*c)).collect();
            (String::from_utf16_lossy(&units), Encoding::Utf16Be)
        } else if bytes.is_empty() {
            (String::new(), Encoding::Utf8)
        } else {
            match std::str::from_utf8(bytes) {
                Ok(s) => (s.to_owned(), Encoding::Utf8),
                // Not valid UTF-8 and no BOM: retry as UTF-16LE.
                Err(_) => {
                    let (pairs, _) = bytes.as_chunks::<2>();
                    let units: Vec<u16> = pairs.iter().map(|c| u16::from_le_bytes(*c)).collect();
                    (String::from_utf16_lossy(&units), Encoding::Utf16Le)
                }
            }
        }
    }

    /// Encode to bytes, including the BOM.
    #[must_use]
    pub fn encode(&self, encoding: Encoding) -> Vec<u8> {
        let text = self.to_text();
        match encoding {
            Encoding::Utf8 => text.into_bytes(),
            Encoding::Utf16Le => {
                let mut out = vec![0xFF, 0xFE];
                for u in text.encode_utf16() {
                    out.extend_from_slice(&u.to_le_bytes());
                }
                out
            }
            Encoding::Utf16Be => {
                let mut out = vec![0xFE, 0xFF];
                for u in text.encode_utf16() {
                    out.extend_from_slice(&u.to_be_bytes());
                }
                out
            }
        }
    }

    /// Parse text.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut ini = Ini::new();
        let mut current: Option<usize> = None;

        for raw in text.split('\n') {
            let line = raw.trim_end_matches('\r').trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix('[') {
                if let Some(end) = rest.find(']') {
                    let name = rest[..end].trim().to_owned();
                    current = Some(ini.ensure_section(&name));
                    continue;
                }
            }
            if let Some(eq) = line.find('=') {
                let key = line[..eq].trim();
                if key.is_empty() {
                    continue;
                }
                let value = line[eq + 1..].trim().to_owned();
                // Keys outside any section are dropped, as the Windows
                // profile API does.
                if let Some(idx) = current {
                    ini.set_in_section(idx, key, value);
                }
            }
        }
        ini
    }

    /// Serialize to text with CRLF line endings, as the profile API does.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for (i, sec) in self.sections.iter().enumerate() {
            if i > 0 {
                out.push_str("\r\n");
            }
            out.push('[');
            out.push_str(&sec.name);
            out.push_str("]\r\n");
            for (k, v) in &sec.entries {
                out.push_str(k);
                out.push('=');
                out.push_str(v);
                out.push_str("\r\n");
            }
        }
        out
    }

    fn section_index(&self, name: &str) -> Option<usize> {
        self.sections
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name.trim()))
    }

    fn ensure_section(&mut self, name: &str) -> usize {
        if let Some(i) = self.section_index(name) {
            return i;
        }
        self.sections.push(Section {
            name: name.trim().to_owned(),
            entries: Vec::new(),
        });
        self.sections.len() - 1
    }

    fn set_in_section(&mut self, idx: usize, key: &str, value: String) {
        let sec = &mut self.sections[idx];
        if let Some(pos) = sec
            .entries
            .iter()
            .position(|(k, _)| k.eq_ignore_ascii_case(key))
        {
            sec.entries[pos].1 = value;
        } else {
            sec.entries.push((key.trim().to_owned(), value));
        }
    }

    /// Read a string. Missing section or key yields `None`; an empty value
    /// yields `Some("")`.
    #[must_use]
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        let idx = self.section_index(section)?;
        self.sections[idx]
            .entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    /// Read an integer. Like `GetPrivateProfileInt`: a missing key yields
    /// `None`, a non-numeric value yields `Some(0)`.
    #[must_use]
    pub fn get_i32(&self, section: &str, key: &str) -> Option<i32> {
        self.get(section, key)
            .map(|v| v.trim().parse::<i32>().unwrap_or(0))
    }

    /// Write a string. Appends the section or key when missing, keeping the
    /// first-seen order.
    pub fn set(&mut self, section: &str, key: &str, value: impl Into<String>) {
        let idx = self.ensure_section(section);
        self.set_in_section(idx, key, value.into());
    }

    pub fn set_i32(&mut self, section: &str, key: &str, value: i32) {
        self.set(section, key, value.to_string());
    }

    /// Remove one key. Does nothing when the section or key is missing.
    ///
    /// The profile API equivalent is `WritePrivateProfileStringW(sec, key,
    /// NULL, ...)`; without this, shrinking a todo list leaves `T1` / `T2`
    /// behind.
    pub fn remove_key(&mut self, section: &str, key: &str) {
        let Some(idx) = self.section_index(section) else {
            return;
        };
        self.sections[idx]
            .entries
            .retain(|(k, _)| !k.eq_ignore_ascii_case(key));
    }

    /// Remove a whole section (`WritePrivateProfileStringW(sec, NULL, NULL, ...)`).
    pub fn remove_section(&mut self, section: &str) {
        self.sections
            .retain(|s| !s.name.eq_ignore_ascii_case(section));
    }

    /// Count key/value pairs in a section, used by self-check fields like `Count`.
    #[must_use]
    pub fn len_of(&self, section: &str) -> usize {
        self.section_index(section)
            .map_or(0, |i| self.sections[i].entries.len())
    }

    /// Load a file. A missing file yields an empty ini, not an error; that is
    /// the first-run state.
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        match fs::read(path) {
            Ok(bytes) => {
                let (text, encoding) = Self::decode(&bytes);
                let mut ini = Self::parse(&text);
                ini.encoding = encoding;
                Ok(ini)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::new()),
            Err(e) => Err(e),
        }
    }

    /// Atomic write: write `<path>.tmp` first, then rename over the target.
    ///
    /// Per-key `WritePrivateProfileStringW` can leave a half-written file if
    /// the process dies mid-save.
    pub fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        let tmp = path.with_extension("ini.tmp");
        fs::write(&tmp, self.encode(self.encoding))?;
        // Rename overwrites an existing target on Windows
        // (MoveFileEx + REPLACE_EXISTING).
        match fs::rename(&tmp, path) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections_and_keys() {
        let ini = Ini::parse("[Settings]\r\nA=1\r\nB = two \r\n; comment\r\n[Items]\r\nI0=x\r\n");
        assert_eq!(ini.get("Settings", "A"), Some("1"));
        assert_eq!(ini.get("Settings", "B"), Some("two"));
        assert_eq!(ini.get("Items", "I0"), Some("x"));
        assert_eq!(ini.len_of("Settings"), 2);
    }

    #[test]
    fn lookup_is_case_insensitive() {
        let ini = Ini::parse("[settings]\niconsize=64\n");
        assert_eq!(ini.get("Settings", "IconSize"), Some("64"));
        assert_eq!(ini.get("SETTINGS", "ICONSIZE"), Some("64"));
    }

    #[test]
    fn values_may_contain_equals_and_pipes() {
        let ini = Ini::parse("[Items]\nI0=0|0|a=b|C:\\x| |0|\n");
        assert_eq!(ini.get("Items", "I0"), Some("0|0|a=b|C:\\x| |0|"));
    }

    #[test]
    fn set_keeps_first_position() {
        let mut ini = Ini::parse("[Settings]\nA=1\nB=2\n");
        ini.set("Settings", "a", "9");
        let text = ini.to_text();
        let a = text.find("A=9").unwrap();
        let b = text.find("B=2").unwrap();
        assert!(a < b, "覆盖已有键不应改变顺序:\n{text}");
    }

    #[test]
    fn unknown_sections_survive_roundtrip() {
        let src = "[Settings]\nOpacity=60\n[Future]\nFoo=bar\n";
        let mut ini = Ini::parse(src);
        ini.set("Settings", "Opacity", "70");
        let again = Ini::parse(&ini.to_text());
        assert_eq!(again.get("Future", "Foo"), Some("bar"));
        assert_eq!(again.get("Settings", "Opacity"), Some("70"));
    }

    #[test]
    fn utf16le_bom_roundtrip() {
        let mut ini = Ini::parse("[Settings]\nOpacity=60\n");
        ini.set("Settings", "Name", "中文名称");
        let bytes = ini.encode(Encoding::Utf16Le);
        assert_eq!(&bytes[..2], &[0xFF, 0xFE]);
        let (text, enc) = Ini::decode(&bytes);
        assert_eq!(enc, Encoding::Utf16Le);
        let back = Ini::parse(&text);
        assert_eq!(back.get("Settings", "Name"), Some("中文名称"));
    }

    #[test]
    fn utf8_without_bom_is_kept() {
        let bytes = b"[Settings]\nOpacity=60\n";
        let (text, enc) = Ini::decode(bytes);
        assert_eq!(enc, Encoding::Utf8);
        assert_eq!(Ini::parse(&text).get("Settings", "Opacity"), Some("60"));
    }

    #[test]
    fn get_i32_matches_profile_api_semantics() {
        let ini = Ini::parse("[S]\nA=42\nB=abc\n");
        assert_eq!(ini.get_i32("S", "A"), Some(42));
        assert_eq!(ini.get_i32("S", "B"), Some(0));
        assert_eq!(ini.get_i32("S", "Missing"), None);
    }

    #[test]
    fn file_roundtrip_is_atomic_and_encoding_stable() {
        let dir = std::env::temp_dir().join("nooksill_ini_test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("NookSill.ini");

        let mut ini = Ini::new();
        ini.set("Settings", "Opacity", "66");
        ini.save(&path).expect("save");

        let loaded = Ini::load(&path).expect("load");
        assert_eq!(loaded.encoding, Encoding::Utf16Le);
        assert_eq!(loaded.get("Settings", "Opacity"), Some("66"));
        // The temp file must be renamed away, never left behind.
        assert!(!path.with_extension("ini.tmp").exists());

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }

    #[test]
    fn missing_file_is_empty_not_error() {
        let ini = Ini::load("Z:/definitely/not/here/NookSill.ini").expect("missing -> empty");
        assert!(ini.sections.is_empty());
    }
}
