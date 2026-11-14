//! Path resolution: portable mode, config directory, icon cache.
//! Order matters: the portable marker is checked before creating directories.
//! Defaults to `%APPDATA%\NookSill`; a marker next to the exe switches the root.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use windows::Win32::UI::Shell::SHGetFolderPathW;

use crate::win::{from_wide, to_wide};

/// `CSIDL_APPDATA` is `FOLDERID_RoamingAppData`. Hand-written to avoid pulling in
/// the whole CSIDL block, where every constant is a bare u32.
const CSIDL_APPDATA: i32 = 0x001A;

/// `SHGetFolderPathW` always writes a `MAX_PATH` buffer; only the exe path
/// (which can be longer) goes through `std::env::current_exe`.
const MAX_PATH: usize = 260;

#[derive(Debug)]
pub enum PathsError {
    ExePath,
    AppData,
    Io(io::Error),
}

impl fmt::Display for PathsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExePath => f.write_str(nooksill_core::i18n::t(
                "Cannot determine the executable path",
            )),
            Self::AppData => f.write_str(nooksill_core::i18n::t(
                "Cannot determine the %APPDATA% path",
            )),
            Self::Io(e) => f.write_str(&nooksill_core::i18n::fmt(
                "File operation failed: {0}",
                &[e.to_string().as_str()],
            )),
        }
    }
}

impl std::error::Error for PathsError {}

impl From<io::Error> for PathsError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Resolved runtime paths.
#[derive(Clone, Debug)]
pub struct Paths {
    /// Config root (`%APPDATA%\NookSill` or the exe directory).
    pub dir: PathBuf,
    /// Full path of `NookSill.ini`.
    pub ini: PathBuf,
    /// Icon cache directory.
    pub icon_dir: PathBuf,
    /// Path of the running exe.
    pub exe: PathBuf,
    /// Whether portable mode is active.
    pub portable: bool,
}

impl Paths {
    /// Resolve the paths and create the required directories.
    pub fn resolve() -> Result<Self, PathsError> {
        let exe = exe_path()?;
        let exe_dir = exe.parent().ok_or(PathsError::ExePath)?.to_path_buf();
        let appdata = appdata_path()?;

        let mut dir = appdata.join("NookSill");
        let mut ini = dir.join("NookSill.ini");
        let local_ini = exe_dir.join("NookSill.ini");
        let marker = exe_dir.join("NookSill.portable");

        let mut portable = false;
        if marker.exists() {
            portable = true;
            dir = exe_dir.clone();
            ini = local_ini.clone();
        } else if !ini.exists() && local_ini.exists() {
            // One-time migration of the ini and the icon cache.
            fs::create_dir_all(&dir)?;
            let _ = fs::copy(&local_ini, &ini);
            let src_icons = exe_dir.join("icons");
            let dst_icons = dir.join("icons");
            if src_icons.is_dir() {
                let _ = fs::create_dir_all(&dst_icons);
                if let Ok(entries) = fs::read_dir(&src_icons) {
                    for entry in entries.flatten() {
                        let from = entry.path();
                        if from.is_file() {
                            let to = dst_icons.join(entry.file_name());
                            let _ = fs::copy(from, to);
                        }
                    }
                }
            }
        }

        fs::create_dir_all(&dir)?;
        let icon_dir = dir.join("icons");
        fs::create_dir_all(&icon_dir)?;
        ensure_empty_ini(&ini)?;

        Ok(Self {
            dir,
            ini,
            icon_dir,
            exe,
            portable,
        })
    }

    /// Path of a cached icon file inside the icon directory.
    #[must_use]
    pub fn icon_for(&self, file_name: &str) -> PathBuf {
        self.icon_dir.join(file_name)
    }
}

/// Create an empty ini holding only a UTF-16 BOM when none exists.
///
/// A zero-byte file makes `WritePrivateProfileStringW` fall back to ANSI, which
/// turns non-ASCII names into question marks. The BOM is part of the contract.
fn ensure_empty_ini(path: &Path) -> io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    fs::write(path, [0xFFu8, 0xFE])
}

fn exe_path() -> Result<PathBuf, PathsError> {
    // `std` grows the buffer itself, so an install path past `MAX_PATH` still
    // resolves; the hand-rolled 260-unit buffer used to truncate it.
    std::env::current_exe().map_err(|_| PathsError::ExePath)
}

fn appdata_path() -> Result<PathBuf, PathsError> {
    let mut buf = [0u16; MAX_PATH];
    unsafe { SHGetFolderPathW(None, CSIDL_APPDATA, None, 0, &mut buf) }
        .map_err(|_| PathsError::AppData)?;
    let s = from_wide(&buf);
    if s.is_empty() {
        return Err(PathsError::AppData);
    }
    Ok(PathBuf::from(s))
}

/// Quote the exe path into a command line for the autostart entry.
#[must_use]
pub fn quoted_exe(exe: &Path) -> Vec<u16> {
    let s = format!("\"{}\"", exe.display());
    to_wide(&s)
}
