//! Streaming download with resume, bounded retries and per-mirror verification.
//!
//! Every retry starts from the original `browser_download_url`: GitHub redirects
//! to a signed CDN URL that expires in minutes and must never be cached.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::release::{Asset, Release};
use super::{cache, mirror, stage as staging, UpdateError, TIMEOUT_MS, UA};
use crate::net;

const MAX_ATTEMPTS: usize = 3;

/// Download one asset to `part`, resuming from the bytes already on disk.
///
/// A `206` appends; a `200` means the server ignored `Range` and the file must
/// be truncated, otherwise the concatenation would silently corrupt the exe.
fn fetch(
    base: &str,
    asset: &Asset,
    part: &Path,
    expected_size: u64,
    on_progress: &mut dyn FnMut(u64, u64),
) -> Result<(), UpdateError> {
    let url = mirror::url(base, &asset.url);
    let mut resume = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    if expected_size > 0 {
        if resume > expected_size {
            let _ = std::fs::remove_file(part);
            resume = 0;
        } else if resume == expected_size {
            return Ok(());
        }
    }

    let mut last = String::from("no response");
    for attempt in 0..MAX_ATTEMPTS {
        let headers = if resume > 0 {
            format!("User-Agent: {UA}\r\nRange: bytes={resume}-\r\n")
        } else {
            format!("User-Agent: {UA}\r\n")
        };
        let mut opened = false;
        let mut seen_status = 0u32;
        let mut file: Option<std::fs::File> = None;
        let mut written = 0u64;
        let mut last_report = 0u64;
        let result = net::get_stream(
            &url,
            &headers,
            TIMEOUT_MS,
            net::LARGE_BODY,
            &mut |status, chunk| {
                if !opened {
                    opened = true;
                    seen_status = status;
                    match status {
                        200 => {
                            file =
                                Some(std::fs::File::create(part).map_err(|_| net::NetError::Sink)?);
                        }
                        206 => {
                            file = Some(
                                std::fs::OpenOptions::new()
                                    .append(true)
                                    .open(part)
                                    .map_err(|_| net::NetError::Sink)?,
                            );
                        }
                        _ => return Err(net::NetError::Sink),
                    }
                    written = if status == 206 { resume } else { 0 };
                    last_report = written;
                    on_progress(written, expected_size);
                }
                if let Some(f) = file.as_mut() {
                    f.write_all(chunk).map_err(|_| net::NetError::Sink)?;
                }
                written += chunk.len() as u64;
                // Throttle to 256 KiB: a 35 MB asset would otherwise queue
                // hundreds of UI messages for no visible gain.
                if written.saturating_sub(last_report) >= 256 * 1024 {
                    on_progress(written, expected_size);
                    last_report = written;
                }
                Ok(())
            },
        );
        if let Some(f) = file.as_mut() {
            let _ = f.flush();
        }
        drop(file);
        let complete = matches!(&result, Ok(r) if r.status == 200 || r.status == 206);
        if complete {
            on_progress(written, expected_size);
        }

        match result {
            Ok(res) if res.status == 200 || res.status == 206 => return Ok(()),
            Ok(res) if res.status == 416 => {
                // A bodiless 416 never reaches the sink, so this path must
                // also drop the stale partial before the next attempt.
                let _ = std::fs::remove_file(part);
                last = "HTTP 416".to_owned();
            }
            Ok(res) => last = format!("HTTP {}", res.status),
            Err(net::NetError::Sink) if seen_status == 416 => {
                // Stale partial range: drop it and start a fresh request.
                let _ = std::fs::remove_file(part);
                last = "HTTP 416".to_owned();
            }
            Err(net::NetError::Sink) => last = format!("HTTP {seen_status}"),
            Err(e) => last = e.to_string(),
        }
        // Exponential backoff with jitter-free spacing; the worker thread is
        // allowed to sleep, the UI thread never calls this.
        std::thread::sleep(Duration::from_millis(1000_u64 << attempt));
        resume = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    }
    Err(UpdateError::Http(last))
}

/// The API digest is authoritative; a `.sha256` side-car is only a fallback
/// for releases published before digests were exposed.
fn expected_digest(base: &str, release: &Release) -> Result<String, UpdateError> {
    if let Some(digest) = &release.exe.digest {
        return Ok(digest.clone());
    }
    let Some(sum) = &release.sha256 else {
        return Err(UpdateError::NoAsset);
    };
    let url = mirror::url(base, &sum.url);
    let res = net::get(
        &url,
        &format!("User-Agent: {UA}\r\n"),
        TIMEOUT_MS,
        net::SMALL_BODY,
    )?;
    let text = String::from_utf8_lossy(&res.body);
    staging::parse_sha256_file(&text).ok_or_else(|| {
        UpdateError::Http(
            nooksill_core::i18n::t("The checksum file has an unexpected format").to_owned(),
        )
    })
}

fn ordered_bases(preferred: &str) -> Vec<String> {
    let mut bases = vec![preferred.to_owned()];
    for b in mirror::candidates(true) {
        if !bases.contains(&b) {
            bases.push(b);
        }
    }
    bases
}

/// Download + verify. The first base producing a hash-matching file wins; a
/// checksum mismatch switches sources instead of retrying the same bytes.
pub fn stage(
    base: &str,
    release: &Release,
    on_progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, UpdateError> {
    let mut last = String::from("no usable source");
    for candidate in ordered_bases(base) {
        let Ok(expected) = expected_digest(&candidate, release) else {
            last = "no checksum available".to_owned();
            continue;
        };
        let part = staging::part_path(&expected, &release.tag);
        if let Err(e) = fetch(
            &candidate,
            &release.exe,
            &part,
            release.exe.size,
            on_progress,
        ) {
            last = e.to_string();
            continue;
        }
        let Ok(len) = std::fs::metadata(&part).map(|m| m.len()) else {
            last = "partial file missing".to_owned();
            continue;
        };
        if release.exe.size > 0 && len != release.exe.size {
            last = format!("size {len} != {}", release.exe.size);
            continue;
        }
        let actual = staging::sha256_file(&part)?;
        if actual != expected {
            let _ = std::fs::remove_file(&part);
            last = format!("checksum mismatch from {candidate}");
            continue;
        }
        staging::write_marker(&expected, len)?;
        let path = staging::finalize(&part)?;
        cache::remember_mirror(&candidate);
        return Ok(path);
    }
    Err(UpdateError::AllMirrorsFailed(last))
}
