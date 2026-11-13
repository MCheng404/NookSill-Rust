//! Staging and verification: only a hash-matched file reaches the final exe.
//!
//! The marker records the expected digest and size so a file left on disk by a
//! previous run cannot be swapped in after being tampered with.

use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::{cache, UpdateError};

const STAGED: &str = "NookSill.update.exe";
const MARKER: &str = "NookSill.update.marker";

/// Where a staged update lives: next to the running exe, so the final `move`
/// stays on one volume (and therefore is atomic-ish).
#[must_use]
pub fn staging_path() -> PathBuf {
    cache::sibling(STAGED)
}

fn marker_path() -> PathBuf {
    cache::sibling(MARKER)
}

/// A unique partial file per release: a re-cut asset with the same name cannot
/// be resumed onto the bytes of the previous build.
#[must_use]
pub fn part_path(digest: &str, tag: &str) -> PathBuf {
    let short: String = digest.chars().take(8).collect();
    let safe: String = tag
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(40)
        .collect();
    cache::sibling(&format!("NookSill-{safe}-{short}.part"))
}

/// Lowercase hex SHA-256 of `data`.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// First 64-hex-digit token of a `sha256sum` line.
#[must_use]
pub fn parse_sha256_file(text: &str) -> Option<String> {
    let token = text.split_whitespace().next()?;
    let ok = token.len() == 64 && token.chars().all(|c| c.is_ascii_hexdigit());
    ok.then(|| token.to_ascii_lowercase())
}

/// Hash a file without loading it into memory.
pub fn sha256_file(path: &Path) -> Result<String, UpdateError> {
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

pub fn write_marker(digest: &str, size: u64) -> Result<(), UpdateError> {
    std::fs::write(marker_path(), format!("{digest}\n{size}"))?;
    Ok(())
}

/// Recheck the staged bytes against the marker before any swap script runs.
pub fn verify_staged(staged: &Path) -> Result<(), UpdateError> {
    let raw = std::fs::read_to_string(marker_path())
        .map_err(|_| UpdateError::Http("更新标记缺失，拒绝安装".to_owned()))?;
    let (expected, size) = raw
        .split_once('\n')
        .ok_or_else(|| UpdateError::Http("更新标记损坏，拒绝安装".to_owned()))?;
    let size: u64 = size
        .trim()
        .parse()
        .map_err(|_| UpdateError::Http("更新标记损坏，拒绝安装".to_owned()))?;
    let len = std::fs::metadata(staged)?.len();
    if len != size {
        let _ = std::fs::remove_file(staged);
        return Err(UpdateError::Http("更新包大小不符，拒绝安装".to_owned()));
    }
    let actual = sha256_file(staged)?;
    if actual != expected.trim() {
        let _ = std::fs::remove_file(staged);
        return Err(UpdateError::Checksum {
            expected: expected.trim().to_owned(),
            actual,
        });
    }
    Ok(())
}

/// Rename the verified partial file onto the staging path, same volume.
pub fn finalize(part: &Path) -> Result<PathBuf, UpdateError> {
    let target = staging_path();
    std::fs::rename(part, &target)?;
    Ok(target)
}

/// Swap the staged exe in on next start.
///
/// A running exe cannot overwrite itself on Windows, so a detached `cmd`
/// waits for this process to exit, moves the file and (only for a manual update)
/// starts it again. The script deletes itself afterwards; if the move fails the
/// old exe is untouched.
pub fn swap_script(staged: &Path, restart: bool) -> Result<(), UpdateError> {
    verify_staged(staged)?;
    let current = std::env::current_exe()?;
    let script = std::env::temp_dir().join("nooksill-update.cmd");
    let body = script_body(&current, staged, restart);
    std::fs::write(&script, body)?;
    std::process::Command::new("cmd")
        .args(["/C", "start", "", "/min"])
        .arg(&script)
        .spawn()?;
    Ok(())
}

/// The batch body is a pure function so the silent-vs-restart distinction can
/// be asserted without spawning `cmd`.
pub(crate) fn script_body(current: &Path, staged: &Path, restart: bool) -> String {
    let exe_name = current
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("nooksill-dock.exe");
    let relaunch = if restart {
        format!("start \"\" \"{current}\"\r\n", current = current.display())
    } else {
        String::new()
    };
    format!(
        "@echo off\r\n\
         :wait\r\n\
         tasklist /FI \"IMAGENAME eq {exe_name}\" 2>nul | find /I \"{exe_name}\" >nul\r\n\
         if not errorlevel 1 (\r\n\
         \tping -n 2 127.0.0.1 >nul\r\n\
         \tgoto wait\r\n\
         )\r\n\
         move /Y \"{staged}\" \"{current}\" >nul\r\n\
         {relaunch}\
         del \"%~f0\"\r\n",
        staged = staged.display(),
        current = current.display(),
    )
}
