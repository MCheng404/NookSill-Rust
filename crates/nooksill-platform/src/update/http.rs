//! Release metadata requests.
//!
//! One fixed `per_page` request serves both channels. A persisted ETag turns
//! repeat checks into `304 Not Modified`; a rate-limit answer is remembered on
//! disk so neither the daily check nor manual checks keep hammering a limited
//! endpoint ("continuing to make requests while you are rate limited may
//! result in the banning of your integration").

use std::time::{SystemTime, UNIX_EPOCH};

use super::release::{self, Channel, Release};
use super::{cache, UpdateError, REPO, TIMEOUT_MS, UA};
use crate::{json, net};

/// Fixed page size: a different size produces a different ETag and would
/// defeat the conditional request.
const PER_PAGE: usize = 15;
/// Never back off longer than an hour, even if the server asks for more.
const MAX_BACKOFF_SECS: u64 = 3600;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn url() -> String {
    format!("https://api.github.com/repos/{REPO}/releases?per_page={PER_PAGE}")
}

/// GitHub rejects requests without a valid User-Agent. The ETag must be sent
/// back verbatim, including a `W/` prefix and its quotes.
pub(crate) fn headers(etag: Option<&str>) -> String {
    let mut block = format!(
        "Accept: application/vnd.github+json\r\n\
         User-Agent: {UA}/2.0 (+https://github.com/{REPO})\r\n\
         X-GitHub-Api-Version: 2022-11-28\r\n"
    );
    if let Some(tag) = etag {
        if !tag.is_empty() && tag != "-" {
            block.push_str(&format!("If-None-Match: {tag}\r\n"));
        }
    }
    block
}

/// Extract the tag from a `/releases/latest` redirect target.
///
/// The web endpoint answers `302` with a `Location` ending in `/releases/tag/<tag>`.
/// Only that exact shape is accepted: a login page or a changed redirect must
/// fail rather than be read as a version.
#[must_use]
pub(crate) fn tag_from_location(location: &str) -> Option<&str> {
    let (_, tail) = location.split_once("/releases/tag/")?;
    let tag = tail.split(['?', '#']).next()?.trim_end_matches('/');
    if tag.is_empty() || tag.len() > 64 {
        return None;
    }
    tag.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        .then_some(tag)
}

/// Resolve the newest stable tag through the web front end, not the REST API.
///
/// This request does not consume the shared 60/hour quota, which is the whole
/// point: a NAT or corporate proxy no longer blocks the daily check. `None` on
/// any deviation so the caller can fall back to the API unchanged.
fn web_tag() -> Option<Release> {
    let url = format!("https://github.com/{REPO}/releases/latest");
    let res = net::get_no_redirect(
        &url,
        &format!("User-Agent: {UA}\r\n"),
        TIMEOUT_MS,
        net::SMALL_BODY,
    )
    .ok()?;
    if !(300..400).contains(&res.status) {
        return None;
    }
    let location = res.header("location")?;
    Release::from_tag(tag_from_location(location)?)
}

pub fn latest(channel: Channel) -> Result<Release, UpdateError> {
    // The web redirect only knows stable releases; beta users need the API to
    // see the rolling pre-release, so they keep the existing path.
    if channel == Channel::Stable {
        if let Some(release) = web_tag() {
            return Ok(release);
        }
    }

    let now = now_secs();
    if let Some(until) = cache::retry_until() {
        if until > now {
            return Err(UpdateError::Http(nooksill_core::i18n::fmt(
                "GitHub API rate limit; retry in about {0} seconds (no request was made)",
                &[(until - now).to_string().as_str()],
            )));
        }
        cache::clear_retry();
    }

    let cached = cache::read_release();
    let etag = cached.as_ref().map(|(tag, _)| tag.as_str());
    let res = net::get_raw(&url(), &headers(etag), TIMEOUT_MS, net::SMALL_BODY)?;
    let body = match res.status {
        200 => {
            let text = String::from_utf8_lossy(&res.body).into_owned();
            cache::write_release(res.header("etag"), &text);
            cache::clear_retry();
            text
        }
        304 => {
            let Some((_, text)) = cached else {
                return Err(UpdateError::Json);
            };
            if json::parse(&text).is_err() {
                // Drop the ETag and body so the next check gets a full 200.
                cache::clear_release();
                return Err(UpdateError::Http(
                    nooksill_core::i18n::t(
                        "The local update cache was corrupt and has been cleared; check again",
                    )
                    .to_owned(),
                ));
            }
            text
        }
        403 | 429 => {
            let (err, wait) = rate_limited(&res, now);
            cache::remember_retry(now + wait.min(MAX_BACKOFF_SECS));
            return Err(err);
        }
        404 => return Err(UpdateError::NoRelease),
        other => {
            return Err(UpdateError::Http(nooksill_core::i18n::fmt(
                "GitHub returned HTTP {0}",
                &[other.to_string().as_str()],
            )))
        }
    };
    let doc = json::parse(&body).map_err(|_| UpdateError::Json)?;
    let list = doc.as_arr().ok_or(UpdateError::Json)?;
    release::pick(list, channel).ok_or(UpdateError::NoRelease)
}

/// Pure so the waiting instruction can be asserted offline; the caller owns
/// persisting the backoff deadline.
pub(crate) fn rate_limited(res: &net::Response, now: u64) -> (UpdateError, u64) {
    let reset = res
        .header("x-ratelimit-reset")
        .and_then(|v| v.trim().parse::<u64>().ok());
    let retry = res
        .header("retry-after")
        .and_then(|v| v.trim().parse::<u64>().ok());
    let wait = reset
        .map(|r| r.saturating_sub(now))
        .filter(|w| *w > 0)
        .or(retry)
        .unwrap_or(60);
    (
        UpdateError::Http(nooksill_core::i18n::fmt(
            "GitHub API rate limit (60/hour for anonymous requests); retry in about {0} seconds",
            &[wait.to_string().as_str()],
        )),
        wait,
    )
}
