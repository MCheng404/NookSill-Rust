//! In-app update: release channels, conditional API requests, verified
//! staging. A running exe cannot replace itself on Windows, so the last step
//! is a small script that moves the staged file after this process exits.

mod cache;
mod download;
mod http;
mod mirror;
mod release;
mod stage;
mod version;

#[cfg(test)]
mod tests;

use std::fmt;
use std::path::{Path, PathBuf};

pub use release::{Asset, Channel, Release};
pub use version::Version;

use crate::net;

/// Repository the updater talks to.
pub const REPO: &str = "MCheng404/NookSill-Rust";

/// Reverse proxies, fastest-first by convention; the real order comes from
/// timing. Keep them all with a trailing slash because [`mirror_url`] relies on it.
pub const MIRRORS: &[&str] = &[
    "https://gh-proxy.com/",
    "https://ghfast.top/",
    "https://ghproxy.net/",
    "https://gh.llkk.cc/",
    "https://mirror.houlang.cloud/",
];

/// A mirror answering faster than this is used without testing the rest.
pub const FAST_ENOUGH_MS: u128 = 600;

pub(crate) const TIMEOUT_MS: i32 = 12_000;
pub(crate) const PROBE_TIMEOUT_MS: i32 = 4_000;
pub(crate) const UA: &str = "NookSill-Updater";

#[derive(Debug)]
pub enum UpdateError {
    Net(net::NetError),
    Json,
    Http(String),
    /// The release has no `.exe` asset; packaging changed under us.
    NoAsset,
    /// No release has been published for the selected channel.
    NoRelease,
    Io(std::io::Error),
    /// Downloaded bytes do not match the published checksum.
    Checksum {
        expected: String,
        actual: String,
    },
    /// No mirror produced a verified file.
    AllMirrorsFailed(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // The anonymous API allows 60 requests per hour; say so plainly so a
            // 403 is not read as "the repository is gone".
            Self::Net(net::NetError::Status(code @ (403 | 429))) => {
                write!(
                    f,
                    "GitHub 接口限流（每小时 60 次，当前 HTTP {code}），请稍后再试"
                )
            }
            Self::Net(e) => write!(f, "网络错误：{e}"),
            Self::Json => write!(f, "GitHub 返回的内容无法解析"),
            Self::Http(what) => write!(f, "{what}"),
            Self::NoAsset => write!(f, "该版本没有可用的 exe 资产"),
            Self::NoRelease => write!(f, "该通道还没有发布任何版本"),
            Self::Io(e) => write!(f, "文件读写失败：{e}"),
            Self::Checksum { expected, actual } => {
                write!(f, "校验失败：期望 {expected}，实际 {actual}")
            }
            Self::AllMirrorsFailed(why) => {
                write!(f, "所有下载源都失败：{why}")
            }
        }
    }
}

impl std::error::Error for UpdateError {}

impl From<net::NetError> for UpdateError {
    fn from(e: net::NetError) -> Self {
        Self::Net(e)
    }
}

impl From<std::io::Error> for UpdateError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Ask GitHub for the newest release on `channel`.
///
/// One request to `/releases?per_page=15` serves both channels; `draft` and
/// `prerelease` are filtered locally. `per_page` is fixed because a different
/// page size produces a different ETag.
pub fn latest_release(channel: Channel) -> Result<Release, UpdateError> {
    http::latest(channel)
}

/// Whether this release should be offered to the current install.
#[must_use]
pub fn is_offered(release: &Release, installed: &Version, channel: Channel) -> bool {
    release::is_offered(release, installed, channel)
}

/// Prefix `original` with `mirror`; an empty mirror means direct download.
#[must_use]
pub fn mirror_url(mirror: &str, original: &str) -> String {
    mirror::url(mirror, original)
}

/// Round-trip time for a small ranged request to `url`, in milliseconds.
#[must_use]
pub fn probe(url: &str) -> Option<u128> {
    mirror::probe(url)
}

/// DoH (Cloudflare) A-record lookup. `None` when DoH itself is unreachable.
pub fn resolve_doh(host: &str) -> Option<Vec<String>> {
    mirror::resolve_doh(host)
}

/// What the operating system resolver returns for `host`.
#[must_use]
pub fn system_resolve(host: &str) -> Vec<String> {
    mirror::system_resolve(host)
}

/// Do the two resolvers disagree completely?
#[must_use]
pub fn dns_looks_poisoned(host: &str) -> bool {
    mirror::dns_looks_poisoned(host)
}

/// Pick a download base for `target`. Returns the prefix (`""` = direct).
#[must_use]
pub fn pick_mirror(target: &str, test_all: bool, allow_direct: bool) -> Option<String> {
    mirror::pick(target, test_all, allow_direct)
}

/// Download + verify. Returns the staged file, ready to be swapped in.
pub fn stage(base: &str, release: &Release) -> Result<PathBuf, UpdateError> {
    download::stage(base, release, &mut |_, _| {})
}

/// [`stage`] with byte-level progress reports for the UI thread.
pub fn stage_with_progress(
    base: &str,
    release: &Release,
    on_progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, UpdateError> {
    download::stage(base, release, on_progress)
}

/// Where a staged update lives: next to the running exe, so the final `move`
/// stays on one volume.
#[must_use]
pub fn staging_path() -> PathBuf {
    stage::staging_path()
}

/// Lowercase hex SHA-256 of `data`.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    stage::sha256_hex(data)
}

/// First 64-hex-digit token of a `sha256sum` line.
#[must_use]
pub fn parse_sha256_file(text: &str) -> Option<String> {
    stage::parse_sha256_file(text)
}

/// Swap the staged exe in and restart the application.
pub fn apply_and_restart(staged: &Path) -> Result<(), UpdateError> {
    stage::swap_script(staged, true)
}

/// Swap the staged exe in after this process exits, without restarting.
/// The next manual launch then runs the new build.
pub fn install_on_exit(staged: &Path) -> Result<(), UpdateError> {
    stage::swap_script(staged, false)
}
