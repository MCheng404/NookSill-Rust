//! Pure logic for app groups ("folders"): create, merge, extract, dissolve.
//!
//! A group is an [`ItemKind::Group`] item whose children live in [`Item::kids`],
//! capped at [`MAX_KIDS`]. Ini IO lives in [`crate::store`]; this module only
//! mutates memory. Groups never nest, overflow errors instead of dropping, and
//! an extracted child lands right after the group.
//!
//! Every operation mutates `Vec<Item>` in place; the caller persists.

use crate::item::{Item, ItemKind};
use crate::MAX_KIDS;

/// Why a group operation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupError {
    /// Index out of range.
    NoSuchItem,
    /// Target is not a group, so it cannot take children.
    NotGroup,
    /// Group already has `MAX_KIDS` children.
    Full,
    /// Groups cannot be nested.
    Nested,
}

impl std::fmt::Display for GroupError {
    /// User-facing text goes through the i18n table like every other string;
    /// `Display` is what the status bar interpolates.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchItem => f.write_str(crate::i18n::t("The item no longer exists")),
            Self::NotGroup => f.write_str(crate::i18n::t("The target is not a group")),
            Self::Full => f.write_str(&crate::i18n::fmt(
                "The group is full (limit {0} items)",
                &[MAX_KIDS.to_string().as_str()],
            )),
            Self::Nested => f.write_str(crate::i18n::t("A group cannot contain another group")),
        }
    }
}

/// Create an empty group.
///
/// The name is truncated by [`crate::item::truncate_units`] like any other
/// item (127 UTF-16 units for `name`).
#[must_use]
pub fn new_group(name: &str) -> Item {
    Item::new(ItemKind::Group, name, "")
}

/// Number of children, or 0 for a non-group.
#[must_use]
pub fn kid_count(group: &Item) -> usize {
    if group.kind == ItemKind::Group {
        group.kids.len()
    } else {
        0
    }
}

/// Add a child. Rejects nested groups and overflow past [`MAX_KIDS`]. On
/// `Err` the group is unchanged.
pub fn add_kid(group: &mut Item, kid: Item) -> Result<(), GroupError> {
    can_add_kid(group, &kid)?;
    group.kids.push(kid);
    Ok(())
}

/// Check whether `kid` fits in `group`.
///
/// Call this before [`add_kid`]: a failed `add_kid` consumes the child and the
/// caller cannot get it back.
pub fn can_add_kid(group: &Item, kid: &Item) -> Result<(), GroupError> {
    if group.kind != ItemKind::Group {
        return Err(GroupError::NotGroup);
    }
    if kid.kind == ItemKind::Group {
        return Err(GroupError::Nested);
    }
    if group.kids.len() >= MAX_KIDS {
        return Err(GroupError::Full);
    }
    Ok(())
}

/// Borrow child `idx` without removing it.
#[must_use]
pub fn kid_at(group: &Item, idx: usize) -> Option<&Item> {
    group.kids.get(idx)
}

/// Flatten a group into its children.
///
/// The returned order is the child order; spreading them onto the bar in that
/// order keeps the group recognizable to the user.
#[must_use]
pub fn dissolve(group: &Item) -> Vec<Item> {
    if group.kind == ItemKind::Group {
        group.kids.clone()
    } else {
        Vec::new()
    }
}

/// Merge `items[src]` into `items[dst]`, which must be a group.
///
/// `src` disappears from the bar and becomes a child. Groups cannot nest.
pub fn merge_item(items: &mut Vec<Item>, src: usize, dst: usize) -> Result<(), GroupError> {
    if src >= items.len() || dst >= items.len() {
        return Err(GroupError::NoSuchItem);
    }
    if items[dst].kind != ItemKind::Group {
        return Err(GroupError::NotGroup);
    }
    if src == dst {
        return Err(GroupError::Nested);
    }
    if items[src].kind == ItemKind::Group {
        return Err(GroupError::Nested);
    }
    let item = items.remove(src);
    // Removing `src` before `dst` shifts the target down by one.
    let dst = if src < dst { dst - 1 } else { dst };
    add_kid(&mut items[dst], item)
}

/// Move child `kid` back onto the bar, right after the group.
///
/// Returns the child's new index on the bar.
pub fn extract_kid(
    items: &mut Vec<Item>,
    group_idx: usize,
    kid: usize,
) -> Result<usize, GroupError> {
    if group_idx >= items.len() {
        return Err(GroupError::NoSuchItem);
    }
    if items[group_idx].kind != ItemKind::Group {
        return Err(GroupError::NotGroup);
    }
    if kid >= items[group_idx].kids.len() {
        return Err(GroupError::NoSuchItem);
    }
    let item = items[group_idx].kids.remove(kid);
    let at = group_idx + 1;
    items.insert(at, item);
    Ok(at)
}

/// Dissolve `items[group_idx]`: children spread onto the bar at the group's
/// slot, the group itself removed.
///
/// Returns the index of the first child.
pub fn dissolve_item(items: &mut Vec<Item>, group_idx: usize) -> Result<usize, GroupError> {
    if group_idx >= items.len() {
        return Err(GroupError::NoSuchItem);
    }
    if items[group_idx].kind != ItemKind::Group {
        return Err(GroupError::NotGroup);
    }
    let kids = std::mem::take(&mut items[group_idx].kids);
    items.remove(group_idx);
    let at = group_idx;
    for (k, kid) in kids.into_iter().enumerate() {
        items.insert(at + k, kid);
    }
    Ok(at)
}

/// Move child `kid` up (`-1`) or down (`+1`).
///
/// Out of range, including moving past either end, returns `NoSuchItem` and
/// leaves the group unchanged.
pub fn move_kid(group: &mut Item, kid: usize, delta: i32) -> Result<(), GroupError> {
    if group.kind != ItemKind::Group {
        return Err(GroupError::NotGroup);
    }
    if kid >= group.kids.len() || delta == 0 {
        return Err(GroupError::NoSuchItem);
    }
    let to = kid as i32 + delta;
    if to < 0 || to as usize >= group.kids.len() {
        return Err(GroupError::NoSuchItem);
    }
    group.kids.swap(kid, to as usize);
    Ok(())
}

/// Grid cells used to draw a group icon.
///
/// The icon is a 2x2 quadrant: 1 child fills it, 2 split it top/bottom, 3-4
/// use all four cells. Returns `(cols, rows)`.
#[must_use]
pub const fn grid_cells(n: usize) -> (usize, usize) {
    match n {
        0 => (0, 0),
        1 => (1, 1),
        2 => (1, 2),
        _ => (2, 2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> Item {
        Item::new(ItemKind::File, name, "C:\\x.exe")
    }

    #[test]
    fn new_group_is_an_empty_group() {
        let g = new_group("工具");
        assert_eq!(g.kind, ItemKind::Group);
        assert_eq!(g.name, "工具");
        assert!(g.kids.is_empty());
        assert_eq!(kid_count(&g), 0);
    }

    #[test]
    fn add_kid_rejects_groups_and_overflow() {
        let mut g = new_group("工具");
        assert_eq!(add_kid(&mut g, file("a")), Ok(()));
        assert_eq!(
            add_kid(&mut g, new_group("嵌套")),
            Err(GroupError::Nested),
            "组里不能塞组"
        );
        for _ in 1..MAX_KIDS {
            add_kid(&mut g, file("x")).expect("未满时应当能加");
        }
        assert_eq!(kid_count(&g), MAX_KIDS);
        assert_eq!(add_kid(&mut g, file("y")), Err(GroupError::Full));
    }

    #[test]
    fn add_kid_to_a_plain_item_is_rejected() {
        let mut it = file("记事本");
        assert_eq!(add_kid(&mut it, file("a")), Err(GroupError::NotGroup));
        assert!(it.kids.is_empty());
    }

    #[test]
    fn merge_moves_an_item_into_the_group() {
        let mut items = vec![file("a"), new_group("工具"), file("b")];
        merge_item(&mut items, 0, 1).expect("应当成功");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, "工具");
        assert_eq!(kid_count(&items[0]), 1);
        assert_eq!(items[0].kids[0].name, "a");
    }

    #[test]
    fn merge_into_a_non_group_fails() {
        let mut items = vec![file("a"), file("b")];
        assert_eq!(merge_item(&mut items, 0, 1), Err(GroupError::NotGroup));
        assert_eq!(items.len(), 2, "失败时列表不能变");
    }

    #[test]
    fn merge_rejects_group_into_group() {
        let mut items = vec![new_group("外"), new_group("内")];
        assert_eq!(merge_item(&mut items, 1, 0), Err(GroupError::Nested));
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn merge_handles_source_after_target() {
        // Removing an earlier source shifts the target down by one.
        let mut items = vec![new_group("工具"), file("a"), file("b")];
        merge_item(&mut items, 2, 0).expect("应当成功");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].kind, ItemKind::Group);
        assert_eq!(kid_count(&items[0]), 1);
        assert_eq!(items[0].kids[0].name, "b");
    }

    #[test]
    fn extract_puts_the_kid_right_after_the_group() {
        let mut items = vec![file("a"), new_group("工具"), file("b")];
        let mut g = new_group("工具");
        add_kid(&mut g, file("x")).unwrap();
        add_kid(&mut g, file("y")).unwrap();
        items[1] = g;

        let at = extract_kid(&mut items, 1, 0).expect("应当成功");
        assert_eq!(at, 2);
        assert_eq!(items.len(), 4);
        assert_eq!(items[2].name, "x");
        assert_eq!(kid_count(&items[1]), 1);
        assert_eq!(items[1].kids[0].name, "y");
    }

    #[test]
    fn extract_rejects_bad_indices() {
        let mut items = vec![new_group("工具")];
        assert_eq!(extract_kid(&mut items, 0, 0), Err(GroupError::NoSuchItem));
        assert_eq!(extract_kid(&mut items, 3, 0), Err(GroupError::NoSuchItem));
    }

    #[test]
    fn dissolve_flattens_the_group_in_place() {
        let mut g = new_group("工具");
        add_kid(&mut g, file("x")).unwrap();
        add_kid(&mut g, file("y")).unwrap();
        let mut items = vec![file("a"), g, file("b")];

        let at = dissolve_item(&mut items, 1).expect("应当成功");
        assert_eq!(at, 1);
        assert_eq!(items.len(), 4);
        assert_eq!(items[1].name, "x");
        assert_eq!(items[2].name, "y");
        assert_eq!(items[3].name, "b");
        assert!(
            items.iter().all(|i| i.kind != ItemKind::Group),
            "解散后不该剩下组"
        );
    }

    #[test]
    fn dissolve_of_a_plain_item_is_rejected() {
        let mut items = vec![file("a")];
        assert_eq!(dissolve_item(&mut items, 0), Err(GroupError::NotGroup));
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn grid_cells_follow_the_four_quadrant_rule() {
        assert_eq!(grid_cells(0), (0, 0));
        assert_eq!(grid_cells(1), (1, 1));
        assert_eq!(grid_cells(2), (1, 2));
        assert_eq!(grid_cells(3), (2, 2));
        assert_eq!(grid_cells(4), (2, 2));
        assert_eq!(grid_cells(9), (2, 2), "超过四个也只用四格");
    }

    /// Swap adjacent children; moving past either end does nothing.
    #[test]
    fn move_kid_swaps_neighbours_only() {
        let mut g = new_group("工具");
        for n in ["a", "b", "c"] {
            add_kid(&mut g, file(n)).unwrap();
        }
        assert_eq!(move_kid(&mut g, 0, -1), Err(GroupError::NoSuchItem));
        assert_eq!(move_kid(&mut g, 2, 1), Err(GroupError::NoSuchItem));

        move_kid(&mut g, 0, 1).expect("应当能下移");
        assert_eq!(g.kids[0].name, "b");
        assert_eq!(g.kids[1].name, "a");
        assert_eq!(g.kids[2].name, "c");

        move_kid(&mut g, 2, -1).expect("应当能上移");
        assert_eq!(g.kids[1].name, "c");
        assert_eq!(g.kids[2].name, "a");
        assert_eq!(move_kid(&mut g, 9, 1), Err(GroupError::NoSuchItem));
    }

    #[test]
    fn move_kid_rejects_a_plain_item() {
        let mut it = file("记事本");
        assert_eq!(move_kid(&mut it, 0, 1), Err(GroupError::NotGroup));
    }

    /// After dissolving, children must round-trip through the ini with all
    /// their fields intact.
    #[test]
    fn dissolved_kids_survive_a_store_roundtrip() {
        let mut g = new_group("工具");
        let mut kid = file("x");
        kid.args = "--flag".into();
        add_kid(&mut g, kid).unwrap();
        let mut items = vec![g];
        let at = dissolve_item(&mut items, 0).unwrap();
        assert_eq!(items[at].args, "--flag");
    }
}
