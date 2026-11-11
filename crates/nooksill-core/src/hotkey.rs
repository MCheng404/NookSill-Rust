//! Global hotkey parsing and formatting.
//!
//! This module is dependency-free: the Win32 layer converts `modifiers` and
//! `vk` straight into `RegisterHotKey` arguments.

/// Win32 `MOD_*` values, mirrored here so `core` needs no platform crate.
pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const MOD_WIN: u32 = 0x0008;

/// Virtual keys that are not a single ASCII letter or digit.
const NAMED_KEYS: &[(&str, u32)] = &[
    ("Space", 0x20),
    ("Enter", 0x0D),
    ("Esc", 0x1B),
    ("Tab", 0x09),
    ("Backspace", 0x08),
    ("Delete", 0x2E),
    ("Insert", 0x2D),
    ("Home", 0x24),
    ("End", 0x23),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("Up", 0x26),
    ("Down", 0x28),
    ("Left", 0x25),
    ("Right", 0x27),
    ("PrintScreen", 0x2C),
    ("Pause", 0x13),
    ("CapsLock", 0x14),
    ("NumLock", 0x90),
    ("ScrollLock", 0x91),
    ("OemPlus", 0xBB),
    ("OemMinus", 0xBD),
    ("OemComma", 0xBC),
    ("OemPeriod", 0xBE),
];

/// A global hotkey: modifier bits plus a Win32 virtual-key code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub modifiers: u32,
    pub vk: u32,
}

impl Hotkey {
    /// `Ctrl+Alt+D`, the combination the dock has always used.
    pub const DEFAULT: Self = Self {
        modifiers: MOD_CONTROL | MOD_ALT,
        vk: b'D' as u32,
    };

    /// Parse `Ctrl+Alt+D` style text. Requires at least one modifier and a
    /// known key; unknown or malformed input yields `None` instead of a
    /// half-valid hotkey that `RegisterHotKey` would silently refuse.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        if parts.len() < 2 || parts.iter().any(|p| p.is_empty()) {
            return None;
        }
        let mut modifiers = 0u32;
        for part in &parts[..parts.len() - 1] {
            let bit = match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => MOD_CONTROL,
                "alt" => MOD_ALT,
                "shift" => MOD_SHIFT,
                "win" | "super" | "meta" => MOD_WIN,
                _ => return None,
            };
            if modifiers & bit != 0 {
                return None;
            }
            modifiers |= bit;
        }
        let vk = parse_key(parts[parts.len() - 1])?;
        Some(Self { modifiers, vk })
    }

    /// Canonical text, with modifiers in a fixed order.
    #[must_use]
    pub fn display(self) -> String {
        let mut out = Vec::new();
        for (bit, name) in [
            (MOD_CONTROL, "Ctrl"),
            (MOD_ALT, "Alt"),
            (MOD_SHIFT, "Shift"),
            (MOD_WIN, "Win"),
        ] {
            if self.modifiers & bit != 0 {
                out.push(name);
            }
        }
        let key = key_name(self.vk);
        out.push(&key);
        out.join("+")
    }
}

fn parse_key(name: &str) -> Option<u32> {
    let upper = name.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    if bytes.len() == 1 && (bytes[0].is_ascii_uppercase() || bytes[0].is_ascii_digit()) {
        return Some(u32::from(bytes[0]));
    }
    if let Some(n) = upper.strip_prefix('F') {
        if let Ok(v) = n.parse::<u32>() {
            if (1..=24).contains(&v) {
                return Some(0x6F + v);
            }
        }
    }
    NAMED_KEYS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, vk)| *vk)
}

fn key_name(vk: u32) -> String {
    if (0x30..=0x39).contains(&vk) || (0x41..=0x5A).contains(&vk) {
        return char::from_u32(vk).map_or_else(|| format!("0x{vk:X}"), |c| c.to_string());
    }
    if (0x70..=0x87).contains(&vk) {
        return format!("F{}", vk - 0x6F);
    }
    for (name, code) in NAMED_KEYS {
        if *code == vk {
            return (*name).to_owned();
        }
    }
    format!("0x{vk:X}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrips() {
        assert_eq!(Hotkey::parse("Ctrl+Alt+D"), Some(Hotkey::DEFAULT));
        assert_eq!(Hotkey::DEFAULT.display(), "Ctrl+Alt+D");
    }

    #[test]
    fn parsing_is_case_insensitive_and_normalises_order() {
        let hk = Hotkey::parse(" shift + WIN + ctrl + f5 ").expect("valid");
        assert_eq!(hk.display(), "Ctrl+Shift+Win+F5");
        assert_eq!(hk.modifiers, MOD_CONTROL | MOD_SHIFT | MOD_WIN);
        assert_eq!(hk.vk, 0x74);
    }

    #[test]
    fn named_keys_and_digits_roundtrip() {
        for text in [
            "Ctrl+Space",
            "Alt+9",
            "Win+Q",
            "Ctrl+Alt+PageDown",
            "Ctrl+F12",
            "Alt+OemPlus",
        ] {
            let hk = Hotkey::parse(text).unwrap_or_else(|| panic!("should parse: {text}"));
            assert_eq!(Hotkey::parse(&hk.display()), Some(hk), "{text}");
        }
    }

    #[test]
    fn malformed_combinations_are_rejected() {
        for text in [
            "",
            "D",
            "Ctrl+",
            "+D",
            "Ctrl+Alt+",
            "Ctrl+Ctrl+D",
            "Ctrl+Unknown",
            "Ctrl+F25",
            "Ctrl+Alt+D+E",
        ] {
            assert_eq!(Hotkey::parse(text), None, "should reject: {text:?}");
        }
    }
}
