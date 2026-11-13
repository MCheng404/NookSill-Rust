//! Release metadata: channel filtering, asset parsing and version discovery.

use super::version::Version;
use super::UpdateError;
use crate::json::Json;

/// Which GitHub releases the install follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Beta,
}

impl Channel {
    pub const ALL: [Channel; 2] = [Channel::Stable, Channel::Beta];

    /// Config values outside 0/1 fall back to stable.
    #[must_use]
    pub fn from_i32(value: i32) -> Self {
        if value == 1 {
            Channel::Beta
        } else {
            Channel::Stable
        }
    }

    #[must_use]
    pub fn as_i32(self) -> i32 {
        match self {
            Channel::Stable => 0,
            Channel::Beta => 1,
        }
    }
}

/// One downloadable file from the release.
#[derive(Debug, Clone)]
pub struct Asset {
    pub name: String,
    pub url: String,
    /// `sha256:` digest from the API, present on assets uploaded since 2025-06.
    pub digest: Option<String>,
    pub size: u64,
}

/// Everything the updater needs from a release.
#[derive(Debug, Clone)]
pub struct Release {
    pub tag: String,
    pub version: Version,
    pub notes: String,
    pub exe: Asset,
    /// Optional: a `.sha256` side-car is only used when the API has no digest.
    pub sha256: Option<Asset>,
    pub prerelease: bool,
}

/// Channel rule: a stable install never receives a pre-release unless it opted
/// into the beta channel. The rolling `dev` build shares its version triple
/// with the stable release it was cut from, so the beta channel compares dev
/// build numbers instead of the triple; switching from stable to beta is an
/// offer even when the triples match.
#[must_use]
pub fn is_offered(release: &Release, installed: &Version, channel: Channel) -> bool {
    match channel {
        Channel::Stable => !release.prerelease && release.version.is_newer_than(installed),
        Channel::Beta if release.prerelease => match (release.version.build, installed.build) {
            (Some(a), Some(b)) => a > b,
            (Some(_), None) => true,
            _ => release.version.is_newer_than(installed),
        },
        Channel::Beta => release.version.is_newer_than(installed),
    }
}

/// Newest usable release from the `/releases` array for `channel`.
///
/// Drafts, half-uploaded assets and entries without an exe are skipped; the
/// highest version wins, not the newest entry, so a re-cut old release cannot
/// shadow a newer one.
pub fn pick(list: &[Json], channel: Channel) -> Option<Release> {
    let mut best_stable: Option<Release> = None;
    let mut best_pre: Option<Release> = None;
    for item in list {
        if item.get("draft").and_then(Json::as_bool).unwrap_or(false) {
            continue;
        }
        let Ok(release) = parse_release(item) else {
            continue;
        };
        let slot = if release.prerelease {
            &mut best_pre
        } else {
            &mut best_stable
        };
        if slot
            .as_ref()
            .is_none_or(|b| release.version.is_newer_than(&b.version))
        {
            *slot = Some(release);
        }
    }
    // A beta subscriber wants the test build even when a stable release with
    // the same triple exists and would win a plain version comparison.
    match channel {
        Channel::Stable => best_stable,
        Channel::Beta => best_pre.or(best_stable),
    }
}

/// Both the single-release and list endpoints return the same object shape.
pub fn parse_release(doc: &Json) -> Result<Release, UpdateError> {
    let tag = doc
        .get("tag_name")
        .and_then(Json::as_str)
        .ok_or(UpdateError::Json)?
        .to_owned();
    let version = version_of(doc).ok_or(UpdateError::Json)?;
    let notes = doc
        .get("body")
        .and_then(Json::as_str)
        .unwrap_or_default()
        .to_owned();
    let prerelease = doc
        .get("prerelease")
        .and_then(Json::as_bool)
        .unwrap_or(false);

    let assets = doc
        .get("assets")
        .and_then(Json::as_arr)
        .ok_or(UpdateError::Json)?;
    let mut exe = None;
    let mut sha256 = None;
    for item in assets {
        let (Some(name), Some(url)) = (
            item.get("name").and_then(Json::as_str),
            item.get("browser_download_url").and_then(Json::as_str),
        ) else {
            continue;
        };
        // GitHub marks an asset "uploaded" only after the bytes are complete.
        if let Some(state) = item.get("state").and_then(Json::as_str) {
            if state != "uploaded" {
                continue;
            }
        }
        let asset = Asset {
            name: name.to_owned(),
            url: url.to_owned(),
            digest: parse_digest(item.get("digest").and_then(Json::as_str)),
            size: item
                .get("size")
                .and_then(Json::as_f64)
                .map(|v| v.max(0.0) as u64)
                .unwrap_or(0),
        };
        if name.ends_with(".exe.sha256") {
            sha256 = Some(asset);
        } else if name.ends_with(".exe") {
            exe = Some(asset);
        }
    }
    Ok(Release {
        tag,
        version,
        notes,
        exe: exe.ok_or(UpdateError::NoAsset)?,
        sha256,
        prerelease,
    })
}

/// GitHub exposes `sha256:<hex>`; the prefix is not part of the checksum.
fn parse_digest(raw: Option<&str>) -> Option<String> {
    let hex = raw?.strip_prefix("sha256:")?;
    let ok = hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit());
    ok.then(|| hex.to_ascii_lowercase())
}

/// The version is not always in `tag_name`.
///
/// The rolling pre-release tag is a fixed `dev` (each build overwrites the same
/// release), so the version lives in the title or the asset name. Asset names are
/// tried before the title because they come from the build itself.
fn version_of(doc: &Json) -> Option<Version> {
    if let Some(tag) = doc.get("tag_name").and_then(Json::as_str) {
        if let Some(v) = version_in(tag) {
            return Some(v);
        }
    }
    if let Some(assets) = doc.get("assets").and_then(Json::as_arr) {
        for item in assets {
            if let Some(name) = item.get("name").and_then(Json::as_str) {
                if let Some(v) = version_in(name) {
                    return Some(v);
                }
            }
        }
    }
    let title = doc.get("name").and_then(Json::as_str)?;
    title.split_whitespace().find_map(version_in)
}

/// Extract a version from arbitrary text.
///
/// Accepts `v1.0.0`, `1.0.0-8-abc1234` and `NookSill-1.0.0-8-abc1234-win64.exe`.
/// `text` is rebuilt in canonical form so asset prefixes never reach the UI.
pub(crate) fn version_in(raw: &str) -> Option<Version> {
    let start = raw.find(|c: char| c.is_ascii_digit())?;
    let rest = &raw[start..];
    let mut parsed = Version::parse(rest)?;
    // Only accept a sane dotted triple, so dates like `2026.11.06` and junk
    // after a fourth group are rejected.
    if parsed.major > 999 || parsed.minor > 999 || parsed.patch > 999 {
        return None;
    }
    parsed.text = match parsed.build {
        Some(build) => format!("{}.{}.{}-{build}", parsed.major, parsed.minor, parsed.patch),
        None => format!("{}.{}.{}", parsed.major, parsed.minor, parsed.patch),
    };
    Some(parsed)
}

/// Parsed for tests without a network round trip.
#[cfg(test)]
pub(crate) fn digest_of(raw: Option<&str>) -> Option<String> {
    parse_digest(raw)
}
