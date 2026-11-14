//! Launching dock items.
//!
//! Only starts things; the bounce animation and running-state scanning belong
//! to the caller.

use nooksill_core::item::Item;
use windows::core::PCWSTR;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Why launching failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchError {
    /// The kind is not implemented yet (group, note, folder fan).
    Unsupported(&'static str),
    /// Shell refused; `code` is the `ShellExecuteW` return value.
    Shell(i32),
    /// The target path is empty.
    EmptyTarget,
    /// The path does not exist or has the wrong type.
    NotFound(String),
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(what) => {
                f.write_str(&nooksill_core::i18n::fmt("Not implemented: {0}", &[what]))
            }
            Self::EmptyTarget => f.write_str(nooksill_core::i18n::t("The target path is empty")),
            Self::NotFound(p) => f.write_str(&nooksill_core::i18n::fmt(
                "The path does not exist: {0}",
                &[p.as_str()],
            )),
            Self::Shell(0) => f.write_str(nooksill_core::i18n::t("Out of memory")),
            Self::Shell(2) => f.write_str(nooksill_core::i18n::t("File not found")),
            Self::Shell(3) => f.write_str(nooksill_core::i18n::t("Path not found")),
            Self::Shell(5) => f.write_str(nooksill_core::i18n::t("Access denied")),
            Self::Shell(c) => f.write_str(&nooksill_core::i18n::fmt(
                "Shell error code {0}",
                &[c.to_string().as_str()],
            )),
        }
    }
}

/// NUL-terminated UTF-16, as `ShellExecuteW` requires.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Call `ShellExecuteW("open", ...)`.
///
/// A return value of 32 or below means failure, by Win32 convention.
fn shell_open(file: &str, params: Option<&str>, dir: Option<&str>) -> Result<(), LaunchError> {
    let file_w = wide(file);
    let params_w = params.map(wide);
    let dir_w = dir.map(wide);
    let op = wide("open");

    let ret = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(op.as_ptr()),
            PCWSTR(file_w.as_ptr()),
            params_w
                .as_ref()
                .map_or(PCWSTR::null(), |v| PCWSTR(v.as_ptr())),
            dir_w
                .as_ref()
                .map_or(PCWSTR::null(), |v| PCWSTR(v.as_ptr())),
            SW_SHOWNORMAL,
        )
    };
    // `HINSTANCE` holds an isize; 32 or below is an error code.
    let code = ret.0 as i32;
    if code <= 32 {
        return Err(LaunchError::Shell(code));
    }
    Ok(())
}

/// Web app: with `app_mode == 1`, prefer an Edge or Chrome `--app` window.
///
/// Edge comes before Chrome because it always exists on Windows; the default
/// browser, which opens a normal tab, is the last resort.
fn open_url(url: &str, app_mode: i32, browser: i32) -> Result<(), LaunchError> {
    let app = app_mode == 1;
    for exe in browser_order(browser) {
        // Firefox has no `--app` window mode, so it opens normally.
        let params = if app && *exe != "firefox.exe" {
            format!("--app={url}")
        } else {
            url.to_owned()
        };
        if shell_open(exe, Some(&params), None).is_ok() {
            return Ok(());
        }
        // The chosen browser is missing: try the next, then the system default.
        // A dead click is worse than opening in another browser.
    }
    shell_open(url, None, None)
}

/// `Browser` key to the executables tried in order.
///
/// Values: 0 auto, 1 Edge, 2 Chrome, 3 Firefox, 4 system default. Auto prefers
/// Edge because it always exists on Windows and supports `--app` best.
/// An empty list means use the system default browser.
const fn browser_order(browser: i32) -> &'static [&'static str] {
    match browser {
        1 => &["msedge.exe"],
        2 => &["chrome.exe"],
        3 => &["firefox.exe"],
        4 => &[],
        _ => &["msedge.exe", "chrome.exe"],
    }
}

/// Launch one item.
///
/// A file gets its own directory as working directory so legacy programs that
/// look for resources next to themselves still find them; a folder opens as is.
pub fn launch(item: &Item, browser: i32) -> Result<(), LaunchError> {
    let params = if item.args.is_empty() {
        None
    } else {
        Some(item.args.as_str())
    };

    match item.kind {
        nooksill_core::item::ItemKind::Separator => Ok(()),
        nooksill_core::item::ItemKind::Url => open_url(&item.path, item.app_mode, browser),
        nooksill_core::item::ItemKind::Trash => shell_open("shell:RecycleBinFolder", None, None),
        nooksill_core::item::ItemKind::File => {
            let dir = std::path::Path::new(&item.path)
                .parent()
                .map(|p| p.to_string_lossy().into_owned());
            shell_open(&item.path, params, dir.as_deref())
        }
        nooksill_core::item::ItemKind::Folder => shell_open(&item.path, params, None),
        // Groups and notes are opened as panels in `bin/dock.rs::step_click`
        // before reaching here; kept so a dispatch change errors loudly instead
        // of silently succeeding.
        nooksill_core::item::ItemKind::Group => {
            Err(LaunchError::Unsupported(nooksill_core::i18n::t("Groups")))
        }
        nooksill_core::item::ItemKind::Note => {
            Err(LaunchError::Unsupported(nooksill_core::i18n::t("Notes")))
        }
    }
}

/// Open a directory in Explorer (the context menu action).
///
/// Unlike [`launch`], this always uses Explorer and never forwards item args:
/// those target the app and could be parsed as switches by `explorer.exe`.
pub fn open_folder(path: &str) -> Result<(), LaunchError> {
    if path.is_empty() {
        return Err(LaunchError::EmptyTarget);
    }
    if !std::path::Path::new(path).is_dir() {
        // Do not pretend a missing directory opens: `ShellExecute` may report
        // success by handing it to a default handler, hiding the error.
        return Err(LaunchError::NotFound(path.to_owned()));
    }
    shell_open(path, None, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nooksill_core::item::{Item, ItemKind};

    fn item(kind: ItemKind) -> Item {
        Item::new(kind, "测试", "")
    }

    /// A separator is not a launch failure, so no error is reported.
    #[test]
    fn separator_is_a_noop() {
        assert_eq!(launch(&item(ItemKind::Separator), 0), Ok(()));
    }

    /// Groups and notes must report "unsupported" rather than silently succeed,
    /// otherwise a click does nothing and the log says nothing either.
    #[test]
    fn group_and_note_report_unsupported() {
        assert_eq!(
            launch(&item(ItemKind::Group), 0),
            Err(LaunchError::Unsupported(nooksill_core::i18n::t("Groups")))
        );
        assert_eq!(
            launch(&item(ItemKind::Note), 0),
            Err(LaunchError::Unsupported(nooksill_core::i18n::t("Notes")))
        );
    }

    /// Really start a process and verify the side effect.
    ///
    /// Ignored by default because it spawns a real process; run explicitly with
    /// `cargo test -p nooksill-platform --lib launch -- --ignored`.
    #[test]
    #[ignore = "会真的启动进程，需显式 --ignored"]
    fn launches_a_real_command_and_it_has_an_effect() {
        let marker = std::env::temp_dir().join("nooksill_launch_probe.txt");
        let _ = std::fs::remove_file(&marker);

        let mut it = Item::new(ItemKind::File, "探针", r"C:\Windows\System32\cmd.exe");
        // Redirect with `>` to prove the args were passed, not just that a
        // process started.
        it.args = format!("/c echo ok > {}", marker.display());
        launch(&it, 0).expect("启动失败");

        // ShellExecute is asynchronous, so poll until the file lands.
        let mut seen = false;
        for _ in 0..40 {
            if marker.exists() {
                seen = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(seen, "标记文件没生成，说明命令没真正执行");
        let text = std::fs::read_to_string(&marker).unwrap_or_default();
        assert!(text.contains("ok"), "标记文件内容不对: {text:?}");
        let _ = std::fs::remove_file(&marker);
    }

    /// Error codes must translate to readable text, at least the common ones.
    #[test]
    fn error_codes_have_messages() {
        assert_eq!(
            LaunchError::Shell(2).to_string(),
            nooksill_core::i18n::t("File not found")
        );
        assert_eq!(
            LaunchError::Shell(5).to_string(),
            nooksill_core::i18n::t("Access denied")
        );
        assert!(LaunchError::Shell(1234).to_string().contains("1234"));
    }
}

#[cfg(test)]
mod browser_tests {
    use super::browser_order;

    /// Value semantics: 0 auto, 1 Edge, 2 Chrome, 3 Firefox, 4 system default.
    #[test]
    fn browser_key_picks_the_expected_exe() {
        assert_eq!(browser_order(0), &["msedge.exe", "chrome.exe"]);
        assert_eq!(browser_order(1), &["msedge.exe"]);
        assert_eq!(browser_order(2), &["chrome.exe"]);
        assert_eq!(browser_order(3), &["firefox.exe"]);
        assert!(browser_order(4).is_empty(), "4 = 系统默认，不指定 exe");
        assert_eq!(
            browser_order(99),
            &["msedge.exe", "chrome.exe"],
            "越界按自动档"
        );
    }
}
