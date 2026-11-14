//! System file dialogs via `IFileOpenDialog`.
//!
//! Modal and blocking, so the UI thread calls it on user input. It stays on that
//! thread to keep its parent relationship, otherwise it sinks behind the window.

use std::path::PathBuf;

use windows::core::HSTRING;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, IShellItem, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM,
    FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
};

use crate::win;

/// One filter entry: `("Apps", "*.exe;*.lnk")`.
pub type Filter<'a> = (&'a str, &'a str);

/// Pick one file. `owner` is the parent window; `None` parents to the desktop.
#[must_use]
pub fn open_file(title: &str, owner: Option<isize>, filters: &[Filter<'_>]) -> Option<PathBuf> {
    show(owner, title, filters, Mode::Open)
}

/// Pick a folder.
#[must_use]
pub fn open_folder(title: &str, owner: Option<isize>) -> Option<PathBuf> {
    show(owner, title, &[], Mode::Folder)
}

/// Save as. `default_name` only pre-fills the name, the user may change it.
#[must_use]
pub fn save_file(title: &str, owner: Option<isize>, default_name: &str) -> Option<PathBuf> {
    let dialog = build(title, &[], Mode::Save)?;
    unsafe {
        let _ = dialog.SetFileName(&HSTRING::from(default_name));
    }
    finish(dialog, owner)
}

enum Mode {
    Open,
    Folder,
    Save,
}

fn build(title: &str, filters: &[Filter<'_>], mode: Mode) -> Option<IFileOpenDialog> {
    win::ensure_com();
    // SAFETY: `ensure_com` initialized this thread; the standard shell dialog
    // creation path only fails with Err.
    let dialog: IFileOpenDialog =
        unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()? };
    unsafe {
        let mut options = FOS_FORCEFILESYSTEM;
        match mode {
            Mode::Folder => options |= FOS_PICKFOLDERS | FOS_PATHMUSTEXIST,
            Mode::Open => options |= FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST,
            Mode::Save => options |= FOS_OVERWRITEPROMPT,
        }
        dialog.SetOptions(options).ok()?;
        dialog.SetTitle(&HSTRING::from(title)).ok()?;

        if !filters.is_empty() {
            // Both HSTRING vectors must outlive `SetFileTypes`; the specs only
            // borrow their pointers.
            let names: Vec<HSTRING> = filters.iter().map(|(n, _)| HSTRING::from(*n)).collect();
            let specs: Vec<HSTRING> = filters.iter().map(|(_, s)| HSTRING::from(*s)).collect();
            let specs_ref: Vec<COMDLG_FILTERSPEC> = names
                .iter()
                .zip(&specs)
                .map(|(n, s)| COMDLG_FILTERSPEC {
                    pszName: windows::core::PCWSTR(n.as_ptr()),
                    pszSpec: windows::core::PCWSTR(s.as_ptr()),
                })
                .collect();
            dialog.SetFileTypes(&specs_ref).ok()?;
        }
    }
    Some(dialog)
}

fn finish(dialog: IFileOpenDialog, owner: Option<isize>) -> Option<PathBuf> {
    // SAFETY: `owner` is a caller-provided HWND; `None` means no parent.
    unsafe {
        let parent = owner.map(|h| windows::Win32::Foundation::HWND(h as *mut _));
        match dialog.Show(parent) {
            Ok(()) => {}
            // Cancel is the most common return and not an error; keep it out of
            // the log.
            Err(e) if e.code() == windows::Win32::Foundation::ERROR_CANCELLED.to_hresult() => {
                return None;
            }
            Err(e) => {
                // Other failures also map to None; one log line makes a dead
                // click traceable.
                eprintln!("[dialog] 打开文件对话框失败：{e}");
                return None;
            }
        }
        let item: IShellItem = dialog.GetResult().ok()?;
        let raw = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        // `GetDisplayName` returns a COM-heap PWSTR that must be freed
        // unconditionally. Convert first: with `raw.to_string().ok()?` a failed
        // conversion returns early and leaks (NTFS allows unpaired surrogates).
        let text = raw.to_string();
        CoTaskMemFree(Some(raw.0.cast()));
        let text = text.ok()?;
        Some(PathBuf::from(text))
    }
}

/// Shared implementation of the three entry points; only the mode differs.
fn show(owner: Option<isize>, title: &str, filters: &[Filter<'_>], mode: Mode) -> Option<PathBuf> {
    let dialog = build(title, filters, mode)?;
    finish(dialog, owner)
}

/// Create a directory under `parent` and return its full path.
///
/// The name goes through [`check_dir_name`] so separators, reserved characters
/// and empty names are rejected: `..\..\Windows` must not escape `parent`.
pub fn create_dir(parent: &std::path::Path, name: &str) -> Result<PathBuf, String> {
    let name = check_dir_name(name)?;
    let path = parent.join(&name);
    std::fs::create_dir(&path).map_err(|e| {
        nooksill_core::i18n::fmt(
            "Could not create the folder: {0}",
            &[e.to_string().as_str()],
        )
    })?;
    Ok(path)
}

/// Validate a directory name and return the cleaned form.
///
/// `..` and separators are rejected so the name cannot escape the parent;
/// `< > : " / \ | ? *` and control characters are rejected; trailing dots and
/// spaces are trimmed because Win32 strips them silently on create.
pub fn check_dir_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(nooksill_core::i18n::t("The name cannot be empty").into());
    }
    if trimmed == "." || trimmed == ".." {
        return Err(nooksill_core::i18n::t("The name cannot be . or ..").into());
    }
    if trimmed.chars().any(|c| {
        c == '/'
            || c == '\\'
            || c == ':'
            || c == '*'
            || c == '?'
            || c == '"'
            || c == '<'
            || c == '>'
            || c == '|'
            || (c as u32) < 0x20
    }) {
        return Err(nooksill_core::i18n::t("The name contains invalid characters").into());
    }
    let cleaned = trimmed.trim_end_matches([' ', '.']).to_owned();
    if cleaned.is_empty() {
        return Err(nooksill_core::i18n::t("The name cannot be empty").into());
    }
    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    use super::check_dir_name;

    #[test]
    fn accepts_ordinary_names() {
        assert_eq!(check_dir_name("工具").unwrap(), "工具");
        assert_eq!(check_dir_name("  下载  ").unwrap(), "下载");
        assert_eq!(check_dir_name("a b").unwrap(), "a b");
    }

    #[test]
    fn rejects_path_traversal_and_separators() {
        assert!(check_dir_name("..").is_err());
        assert!(check_dir_name(".").is_err());
        assert!(check_dir_name("a/b").is_err());
        assert!(check_dir_name("a\\b").is_err());
        assert!(check_dir_name("C:x").is_err());
    }

    #[test]
    fn rejects_empty_and_reserved_characters() {
        assert!(check_dir_name("").is_err());
        assert!(check_dir_name("   ").is_err());
        assert!(check_dir_name("a<b").is_err());
        assert!(check_dir_name("a|b").is_err());
        assert!(check_dir_name("a\u{1}b").is_err());
    }

    #[test]
    fn trims_trailing_dots_and_spaces() {
        // Win32 strips trailing dots and spaces silently, so an untrimmed name
        // would not match the directory actually created.
        assert_eq!(check_dir_name("abc. ").unwrap(), "abc");
    }
}
