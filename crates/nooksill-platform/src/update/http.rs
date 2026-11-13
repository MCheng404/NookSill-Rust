//! Release metadata requests.
//!
//! One fixed `per_page` request serves both channels. A persisted ETag turns
//! repeat checks into `304 Not Modified`; rate-limit headers are translated
//! into a readable waiting instruction instead of a blind retry.

use std::time::{SystemTime, UNIX_EPOCH};

use super::release::{self, Channel, Release};
use super::{cache, UpdateError, REPO, TIMEOUT_MS, UA};
use crate::{json, net};

/// Fixed page size: a different size produces a different ETag and would
/// defeat the conditional request.
const PER_PAGE: usize = 15;

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

pub fn latest(channel: Channel) -> Result<Release, UpdateError> {
    let cached = cache::read_release();
    let etag = cached.as_ref().map(|(tag, _)| tag.as_str());
    let res = net::get_raw(&url(), &headers(etag), TIMEOUT_MS, net::SMALL_BODY)?;
    let body = match res.status {
        200 => {
            let text = String::from_utf8_lossy(&res.body).into_owned();
            cache::write_release(res.header("etag"), &text);
            text
        }
        304 => cached.ok_or(UpdateError::Json).map(|(_, text)| text)?,
        403 | 429 => return Err(rate_limited(&res)),
        404 => return Err(UpdateError::NoRelease),
        other => return Err(UpdateError::Http(format!("GitHub 返回 HTTP {other}"))),
    };
    let doc = json::parse(&body).map_err(|_| UpdateError::Json)?;
    let list = doc.as_arr().ok_or(UpdateError::Json)?;
    release::pick(list, channel).ok_or(UpdateError::NoRelease)
}

pub(crate) fn rate_limited(res: &net::Response) -> UpdateError {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
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
    UpdateError::Http(format!(
        "GitHub 接口限流（匿名每小时 60 次）；约 {wait} 秒后可重试"
    ))
}
