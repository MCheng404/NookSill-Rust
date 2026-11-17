//! Resource monitor item identity and persisted order.
//!
//! Pure data only: sampling lives in `nooksill-platform`, painting in
//! `nooksill-gpu`. The ini value is a comma-separated list of item codes; its
//! order is the panel order, and an absent item is hidden.

/// One monitorable resource.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ResKind {
    Cpu,
    Gpu,
    Memory,
    Disk,
    Network,
    Processes,
    Uptime,
}

impl ResKind {
    pub const ALL: [Self; 7] = [
        Self::Cpu,
        Self::Gpu,
        Self::Memory,
        Self::Disk,
        Self::Network,
        Self::Processes,
        Self::Uptime,
    ];

    /// Shipped order, also used when the ini value is empty or unreadable.
    pub const DEFAULT_ORDER: [Self; 7] = Self::ALL;

    /// Stable ini code; renaming one breaks existing configs.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
            Self::Memory => "mem",
            Self::Disk => "disk",
            Self::Network => "net",
            Self::Processes => "proc",
            Self::Uptime => "uptime",
        }
    }

    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.code() == code.trim())
    }

    /// English UI key; translated once at the call site via `i18n::t`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cpu => "Processor",
            Self::Gpu => "GPU",
            Self::Memory => "Memory",
            Self::Disk => "Disk (C:)",
            Self::Network => "Network",
            Self::Processes => "Processes",
            Self::Uptime => "Uptime",
        }
    }

    /// Whether the row draws a 0..100% bar rather than a text-only value.
    #[must_use]
    pub const fn has_bar(self) -> bool {
        matches!(self, Self::Cpu | Self::Gpu | Self::Memory | Self::Disk)
    }
}

/// Parse the ini value: unknown codes and duplicates are dropped.
///
/// An empty or fully invalid list returns the shipped order, so a panel is
/// never blank because of a hand-edited ini.
#[must_use]
pub fn parse_order(text: &str) -> Vec<ResKind> {
    let mut out = Vec::new();
    for part in text.split(',') {
        if let Some(kind) = ResKind::from_code(part) {
            if !out.contains(&kind) {
                out.push(kind);
            }
        }
    }
    if out.is_empty() {
        ResKind::DEFAULT_ORDER.to_vec()
    } else {
        out
    }
}

/// Serialize an order for the ini.
#[must_use]
pub fn order_text(order: &[ResKind]) -> String {
    order.iter().map(|k| k.code()).collect::<Vec<_>>().join(",")
}

/// Normalize a hand-edited value before saving it back.
#[must_use]
pub fn clean_order(text: &str) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    order_text(&parse_order(text))
}

/// Move `kind` one slot towards the top; returns false when already first.
pub fn move_up(order: &mut [ResKind], kind: ResKind) -> bool {
    let Some(i) = order.iter().position(|k| *k == kind) else {
        return false;
    };
    if i == 0 {
        return false;
    }
    order.swap(i, i - 1);
    true
}

/// Move `kind` one slot towards the bottom; returns false when already last.
pub fn move_down(order: &mut [ResKind], kind: ResKind) -> bool {
    let Some(i) = order.iter().position(|k| *k == kind) else {
        return false;
    };
    if i + 1 == order.len() {
        return false;
    }
    order.swap(i, i + 1);
    true
}

/// Add a hidden item at the end; false when it is already visible.
pub fn add(order: &mut Vec<ResKind>, kind: ResKind) -> bool {
    if order.contains(&kind) {
        return false;
    }
    order.push(kind);
    true
}

/// Remove an item but never the last one; false when nothing changed.
pub fn remove(order: &mut Vec<ResKind>, kind: ResKind) -> bool {
    if order.len() <= 1 || !order.contains(&kind) {
        return false;
    }
    order.retain(|k| *k != kind);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip_all_items() {
        for kind in ResKind::ALL {
            assert_eq!(ResKind::from_code(kind.code()), Some(kind));
            assert!(!kind.label().is_empty());
        }
        assert_eq!(ResKind::from_code("nope"), None);
    }

    #[test]
    fn empty_or_invalid_order_falls_back_to_the_default() {
        assert_eq!(parse_order(""), ResKind::DEFAULT_ORDER);
        assert_eq!(parse_order("???"), ResKind::DEFAULT_ORDER);
        assert_eq!(clean_order(" cpu , nope, mem,cpu"), "cpu,mem");
        assert_eq!(clean_order(""), "");
    }

    #[test]
    fn order_drops_duplicates_and_preserves_the_rest() {
        let got = parse_order("net,cpu,net,uptime,bogus,disk");
        assert_eq!(
            got,
            vec![
                ResKind::Network,
                ResKind::Cpu,
                ResKind::Uptime,
                ResKind::Disk
            ]
        );
        assert_eq!(order_text(&got), "net,cpu,uptime,disk");
    }

    #[test]
    fn moves_stop_at_the_ends() {
        let mut order = parse_order("cpu,mem,disk");
        assert!(!move_up(&mut order, ResKind::Cpu));
        assert!(move_up(&mut order, ResKind::Disk));
        assert_eq!(order, vec![ResKind::Cpu, ResKind::Disk, ResKind::Memory]);
        assert!(!move_down(&mut order, ResKind::Memory));
        assert!(move_down(&mut order, ResKind::Cpu));
        assert_eq!(order, vec![ResKind::Disk, ResKind::Cpu, ResKind::Memory]);
    }

    #[test]
    fn visibility_keeps_at_least_one_item() {
        let mut order = parse_order("cpu");
        assert!(!remove(&mut order, ResKind::Cpu));
        assert!(add(&mut order, ResKind::Network));
        assert!(!add(&mut order, ResKind::Network));
        assert!(remove(&mut order, ResKind::Cpu));
        assert_eq!(order, vec![ResKind::Network]);
    }
}
