//! Menu construction: item menu and tray menu.

use super::*;

impl Menu {
    /// Builds the menu for the clicked item; `None` means the whole Dock.
    ///
    /// `running` decides whether "quit" is offered. `temp` marks an unpinned
    /// running app, which has no config slot, so the index-based commands
    /// are withheld. Returns `None` when no command applies: an empty menu
    /// window is worse than no menu.
    #[must_use]
    pub fn build(item: Option<&Item>, running: bool, temp: bool, opts: MenuOpts) -> Option<Self> {
        let mut entries = Vec::new();
        match item.map(|i| i.kind) {
            None => {
                // Only files and folders: a URL needs a text field, and the
                // menu has no way to type one.
                if opts.add_items {
                    entries.push(MenuEntry::new("添加文件…", MenuCmd::AddFile, false, false));
                    entries.push(MenuEntry::new(
                        "添加文件夹…",
                        MenuCmd::AddFolder,
                        false,
                        false,
                    ));
                    entries.push(MenuEntry::new(
                        "新建文件夹…",
                        MenuCmd::NewFolder,
                        false,
                        true,
                    ));
                    entries.push(MenuEntry::new(
                        "新建应用组…",
                        MenuCmd::NewGroup,
                        false,
                        false,
                    ));
                }
                // The caller supplies the state; the menu never touches the
                // registry.
                if let Some(on) = opts.autostart {
                    entries.push(MenuEntry::toggle("开机自启", MenuCmd::ToggleAutostart, on));
                }
                // A shortcut to the notes panel, independent of whether the
                // Dock shows a notes icon.
                if opts.notes {
                    entries.push(MenuEntry::new("速记", MenuCmd::Notes, false, false));
                }
                entries.push(MenuEntry::new("退出 Dock", MenuCmd::ExitDock, true, false));
            }
            Some(ItemKind::Separator) => {
                entries.push(MenuEntry::new(
                    "删除分割线",
                    MenuCmd::RemoveSep,
                    true,
                    false,
                ));
            }
            Some(ItemKind::Group) => {
                // A group behaves like a directory: open, rename, dissolve.
                entries.push(MenuEntry::new("打开", MenuCmd::OpenGroup, false, false));
                entries.push(MenuEntry::new("重命名…", MenuCmd::Rename, false, false));
                entries.push(MenuEntry::new(
                    "解散分组",
                    MenuCmd::DissolveGroup,
                    true,
                    true,
                ));
                if !temp {
                    entries.push(MenuEntry::new(
                        "在右侧添加分割线",
                        MenuCmd::AddSepAfter,
                        false,
                        false,
                    ));
                    entries.push(MenuEntry::new(
                        "从 Dock 移除",
                        MenuCmd::RemoveItem,
                        true,
                        false,
                    ));
                }
            }
            Some(kind) => {
                // A running file item gets show/minimise, everything else open.
                let label = if running && kind == ItemKind::File {
                    "显示 / 最小化"
                } else {
                    "打开"
                };
                entries.push(MenuEntry::new(label, MenuCmd::Open, false, false));
                // Several windows of one app: pick one here instead of hunting
                // through Alt+Tab. Titles are free text, so they are shortened
                // to keep the menu narrow.
                if opts.windows.len() > 1 {
                    for (k, window) in opts.windows.iter().take(8).enumerate() {
                        let title = window.title.trim();
                        let label = if title.is_empty() {
                            format!("#{}", k + 1)
                        } else {
                            title.chars().take(28).collect::<String>()
                        };
                        entries.push(MenuEntry::new(
                            &label,
                            MenuCmd::ActivateWindow(window.hwnd),
                            false,
                            false,
                        ));
                    }
                    if let Some(last) = entries.last_mut() {
                        last.sep_after = true;
                    }
                }
                if kind == ItemKind::Folder {
                    entries.push(MenuEntry::new(
                        "在资源管理器中打开",
                        MenuCmd::OpenInExplorer,
                        false,
                        false,
                    ));
                }
                if running && matches!(kind, ItemKind::File | ItemKind::Url) {
                    entries.push(MenuEntry::new("退出应用", MenuCmd::QuitApp, false, false));
                }
                // An unpinned running app gets a way to stay.
                if temp && matches!(kind, ItemKind::File | ItemKind::Url) {
                    entries.push(MenuEntry::new(
                        "固定到 Dock",
                        MenuCmd::PinItem,
                        false,
                        false,
                    ));
                }
                if !temp {
                    entries.push(MenuEntry::new("重命名…", MenuCmd::Rename, false, false));
                    entries.push(MenuEntry::new("复制一份", MenuCmd::Duplicate, false, false));
                    // Icon picking only helps kinds that have a shell icon;
                    // URL and notes fall back to a monogram.
                    if matches!(kind, ItemKind::File | ItemKind::Folder | ItemKind::Trash) {
                        entries.push(MenuEntry::new(
                            "更换图标…",
                            MenuCmd::ChangeIcon,
                            false,
                            false,
                        ));
                        entries.push(MenuEntry::new(
                            "恢复默认图标",
                            MenuCmd::ResetIcon,
                            false,
                            false,
                        ));
                    }
                    entries.push(MenuEntry::new(
                        "在右侧添加分割线",
                        MenuCmd::AddSepAfter,
                        false,
                        true,
                    ));
                    entries.push(MenuEntry::new(
                        "从 Dock 移除",
                        MenuCmd::RemoveItem,
                        true,
                        false,
                    ));
                }
            }
        }
        if entries.is_empty() {
            return None;
        }
        // Settings must be reachable from every menu: with many icons the
        // cursor almost always lands on an item, and there is no tray yet.
        // The separator keeps it away from the common entries.
        if let Some(last) = entries.last_mut() {
            last.sep_after = true;
        }
        // Launchpad comes before settings: one is daily use, the other is not.
        entries.push(MenuEntry::new("起动台", MenuCmd::Launchpad, false, false));
        entries.push(MenuEntry::new("Dock 设置", MenuCmd::Settings, false, false));
        Some(Self {
            entries,
            hover: None,
        })
    }

    /// Builds the tray icon menu.
    ///
    /// Win32 `TrackPopupMenu` needs a foreground window owned by the calling
    /// thread, but the Dock is `WS_EX_NOACTIVATE` and the menu would then
    /// never dismiss. The self-drawn modal window is reused instead.
    #[must_use]
    pub fn tray(hidden: bool) -> Self {
        let label = if hidden { "显示 Dock" } else { "隐藏 Dock" };
        Self {
            entries: vec![
                MenuEntry::new(label, MenuCmd::ToggleDock, false, false),
                MenuEntry::new("起动台", MenuCmd::Launchpad, false, false),
                MenuEntry::new("Dock 设置", MenuCmd::Settings, false, true),
                MenuEntry::new("退出 Dock", MenuCmd::ExitDock, true, false),
            ],
            hover: None,
        }
    }
}
