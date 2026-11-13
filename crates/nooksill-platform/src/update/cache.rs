//! Small on-disk caches next to the exe: the release ETag/body and the last
//! mirror that produced a verified file.
//!
//! GitHub's `Cache-Control: max-age=60` is far too short to rely on, so the
//! ETag is persisted here and sent back as `If-None-Match` on every check.

use std::path::PathBuf;

const RELEASE_CACHE: &str = "NookSill.releases.cache";
const RELEASE_TMP: &str = "NookSill.releases.cache.tmp";
const MIRROR_CACHE: &str = "NookSill.mirror.cache";
const RETRY_CACHE: &str = "NookSill.retry.cache";

/// A file beside the running exe, so all update files stay on one volume.
pub(crate) fn sibling(name: &str) -> PathBuf {
    let mut path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("nooksill-dock.exe"));
    path.set_file_name(name);
    path
}

/// `(etag, body)` from the last 200 response. An empty etag means no tag was
/// sent, in which case a conditional request cannot be made.
pub(crate) fn read_release() -> Option<(String, String)> {
    let text = std::fs::read_to_string(sibling(RELEASE_CACHE)).ok()?;
    let (tag, body) = text.split_once('\n')?;
    if body.trim().is_empty() {
        return None;
    }
    Some((tag.trim().to_owned(), body.to_owned()))
}

/// Write through a temporary file on the same volume: a process killed midway
/// must not leave half a JSON body for the next `304` to parse.
pub(crate) fn write_release(etag: Option<&str>, body: &str) {
    let tag = etag.unwrap_or("-");
    let tmp = sibling(RELEASE_TMP);
    if std::fs::write(&tmp, format!("{tag}\n{body}")).is_ok() {
        let _ = std::fs::rename(&tmp, sibling(RELEASE_CACHE));
    }
}

pub(crate) fn clear_release() {
    let _ = std::fs::remove_file(sibling(RELEASE_CACHE));
}

/// Empty string is GitHub direct, a non-empty value is a mirror prefix.
pub(crate) fn sticky_mirror() -> Option<String> {
    let text = std::fs::read_to_string(sibling(MIRROR_CACHE)).ok()?;
    Some(text.trim().to_owned())
}

pub(crate) fn remember_mirror(base: &str) {
    let _ = std::fs::write(sibling(MIRROR_CACHE), base);
}

/// UTC epoch seconds before which no metadata request should be sent.
pub(crate) fn retry_until() -> Option<u64> {
    std::fs::read_to_string(sibling(RETRY_CACHE))
        .ok()?
        .trim()
        .parse()
        .ok()
}

pub(crate) fn remember_retry(until: u64) {
    let _ = std::fs::write(sibling(RETRY_CACHE), until.to_string());
}

pub(crate) fn clear_retry() {
    let _ = std::fs::remove_file(sibling(RETRY_CACHE));
}
