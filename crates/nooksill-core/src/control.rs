//! Control center tile model.
//!
//! Only "which tiles exist, in what order, how many per row" is modelled, so
//! it stays unit-testable. The system calls live in `platform::sysact` /
//! `platform::audio`, the drawing in `gpu::cc_win`.
//!
//! The default order is fixed, with the last two (energy / hotspot) appended
//! per config flag. After the user drags a tile, the order is stored in
//! `[widgets] Order` (see [`parse_order`] / [`order_text`]). The stored value
//! is a tile id, not a position: toggling one tile must not shuffle the rest.

/// One tile.
///
/// The discriminant is `u->id[k]`; the bit order must not change, the id
/// indexes the name and icon tables.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Tile {
    /// Wireless network.
    Wifi,
    /// Bluetooth.
    Bluetooth,
    /// Dark mode.
    DarkMode,
    /// Location.
    Location,
    /// Open Windows Settings.
    Settings,
    /// Mobile hotspot.
    Hotspot,
    /// Wired network (replaces [`Tile::Wifi`] when a wired NIC is present).
    Ethernet,
}

impl Tile {
    /// Text shown on the tile.
    ///
    /// Translated like any other UI text; the English string is the key.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Wifi => crate::i18n::t("Wi-Fi"),
            Self::Bluetooth => crate::i18n::t("Bluetooth"),
            Self::DarkMode => crate::i18n::t("Dark mode"),
            Self::Location => crate::i18n::t("Location"),
            Self::Settings => crate::i18n::t("Settings"),
            Self::Hotspot => crate::i18n::t("Hotspot"),
            Self::Ethernet => crate::i18n::t("Wired network"),
        }
    }

    /// Stable tile id.
    ///
    /// The numbers match the upstream `[Cc] Order` ini values so a config
    /// migrated from the C build needs no translation. 4 (mute) and 6
    /// (energy saver) are retired: mute moved onto the volume icon and the
    /// energy tile was removed, so those ids are skipped instead of renumbering
    /// every remaining tile (which would scramble saved orders).
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            Self::Wifi => 0,
            Self::Bluetooth => 1,
            Self::DarkMode => 2,
            Self::Location => 3,
            Self::Settings => 5,
            Self::Hotspot => 7,
            Self::Ethernet => 8,
        }
    }

    /// Look up a tile by id. Retired or unknown ids return `None`; a saved
    /// order keeps the rest of its sequence.
    #[must_use]
    pub const fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::Wifi),
            1 => Some(Self::Bluetooth),
            2 => Some(Self::DarkMode),
            3 => Some(Self::Location),
            5 => Some(Self::Settings),
            7 => Some(Self::Hotspot),
            8 => Some(Self::Ethernet),
            _ => None,
        }
    }

    /// Whether this is the network tile.
    ///
    /// Wired and wireless share one slot: the order stores the slot position,
    /// the active NIC decides which one is shown, so plugging in a cable does
    /// not move the tile.
    #[must_use]
    pub const fn is_net(self) -> bool {
        matches!(self, Self::Wifi | Self::Ethernet)
    }

    /// Whether the tile is currently on (decides the accent vs. grey plate).
    ///
    /// Only dark mode has on/off semantics now; mute lives on the volume icon
    /// and energy saver was removed. The rest jump to a system panel or a
    /// settings page and have no state.
    #[must_use]
    pub const fn is_switch(self) -> bool {
        matches!(self, Self::DarkMode)
    }
}

/// Tiles per row.
pub const COLS: usize = 3;

/// Build the default tile order.
///
/// The order is `[wired/wireless] bluetooth dark location settings
/// [hotspot]`. Hotspot is optional and disappears when `CcHotspot` is off;
/// mute and energy saver are no longer tiles.
#[must_use]
pub fn tiles(hotspot: bool, wired: bool) -> Vec<Tile> {
    let mut v = vec![
        if wired { Tile::Ethernet } else { Tile::Wifi },
        Tile::Bluetooth,
        Tile::DarkMode,
        Tile::Location,
        Tile::Settings,
    ];
    if hotspot {
        v.push(Tile::Hotspot);
    }
    v
}

/// Parse `[widgets] Order`: comma-separated ids.
///
/// Whitespace, unknown ids and duplicates are tolerated; users hand-edit this
/// key and one typo must not break the whole panel. An empty string yields an
/// empty list (meaning the default order).
#[must_use]
pub fn parse_order(text: &str) -> Vec<Tile> {
    let mut out: Vec<Tile> = Vec::new();
    for part in text.split(',') {
        let Ok(id) = part.trim().parse::<u8>() else {
            continue;
        };
        let Some(t) = Tile::from_id(id) else {
            continue;
        };
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// Write the order back to the ini. Inverse of [`parse_order`].
#[must_use]
pub fn order_text(tiles: &[Tile]) -> String {
    tiles
        .iter()
        .map(|t| t.id().to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Build the tile order from the saved order plus the current flags.
///
/// Three rules:
///
/// 1. A disabled tile (`CcHotspot`) is dropped but keeps its slot in `order`,
///    so it returns to the same place when re-enabled.
/// 2. The network slot shows wired or wireless per the active NIC, and
///    follows the position stored in `order`.
/// 3. Tiles missing from `order` are appended at the end, which covers both
///    newly added tiles and a hand-edited order that lists only some of them.
#[must_use]
pub fn tiles_ordered(order: &[Tile], hotspot: bool, wired: bool) -> Vec<Tile> {
    let net = if wired { Tile::Ethernet } else { Tile::Wifi };
    let enabled = |t: Tile| match t {
        Tile::Hotspot => hotspot,
        _ => true,
    };
    let mut out: Vec<Tile> = Vec::new();
    for t in order {
        let t = if t.is_net() { net } else { *t };
        if enabled(t) && !out.contains(&t) {
            out.push(t);
        }
    }
    // Append the tiles `order` never mentioned (including brand new ones).
    for t in tiles(hotspot, wired) {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// Number of tile rows.
///
/// Kept at 2 (`2 * (th + 10)`), so a third row would be covered by the
/// volume slider when both optional tiles are enabled.
#[must_use]
pub const fn rows(n: usize) -> usize {
    n.div_ceil(COLS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tiles_are_six_with_hotspot() {
        assert_eq!(
            tiles(true, false),
            [
                Tile::Wifi,
                Tile::Bluetooth,
                Tile::DarkMode,
                Tile::Location,
                Tile::Settings,
                Tile::Hotspot,
            ]
        );
    }

    #[test]
    fn hotspot_can_be_disabled() {
        let t = tiles(false, false);
        assert_eq!(t.len(), 5);
        assert!(!t.contains(&Tile::Hotspot));
    }

    #[test]
    fn wired_replaces_the_wifi_tile() {
        let t = tiles(false, true);
        assert_eq!(t[0], Tile::Ethernet);
        assert!(!t.contains(&Tile::Wifi));
    }

    #[test]
    fn rows_follow_the_tile_count() {
        assert_eq!(rows(5), 2);
        assert_eq!(rows(6), 2);
        assert_eq!(rows(7), 3);
    }

    #[test]
    fn only_dark_mode_is_a_switch() {
        let switches: Vec<Tile> = [
            Tile::Wifi,
            Tile::Bluetooth,
            Tile::DarkMode,
            Tile::Location,
            Tile::Settings,
            Tile::Hotspot,
            Tile::Ethernet,
        ]
        .into_iter()
        .filter(|t| t.is_switch())
        .collect();
        assert_eq!(switches, [Tile::DarkMode]);
    }

    #[test]
    fn every_tile_has_a_label() {
        for t in [
            Tile::Wifi,
            Tile::Bluetooth,
            Tile::DarkMode,
            Tile::Location,
            Tile::Settings,
            Tile::Hotspot,
            Tile::Ethernet,
        ] {
            assert!(!t.label().is_empty());
        }
    }

    #[test]
    fn tile_ids_are_unique_and_roundtrip() {
        let all = [
            Tile::Wifi,
            Tile::Bluetooth,
            Tile::DarkMode,
            Tile::Location,
            Tile::Settings,
            Tile::Hotspot,
            Tile::Ethernet,
        ];
        let mut seen = Vec::new();
        for t in all {
            assert_eq!(Tile::from_id(t.id()), Some(t), "{t:?} 的 id 对不上");
            assert!(!seen.contains(&t.id()), "id {} 重复", t.id());
            seen.push(t.id());
        }
        assert_eq!(Tile::from_id(9), None);
    }

    /// Retired ids are skipped, not reused: a saved order keeps the rest of
    /// its sequence and still loads.
    #[test]
    fn retired_ids_are_skipped_without_renumbering() {
        assert_eq!(Tile::from_id(4), None, "静音已移到音量图标");
        assert_eq!(Tile::from_id(6), None, "节能磁贴已移除");
        assert_eq!(Tile::Settings.id(), 5, "其余 id 保持不变");
        assert_eq!(Tile::Hotspot.id(), 7);
        assert_eq!(Tile::from_id(8), Some(Tile::Ethernet));
    }

    #[test]
    fn parse_order_skips_junk_and_retired_ids() {
        assert_eq!(
            parse_order(" 1, 9, x, 2,2 ,,5"),
            [Tile::Bluetooth, Tile::DarkMode, Tile::Settings],
            "未知 id / 非数字 / 重复项都跳过"
        );
        assert!(parse_order("4,6").is_empty());
        assert!(parse_order("").is_empty());
    }

    #[test]
    fn order_text_roundtrips() {
        let order = [Tile::Settings, Tile::Wifi, Tile::Hotspot];
        assert_eq!(order_text(&order), "5,0,7");
        assert_eq!(parse_order(&order_text(&order)), order);
    }

    /// An order written before the removal still loads: mute and energy vanish
    /// and the remaining tiles keep their relative positions.
    #[test]
    fn old_orders_keep_the_remaining_tiles_in_place() {
        let order = parse_order("5,0,4,1,2,3,6,7");
        assert_eq!(
            order,
            [
                Tile::Settings,
                Tile::Wifi,
                Tile::Bluetooth,
                Tile::DarkMode,
                Tile::Location,
                Tile::Hotspot,
            ]
        );
        assert_eq!(tiles_ordered(&order, true, false), order);
    }

    #[test]
    fn disabled_hotspot_keeps_its_slot() {
        let order = parse_order("0,7,1,2,3,5");
        let off = tiles_ordered(&order, false, false);
        assert!(!off.contains(&Tile::Hotspot));
        let on = tiles_ordered(&order, true, false);
        assert_eq!(on[1], Tile::Hotspot, "热点回到第 2 格");
    }

    #[test]
    fn net_slot_switches_in_place() {
        let order = parse_order("1,0,2,3,5");
        let wifi = tiles_ordered(&order, false, false);
        assert_eq!(wifi[1], Tile::Wifi);
        let wired = tiles_ordered(&order, false, true);
        assert_eq!(wired[1], Tile::Ethernet);
        assert_eq!(wired.len(), wifi.len(), "只是换了一个，不是多一个");
    }

    #[test]
    fn unlisted_tiles_are_appended() {
        let got = tiles_ordered(&[Tile::Settings], false, false);
        assert_eq!(got[0], Tile::Settings);
        assert_eq!(got.len(), 5, "其余四个（含网络）都要补上");
        assert_eq!(got[1], Tile::Wifi, "补上的按默认顺序");
    }

    #[test]
    fn empty_order_equals_default() {
        assert_eq!(tiles_ordered(&[], true, false), tiles(true, false));
    }
}
