//! Small on-disk caches next to the exe: the release ETag/body and the last
//! mirror that produced a verified file.
//!
//! GitHub's `Cache-Control: max-age=60` is far too short to rely on, so the
//! ETag is persisted here and sent back as `If-None-Match` on every check.

use std::path::PathBuf;

const RELEASE_CACHE: &str = "NookSill.releases.cache";
const MIRROR_CACHE: &str = "NookSill.mirror.cache";

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

pub(crate) fn write_release(etag: Option<&str>, body: &str) {
    let tag = etag.unwrap_or("-");
    let _ = std::fs::write(sibling(RELEASE_CACHE), format!("{tag}\n{body}"));
}

/// Empty string is GitHub direct, a non-empty value is a mirror prefix.
pub(crate) fn sticky_mirror() -> Option<String> {
    let text = std::fs::read_to_string(sibling(MIRROR_CACHE)).ok()?;
    Some(text.trim().to_owned())
}

pub(crate) fn remember_mirror(base: &str) {
    let _ = std::fs::write(sibling(MIRROR_CACHE), base);
}
