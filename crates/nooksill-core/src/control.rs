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
    /// Mute.
    Mute,
    /// Open Windows Settings.
    Settings,
    /// Energy saver.
    Energy,
    /// Mobile hotspot.
    Hotspot,
    /// Wired network (replaces [`Tile::Wifi`] when a wired NIC is present).
    Ethernet,
}

impl Tile {
    /// Text shown on the tile.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Wifi => "无线网络",
            Self::Bluetooth => "蓝牙",
            Self::DarkMode => "深色模式",
            Self::Location => "定位",
            Self::Mute => "静音",
            Self::Settings => "设置",
            Self::Energy => "节能模式",
            Self::Hotspot => "移动热点",
            Self::Ethernet => "有线网络",
        }
    }

    /// Stable tile id.
    ///
    /// The numbers match the upstream `[Cc] Order` ini values (0..=7, wired
    /// uses 8) so a config migrated from the C build needs no translation.
    /// They must not follow the enum order; these numbers are what lands in
    /// the ini.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            Self::Wifi => 0,
            Self::Bluetooth => 1,
            Self::DarkMode => 2,
            Self::Location => 3,
            Self::Mute => 4,
            Self::Settings => 5,
            Self::Energy => 6,
            Self::Hotspot => 7,
            Self::Ethernet => 8,
        }
    }

    /// Look up a tile by id. Unknown ids return `None` (written by an older
    /// build or hand-edited).
    #[must_use]
    pub const fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::Wifi),
            1 => Some(Self::Bluetooth),
            2 => Some(Self::DarkMode),
            3 => Some(Self::Location),
            4 => Some(Self::Mute),
            5 => Some(Self::Settings),
            6 => Some(Self::Energy),
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
    /// Only three tiles have on/off semantics: dark mode, mute, energy. The
    /// rest jump to a system panel or a settings page and have no state.
    #[must_use]
    pub const fn is_switch(self) -> bool {
        matches!(self, Self::DarkMode | Self::Mute | Self::Energy)
    }
}

/// Tiles per row.
pub const COLS: usize = 3;

/// Build the default tile order.
///
/// The order is `[wired/wireless] bluetooth dark location mute settings
/// [energy] [hotspot]`. The last two are optional and disappear when
/// `CcEnergy` / `CcHotspot` are off.
#[must_use]
pub fn tiles(energy: bool, hotspot: bool, wired: bool) -> Vec<Tile> {
    let mut v = vec![
        if wired { Tile::Ethernet } else { Tile::Wifi },
        Tile::Bluetooth,
        Tile::DarkMode,
        Tile::Location,
        Tile::Mute,
        Tile::Settings,
    ];
    if energy {
        v.push(Tile::Energy);
    }
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
/// 1. A disabled tile (`CcEnergy` / `CcHotspot`) is dropped but keeps its
///    slot in `order`, so it returns to the same place when re-enabled.
/// 2. The network slot shows wired or wireless per the active NIC, and
///    follows the position stored in `order`.
/// 3. Tiles missing from `order` are appended at the end, which covers both
///    newly added tiles and a hand-edited order that lists only some of them.
#[must_use]
pub fn tiles_ordered(order: &[Tile], energy: bool, hotspot: bool, wired: bool) -> Vec<Tile> {
    let net = if wired { Tile::Ethernet } else { Tile::Wifi };
    let enabled = |t: Tile| match t {
        Tile::Energy => energy,
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
    for t in tiles(energy, hotspot, wired) {
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

    /// The default config (energy + hotspot on) has 8 tiles.
    #[test]
    fn default_tiles_are_eight() {
        let t = tiles(true, true, false);
        assert_eq!(t.len(), 8);
        assert_eq!(t[0], Tile::Wifi);
        assert_eq!(t[5], Tile::Settings);
        assert_eq!(t[6], Tile::Energy);
        assert_eq!(t[7], Tile::Hotspot);
    }

    /// Turning off both optional tiles leaves 6.
    #[test]
    fn optional_tiles_can_be_disabled() {
        let t = tiles(false, false, false);
        assert_eq!(t.len(), 6);
        assert!(!t.contains(&Tile::Energy));
        assert!(!t.contains(&Tile::Hotspot));
    }

    /// A wired NIC replaces the first tile.
    #[test]
    fn wired_replaces_the_wifi_tile() {
        let t = tiles(false, false, true);
        assert_eq!(t[0], Tile::Ethernet);
        assert!(!t.contains(&Tile::Wifi));
    }

    /// The row count follows the actual tile count.
    #[test]
    fn rows_follow_the_tile_count() {
        assert_eq!(rows(6), 2);
        assert_eq!(rows(7), 3);
        assert_eq!(rows(8), 3);
        assert_eq!(rows(9), 3);
    }

    /// Only three tiles have on/off semantics.
    #[test]
    fn only_three_tiles_are_switches() {
        let switches: Vec<&str> = [
            Tile::Wifi,
            Tile::Bluetooth,
            Tile::DarkMode,
            Tile::Location,
            Tile::Mute,
            Tile::Settings,
            Tile::Energy,
            Tile::Hotspot,
            Tile::Ethernet,
        ]
        .into_iter()
        .filter(|t| t.is_switch())
        .map(Tile::label)
        .collect();
        assert_eq!(switches, ["深色模式", "静音", "节能模式"]);
    }

    /// Every tile has a label.
    #[test]
    fn every_tile_has_a_label() {
        for t in [
            Tile::Wifi,
            Tile::Bluetooth,
            Tile::DarkMode,
            Tile::Location,
            Tile::Mute,
            Tile::Settings,
            Tile::Energy,
            Tile::Hotspot,
            Tile::Ethernet,
        ] {
            assert!(!t.label().is_empty());
        }
    }

    /// The id is the number in the ini; it must be unique and decoupled from
    /// the enum order.
    #[test]
    fn tile_ids_are_unique_and_roundtrip() {
        let all = [
            Tile::Wifi,
            Tile::Bluetooth,
            Tile::DarkMode,
            Tile::Location,
            Tile::Mute,
            Tile::Settings,
            Tile::Energy,
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

    /// Whitespace, unknown ids and duplicates in a hand-edited order must not
    /// break the layout.
    #[test]
    fn parse_order_skips_junk() {
        let got = parse_order(" 1, 9, x, 2,2 ,,5");
        assert_eq!(
            got,
            [Tile::Bluetooth, Tile::DarkMode, Tile::Settings],
            "未知 id / 非数字 / 重复项都跳过"
        );
        assert!(parse_order("").is_empty());
        assert!(parse_order("   ").is_empty());
        assert!(parse_order("abc").is_empty());
    }

    /// Saving and reloading must be identical (the ini is hand-edited).
    #[test]
    fn order_text_roundtrips() {
        let order = [Tile::Settings, Tile::Wifi, Tile::Mute, Tile::Energy];
        assert_eq!(order_text(&order), "5,0,4,6");
        assert_eq!(parse_order(&order_text(&order)), order);
    }

    /// The saved order decides the tile positions.
    #[test]
    fn tiles_ordered_follows_saved_order() {
        let order = parse_order("5,0,4,1,2,3,6,7");
        let got = tiles_ordered(&order, true, true, false);
        assert_eq!(
            got,
            [
                Tile::Settings,
                Tile::Wifi,
                Tile::Mute,
                Tile::Bluetooth,
                Tile::DarkMode,
                Tile::Location,
                Tile::Energy,
                Tile::Hotspot,
            ]
        );
    }

    /// A disabled tile disappears but keeps its slot, so re-enabling returns
    /// it to the same place instead of the end.
    #[test]
    fn disabled_tile_keeps_its_slot() {
        let order = parse_order("0,6,1,2,3,4,5,7");
        let off = tiles_ordered(&order, false, false, false);
        assert!(!off.contains(&Tile::Energy));
        assert!(!off.contains(&Tile::Hotspot));
        let on = tiles_ordered(&order, true, true, false);
        assert_eq!(on[1], Tile::Energy, "节能回到第 2 格");
        assert_eq!(on[7], Tile::Hotspot);
    }

    /// Wired and wireless share one slot: swapping the NIC changes neither
    /// the position nor the count.
    #[test]
    fn net_slot_switches_in_place() {
        let order = parse_order("1,0,2,3,4,5");
        let wifi = tiles_ordered(&order, false, false, false);
        assert_eq!(wifi[1], Tile::Wifi);
        let wired = tiles_ordered(&order, false, false, true);
        assert_eq!(wired[1], Tile::Ethernet);
        assert!(!wired.contains(&Tile::Wifi));
        assert_eq!(wired.len(), wifi.len(), "只是换了一个，不是多一个");
    }

    /// Tiles missing from the order are appended: a new tile, or a hand-edited
    /// order that lists only some of them.
    #[test]
    fn unlisted_tiles_are_appended() {
        let got = tiles_ordered(&[Tile::Settings], false, false, false);
        assert_eq!(got[0], Tile::Settings);
        assert_eq!(got.len(), 6, "其余五个（含网络）都要补上");
        assert_eq!(got[1], Tile::Wifi, "补上的按默认顺序");
    }

    /// An empty order equals the default order (a user who never dragged).
    #[test]
    fn empty_order_equals_default() {
        assert_eq!(
            tiles_ordered(&[], true, true, false),
            tiles(true, true, false)
        );
    }
}
