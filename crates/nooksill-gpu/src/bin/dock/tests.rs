//! Behaviour tests for the dock binary: bar reordering and drop handling.

use super::drag::{reorder_bar, BarMove};
use super::sys::item_from_drop;
use super::*;

/// Name comes from the file stem; a folder must not be mistaken for a file.
#[test]
fn drop_builds_a_named_item_from_a_path() {
    let item = item_from_drop(r"C:\Apps\MyTool.exe").expect("应当能建条目");
    assert_eq!(item.name, "MyTool");
    assert_eq!(item.kind, ItemKind::File);
    assert_eq!(item.path, r"C:\Apps\MyTool.exe");
    assert!(item_from_drop("").is_none());
}

/// A real directory becomes `Folder`, not `File` just because it is missing.
#[test]
fn drop_recognises_a_real_directory() {
    let dir = std::env::temp_dir().join("nooksill_drop_dir_test");
    let _ = std::fs::create_dir_all(&dir);
    let s = dir.to_string_lossy().into_owned();
    let item = item_from_drop(&s).expect("应当能建条目");
    assert_eq!(item.kind, ItemKind::Folder);
    assert_eq!(item.path, s);
    let _ = std::fs::remove_dir(&dir);
}

/// A `.url` becomes a URL item and keeps the shell icon source.
#[test]
fn drop_converts_a_url_shortcut() {
    let dir = std::env::temp_dir().join("nooksill_drop_url_test");
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("Example.url");
    let content = "[InternetShortcut]\nURL=https://example.com/a\nIconIndex=0\n";
    std::fs::write(&file, content).expect("写临时 .url");
    let s = file.to_string_lossy().into_owned();
    let item = item_from_drop(&s).expect("应当能建条目");
    assert_eq!(item.kind, ItemKind::Url);
    assert_eq!(item.name, "Example");
    assert_eq!(item.path, "https://example.com/a");
    assert_eq!(item.icon_src, s);
    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_dir(&dir);
}

fn items(names: &[&str]) -> Vec<Item> {
    names
        .iter()
        .map(|n| Item::new(ItemKind::File, *n, format!("C:/{n}.exe")))
        .collect()
}

/// Swaps inside the pinned segment edit `raw`.
#[test]
fn reorder_inside_the_pinned_segment_edits_raw() {
    let (mut raw, mut temps) = (items(&["a", "b", "c"]), items(&["r1", "r2"]));
    let mut keys: Vec<String> = vec!["r1".into(), "r2".into()];
    assert_eq!(
        reorder_bar(&mut raw, &mut temps, &mut keys, 0, 2),
        BarMove::Pinned
    );
    let names: Vec<&str> = raw.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["b", "c", "a"]);
    assert_eq!(temps.len(), 2, "临时条目不该被动到");
    assert_eq!(keys, ["r1", "r2"]);
}

/// Running-to-running swaps must write `temp_keys` back too.
///
/// Editing `temps` alone lets the next scan (~1.2 s) rebuild the order from
/// `temp_keys`, so the icon springs back on release.
#[test]
fn reorder_between_running_items_keeps_temp_keys_in_step() {
    let (mut raw, mut temps) = (items(&["a"]), items(&["r1", "r2", "r3"]));
    let mut keys: Vec<String> = vec!["r1".into(), "r2".into(), "r3".into()];
    // Bar indices: 0 = a, 1..=3 = r1..r3.
    assert_eq!(
        reorder_bar(&mut raw, &mut temps, &mut keys, 1, 3),
        BarMove::Running
    );
    let names: Vec<&str> = temps.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["r2", "r3", "r1"]);
    assert_eq!(keys, ["r2", "r3", "r1"], "顺序必须与 temps 一致");
    assert_eq!(raw.len(), 1);
}

/// Dragging a running app into the pinned range pins it in place.
///
/// `raw` grows but "pinned first, running after" still holds, so the index
/// mapping used by clicks, menus and icons stays valid.
#[test]
fn dropping_a_running_item_into_the_pinned_zone_pins_it() {
    let (mut raw, mut temps) = (items(&["a", "b"]), items(&["r1", "r2"]));
    let mut keys: Vec<String> = vec!["r1".into(), "r2".into()];
    // Bar indices: 0 = a, 1 = b, 2 = r1, 3 = r2. Drag r1 between a and b.
    assert_eq!(
        reorder_bar(&mut raw, &mut temps, &mut keys, 2, 1),
        BarMove::PinnedNow
    );
    let names: Vec<&str> = raw.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["a", "r1", "b"], "落点就是固定位置");
    assert!(!raw[1].temp, "固定之后它就是配置的一部分");
    let left: Vec<&str> = temps.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(left, ["r2"]);
    assert_eq!(keys, ["r2"], "temp_keys 要跟着少一项");
}

/// A pinned item cannot slide into the running segment.
#[test]
fn a_pinned_item_cannot_slide_into_the_running_segment() {
    let (mut raw, mut temps) = (items(&["a", "b"]), items(&["r1"]));
    let mut keys: Vec<String> = vec!["r1".into()];
    assert_eq!(
        reorder_bar(&mut raw, &mut temps, &mut keys, 0, 2),
        BarMove::None
    );
    assert_eq!(
        reorder_bar(&mut raw, &mut temps, &mut keys, 0, 0),
        BarMove::None
    );
    // An out-of-range index must not panic.
    assert_eq!(
        reorder_bar(&mut raw, &mut temps, &mut keys, 1, 9),
        BarMove::None
    );
    let names: Vec<&str> = raw.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["a", "b"], "被拒绝的换位不能改任何顺序");
    assert_eq!(temps.len(), 1);
}
