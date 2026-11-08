use super::*;

fn entry(name: &str) -> AppEntry {
    AppEntry {
        name: name.to_owned(),
        path: format!(r"C:\Start\{name}.lnk"),
    }
}

fn with(names: &[&str]) -> Launchpad {
    let mut lp = Launchpad::new();
    let scanned: Vec<AppEntry> = names.iter().map(|n| entry(n)).collect();
    lp.absorb(&scanned);
    lp
}

/// `serialize` then `parse` must be lossless, folders included.
///
/// This is the only thing between a user drag and the next launch.
#[test]
fn dat_roundtrip_keeps_order_and_folders() {
    let mut lp = with(&["A", "B", "C", "D"]);
    // B dropped on A merges into a folder.
    assert!(lp.drop_at(1, 0, true));
    lp.hidden.push(r"C:\Start\D.lnk".to_owned());

    let dat = lp.serialize();
    let back = Launchpad::parse(&dat);
    assert_eq!(back.serialize(), dat, "存读之后内容应当完全一致");
    assert_eq!(back.top(), lp.top());
    assert_eq!(back.folders().len(), 1);
    assert_eq!(back.folder_len(0), 2);
}

/// Reads the line shapes the original launcher writes.
#[test]
fn parses_upstream_file() {
    let dat = "T\tC:\\A.lnk\tA\nF\t开发\nf\tC:\\B.lnk\tB\nf\tC:\\C.lnk\tC\nE\nH\tC:\\D.lnk\n";
    let lp = Launchpad::parse(dat);
    assert_eq!(lp.len(), 2);
    assert_eq!(lp.name_of(lp.top()[0]), "A");
    assert_eq!(lp.name_of(lp.top()[1]), "开发");
    assert_eq!(lp.folder_len(0), 2);
    assert!(lp.is_hidden(r"C:\D.lnk"));
}

/// A rescan must neither duplicate a cell nor resurrect a removed one.
#[test]
fn absorb_skips_placed_and_hidden() {
    let mut lp = with(&["A", "B"]);
    lp.hidden.push(r"C:\Start\C.lnk".to_owned());
    lp.absorb(&[entry("A"), entry("C"), entry("D")]);
    // C is hidden and A is placed, so only D is new.
    assert_eq!(lp.len(), 3);
    assert_eq!(lp.name_of(lp.top()[2]), "D");
}

/// Being inside a folder counts as placed, so the scanner leaves it there.
#[test]
fn absorb_skips_apps_inside_folders() {
    let mut lp = with(&["A", "B"]);
    assert!(lp.drop_at(1, 0, true));
    lp.absorb(&[entry("A"), entry("B")]);
    assert_eq!(lp.len(), 1, "文件夹里的 A/B 不该再出现在顶层");
}

/// Dropping onto a folder appends to it, even when it already holds apps.
#[test]
fn drop_into_existing_folder() {
    let mut lp = with(&["A", "B", "C"]);
    assert!(lp.drop_at(1, 0, true)); // A+B become a folder in cell 0
    assert_eq!(lp.len(), 2);
    assert!(lp.drop_at(1, 0, true)); // C joins the folder
    assert_eq!(lp.len(), 1);
    assert_eq!(lp.folder_len(0), 3);
}

/// Without `on_icon` a drop only reorders, never merges.
#[test]
fn drop_reorders_around_the_target() {
    let mut lp = with(&["A", "B", "C"]);
    // Cell 2 onto cell 0.
    assert!(lp.drop_at(2, 0, false));
    let names: Vec<&str> = lp.top().iter().map(|t| lp.name_of(*t)).collect();
    assert_eq!(names, ["C", "A", "B"]);
}

/// A folder with no member disappears, one with a single member unfolds.
#[test]
fn folders_dissolve_when_they_shrink() {
    let mut lp = with(&["A", "B", "C"]);
    assert!(lp.drop_at(1, 0, true)); // folder(A,B) + C
    assert_eq!(lp.len(), 2);
    // Drop B: only A is left, so the cell turns back into a plain app.
    assert!(lp.take_from_folder(0, 1, TakeOut::Drop));
    assert_eq!(lp.len(), 2, "散开后 A 回到顶层");
    assert!(matches!(lp.top()[0], Top::App(_)));

    // An empty folder can only come from the file: any removal below two
    // members already unfolded it, so build one directly.
    let mut lp2 = Launchpad::parse("F\t空\nE\nT\tC:\\A.lnk\tA\n");
    assert_eq!(lp2.len(), 2);
    lp2.cleanup_folders();
    assert_eq!(lp2.len(), 1, "空文件夹应当整格消失");
    assert!(matches!(lp2.top()[0], Top::App(_)));
}

/// Restored apps go to the end, so no later cell has to shift.
#[test]
fn take_out_to_top_appends() {
    let mut lp = with(&["A", "B", "C"]);
    assert!(lp.drop_at(1, 0, true));
    assert!(lp.take_from_folder(0, 0, TakeOut::ToTop));
    let names: Vec<&str> = lp.top().iter().map(|t| lp.name_of(*t)).collect();
    assert_eq!(names.last().copied(), Some("A"));
}

/// Hiding has to survive the scanner, so it goes through the same list that
/// `absorb` consults.
#[test]
fn take_out_hide_feeds_the_hidden_list() {
    let mut lp = with(&["A", "B", "C"]);
    assert!(lp.drop_at(1, 0, true));
    assert!(lp.take_from_folder(0, 1, TakeOut::Hide));
    assert!(lp.is_hidden(r"C:\Start\B.lnk"));
    lp.absorb(&[entry("B")]);
    assert_eq!(lp.len(), 2, "隐藏之后不该再被扫描补回来");
}

/// Dissolving is not sorting: the stored member order is all that is left.
#[test]
fn dissolve_puts_members_back_in_order() {
    let mut lp = with(&["A", "B", "C"]);
    assert!(lp.drop_at(1, 0, true)); // folder(A,B) + C
    assert!(lp.dissolve(0));
    let names: Vec<&str> = lp.top().iter().map(|t| lp.name_of(*t)).collect();
    assert_eq!(names, ["A", "B", "C"]);
}

/// Case matters because names come from the filesystem.
#[test]
fn filter_is_case_insensitive_and_skips_hidden() {
    let mut lp = with(&["Visual Studio", "WeChat", "Word"]);
    lp.hidden.push(r"C:\Start\Word.lnk".to_owned());
    let hit = lp.filter("w");
    let names: Vec<&str> = hit.iter().map(|i| lp.apps()[*i].name.as_str()).collect();
    assert_eq!(names, ["WeChat"], "Word 被隐藏，Visual Studio 不含 w");
    assert_eq!(lp.filter("").len(), 0, "空查询不算搜索");
}

/// The last page holds the remainder; an empty grid still needs one page.
#[test]
fn page_count_rounds_up() {
    assert_eq!(Launchpad::pages(0, 28), 1);
    assert_eq!(Launchpad::pages(1, 28), 1);
    assert_eq!(Launchpad::pages(28, 28), 1);
    assert_eq!(Launchpad::pages(29, 28), 2);
    assert_eq!(Launchpad::pages(56, 28), 2);
}

/// Blank input is refused so a cell never loses its label.
#[test]
fn rename_works_for_both_kinds() {
    let mut lp = with(&["A", "B"]);
    assert!(lp.rename(0, "记事本"));
    assert_eq!(lp.name_of(lp.top()[0]), "记事本");
    assert!(!lp.rename(0, "   "));
    assert!(lp.drop_at(1, 0, true));
    assert!(lp.rename(0, "工具"));
    assert_eq!(lp.name_of(lp.top()[0]), "工具");
}

/// Adding by hand is how a hidden app is brought back.
#[test]
fn add_is_idempotent_and_unhides() {
    let mut lp = with(&["A"]);
    assert_eq!(lp.add("A", r"C:\Start\A.lnk"), None, "同一路径不该重复");
    lp.hidden.push(r"C:\New\X.exe".to_owned());
    assert!(lp.add("X", r"C:\New\X.exe").is_some());
    assert!(!lp.is_hidden(r"C:\New\X.exe"));
}

/// A self drop must not merge an app with itself.
#[test]
fn dropping_on_itself_is_a_noop() {
    let mut lp = with(&["A", "B"]);
    assert!(!lp.drop_at(1, 1, true));
    assert_eq!(lp.len(), 2);
}

fn app(name: &str, path: &str) -> App {
    App {
        name: name.to_owned(),
        path: path.to_owned(),
    }
}

/// The defaults line up with the classifier's three buckets.
#[test]
fn default_tags_are_office_system_tools() {
    let lp = Launchpad::new();
    assert_eq!(lp.tags(), ["办公", "系统", "工具"]);
}

/// Every Start Menu `.lnk` path contains `\Windows\`, so it must not count.
#[test]
fn classifier_maps_system_office_and_tools() {
    assert_eq!(
        Launchpad::classify(&app("设置", r"C:\Windows\ImmersiveControlPanel\x.exe")),
        2,
        "Windows 目录下的算系统"
    );
    assert_eq!(
        Launchpad::classify(&app("回收站", "shell:RecycleBinFolder")),
        2
    );
    assert_eq!(
        Launchpad::classify(&app("Microsoft Word", r"C:\Office\WINWORD.EXE")),
        1
    );
    assert_eq!(
        Launchpad::classify(&app("Notion", r"C:\Apps\Notion.exe")),
        1
    );
    assert_eq!(
        Launchpad::classify(&app("Visual Studio Code", r"C:\Apps\Code.exe")),
        3
    );
    // A Start Menu `.lnk` path itself contains `\Windows\`; it must stay a tool.
    assert_eq!(
        Launchpad::classify(&app(
            "123云盘",
            r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\123云盘.lnk"
        )),
        3
    );
}

/// Clearing an override returns to the heuristic, not to the first group.
#[test]
fn manual_category_overrides_classifier() {
    let mut lp = Launchpad::new();
    lp.absorb(&[entry("Notepad")]);
    assert_eq!(lp.category_of(0), 3, "记事本默认归工具");
    lp.set_category(0, Some(2));
    assert_eq!(lp.category_of(0), 2);
    lp.set_category(0, None);
    assert_eq!(lp.category_of(0), 3, "清除后应当回到自动分类");
}

/// Stored numbers outlive their group, so reads clamp instead of trusting them.
#[test]
fn category_is_clamped_to_tag_count() {
    let mut lp = Launchpad::new();
    lp.absorb(&[entry("X")]);
    lp.set_category(0, Some(99));
    assert_eq!(lp.category_of(0), lp.tags().len());
    lp.remove_tag(2);
    assert_eq!(lp.category_of(0), lp.tags().len(), "删组后要重新夹紧");
}

/// Category and query are ANDed, and hidden apps never show up.
#[test]
fn visible_in_combines_category_and_query() {
    let mut lp = Launchpad::new();
    lp.absorb(&[entry("Word"), entry("Excel"), entry("Code"), entry("Zoom")]);
    let office = lp.visible_in(1, "");
    assert_eq!(office.len(), 2, "Word / Excel 归办公");
    let hit = lp.visible_in(1, "ex");
    assert_eq!(hit.len(), 1);
    assert_eq!(lp.apps()[hit[0]].name, "Excel");
    assert_eq!(lp.visible_in(0, "").len(), 4, "cat=0 是全部");
    let first = lp.apps()[0].path.clone();
    lp.hidden.push(first);
    assert_eq!(lp.visible_in(0, "").len(), 3, "隐藏的不出现");
}

/// Scan order and grid order differ, and the filtered view must use the latter:
/// otherwise dragging a tile under a filter would visibly change nothing.
#[test]
fn filtered_view_follows_top_order() {
    let mut lp = Launchpad::new();
    lp.absorb(&[entry("Word"), entry("Excel"), entry("Code"), entry("Zoom")]);
    // The user dragged Excel to the front.
    lp.drop_at(1, 0, false);
    let names: Vec<&str> = lp
        .visible_in(0, "")
        .iter()
        .map(|ai| lp.apps()[*ai].name.as_str())
        .collect();
    assert_eq!(names, ["Excel", "Word", "Code", "Zoom"]);
    let office: Vec<&str> = lp
        .visible_in(1, "")
        .iter()
        .map(|ai| lp.apps()[*ai].name.as_str())
        .collect();
    assert_eq!(office, ["Excel", "Word"], "组内顺序也跟顶层走");
}

/// A merged folder keeps its members in the order the merge produced.
#[test]
fn filtered_view_orders_folder_members_at_the_folder_slot() {
    let mut lp = Launchpad::new();
    lp.absorb(&[entry("Word"), entry("Excel"), entry("Code")]);
    // Word onto Excel makes a folder in cell 0.
    assert!(lp.drop_at(0, 1, true));
    let names: Vec<&str> = lp
        .visible_in(0, "")
        .iter()
        .map(|ai| lp.apps()[*ai].name.as_str())
        .collect();
    assert_eq!(names, ["Excel", "Word", "Code"], "夹内顺序：目标在前");
}

/// Removing the source shifts the target, so both directions must still land
/// immediately before the destination app.
#[test]
fn move_app_before_reorders_top() {
    let mut lp = Launchpad::new();
    lp.absorb(&[entry("A"), entry("B"), entry("C"), entry("D")]);
    // D (3) in front of A (0).
    assert!(lp.move_app_before(3, 0));
    let names: Vec<&str> = lp.top().iter().map(|t| lp.name_of(*t)).collect();
    assert_eq!(names, ["D", "A", "B", "C"]);
    // Downwards: A (now 1) in front of C (3), landing between B and C.
    assert!(lp.move_app_before(0, 2));
    let names: Vec<&str> = lp.top().iter().map(|t| lp.name_of(*t)).collect();
    assert_eq!(names, ["D", "B", "A", "C"]);
    assert!(!lp.move_app_before(1, 1));
    assert!(!lp.move_app_before(0, 9), "越界返回 false");
}

/// A folder member has no top cell, so a top-level reorder would lose it.
#[test]
fn move_app_before_refuses_folder_members() {
    let mut lp = Launchpad::new();
    lp.absorb(&[entry("Word"), entry("Excel"), entry("Code")]);
    assert!(lp.drop_at(0, 1, true)); // Word / Excel become a folder
    let (w, c) = (lp.top.len(), lp.folders().len());
    assert!(!lp.move_app_before(0, 2), "Word 在文件夹里，挪不动");
    assert_eq!(lp.top.len(), w);
    assert_eq!(lp.folders().len(), c);
}

/// Sorting rewrites the persisted grid, and case must not split the list.
#[test]
fn sort_az_reorders_top_by_name() {
    let mut lp = Launchpad::new();
    lp.absorb(&[entry("Zoom"), entry("alpha"), entry("Beta")]);
    lp.sort_az();
    let names: Vec<&str> = lp.top().iter().map(|t| lp.name_of(*t)).collect();
    assert_eq!(names, ["alpha", "Beta", "Zoom"], "大小写不敏感");
}

/// Tags and manual categories share the file, so they must survive it together.
#[test]
fn tags_and_overrides_roundtrip() {
    let mut lp = Launchpad::new();
    lp.absorb(&[entry("Word"), entry("Code")]);
    assert!(lp.set_tag(2, "开发"));
    lp.add_tag("游戏");
    lp.set_category(1, Some(4));
    let again = Launchpad::parse(&lp.serialize());
    assert_eq!(again.tags(), ["办公", "系统", "开发", "游戏"]);
    assert_eq!(again.category_of(1), 4);
    assert_eq!(again.category_of(0), 1, "没指定的仍走自动分类");
}

/// A file whose groups happen to equal the defaults must not wipe them.
///
/// This nails the trap that a per-line comparison against `DEFAULT_TAGS` falls
/// into: after three groups the file looks untouched again, and a fourth `G`
/// clears the ones already read.
#[test]
fn default_tags_survive_a_roundtrip() {
    let lp = Launchpad::new();
    let again = Launchpad::parse(&lp.serialize());
    assert_eq!(again.tags(), ["办公", "系统", "工具"]);
}

/// Category numbers are positional, so removing one shifts every later group
/// and strands overrides that pointed at the removed one.
#[test]
fn remove_tag_shifts_and_clears_overrides() {
    let mut lp = Launchpad::new();
    lp.absorb(&[entry("A"), entry("B")]);
    lp.add_tag("游戏");
    lp.set_category(0, Some(2)); // A -> system
    lp.set_category(1, Some(3)); // B -> tools
    assert!(lp.remove_tag(1)); // removes system
    assert_eq!(lp.tags(), ["办公", "工具", "游戏"]);
    assert_eq!(lp.category_of(0), 3, "被删那组的指定退回自动（A 归工具）");
    assert_eq!(lp.category_of(1), 2, "原第 3 组前移成第 2 组");
}
