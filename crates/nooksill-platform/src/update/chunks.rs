//! Parallel ranged download: split one asset into fixed chunks, fetch them on
//! independent WinHTTP sessions, then concatenate into the single staged file.
//!
//! Chunk boundaries and the 206/200 decision are pure functions so the tricky
//! parts are testable without a socket. A server that ignores `Range` answers
//! 200 and the caller falls back to the single-connection path.

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::download::chunk_path;
use super::release::Asset;
use super::{UpdateError, TIMEOUT_MS, UA};
use crate::net;

/// Fixed chunk size: large enough to amortise TLS handshakes, small enough
/// that a failed slice can be retried without losing the whole transfer.
pub const CHUNK: u64 = 8 * 1024 * 1024;
/// Four connections keep a cross-border link busy without tripping per-IP
/// connection caps on the reverse proxies.
pub const LANES: usize = 4;
/// Below this size the handshake overhead of four sessions exceeds the gain.
pub const MIN_PARALLEL: u64 = 4 * 1024 * 1024;

/// Half-open byte ranges `[start, end)` covering `total`.
#[must_use]
pub fn ranges(total: u64, chunk: u64) -> Vec<(u64, u64)> {
    if total == 0 || chunk == 0 {
        return Vec::new();
    }
    (0..total)
        .step_by(chunk as usize)
        .map(|start| (start, (start + chunk).min(total)))
        .collect()
}

/// Whether a ranged answer may be appended to a chunk file.
#[must_use]
pub const fn accepts_range(status: u32) -> bool {
    status == 206
}

/// Whether an asset is worth fetching in parallel.
#[must_use]
pub const fn worthwhile(size: u64) -> bool {
    size >= MIN_PARALLEL
}

/// Per-chunk headers. `Accept-Encoding: identity` is mandatory: a gzip answer
/// would make the received bytes shorter than the requested range.
fn headers(start: u64, end: u64) -> String {
    format!(
        "User-Agent: {UA}\r\nAccept-Encoding: identity\r\nRange: bytes={}-{}\r\n",
        start,
        end - 1
    )
}

/// Download the missing part of one range into its own file.
///
/// `progress` counts only freshly received bytes, never bytes already on disk.
/// Returns `false` when the source ignored `Range`, so the caller can abandon
/// every chunk and use the single-connection path instead.
fn fetch_one(
    base: &str,
    asset: &Asset,
    start: u64,
    end: u64,
    path: &Path,
    progress: &AtomicU64,
) -> Result<bool, UpdateError> {
    let mut have = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let span = end - start;
    if have > span {
        let _ = std::fs::remove_file(path);
        have = 0;
    }
    let from = start + have;
    if from >= end {
        return Ok(true);
    }
    let url = super::mirror::url(base, &asset.url);
    let mut opened = false;
    let mut rejected = false;
    let mut file = None;
    let result = net::get_stream(
        &url,
        &headers(from, end),
        TIMEOUT_MS,
        net::LARGE_BODY,
        &mut |status, chunk| {
            if !opened {
                opened = true;
                if !accepts_range(status) {
                    rejected = true;
                    return Err(net::NetError::Sink);
                }
                file = Some(
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .map_err(|_| net::NetError::Sink)?,
                );
            }
            if let Some(f) = file.as_mut() {
                f.write_all(chunk).map_err(|_| net::NetError::Sink)?;
            }
            progress.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            Ok(())
        },
    );
    if let Some(f) = file.as_mut() {
        let _ = f.flush();
    }
    drop(file);
    if rejected {
        return Ok(false);
    }
    result?;
    Ok(true)
}

/// Download all chunks with at most [`LANES`] live sessions, then join them
/// into `part`.
///
/// Returns `false` when any source ignored `Range`: the caller must discard the
/// chunk files and fall back. `done` is shared so one progress line covers all
/// lanes; the caller owns the reporting cadence.
pub fn fetch(
    base: &str,
    asset: &Asset,
    part: &Path,
    total: u64,
    done: &Arc<AtomicU64>,
) -> Result<bool, UpdateError> {
    let plan = ranges(total, CHUNK);
    if plan.is_empty() || plan.len() > 4096 {
        return Ok(false);
    }
    let root = PathBuf::from(part);
    let root_ref = &root;
    let queue = Arc::new(Mutex::new(
        plan.iter().copied().enumerate().collect::<VecDeque<_>>(),
    ));
    let accepted = Arc::new(Mutex::new(true));
    let ok = std::thread::scope(|scope| -> Result<(), UpdateError> {
        let mut lanes = Vec::new();
        for _ in 0..LANES.min(plan.len()) {
            let queue = queue.clone();
            let accepted = accepted.clone();
            lanes.push(scope.spawn(move || {
                let local = AtomicU64::new(0);
                loop {
                    let job = queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
                    let Some((i, (start, end))) = job else { break };
                    let path = chunk_path(root_ref, i);
                    match fetch_one(base, asset, start, end, &path, &local) {
                        Ok(true) => {}
                        Ok(false) => *accepted.lock().unwrap_or_else(|e| e.into_inner()) = false,
                        Err(_) => *accepted.lock().unwrap_or_else(|e| e.into_inner()) = false,
                    }
                    let flushed = local.swap(0, Ordering::Relaxed);
                    done.fetch_add(flushed, Ordering::Relaxed);
                }
            }));
        }
        for lane in lanes {
            lane.join()
                .map_err(|_| UpdateError::Http("download lane panicked".to_owned()))?;
        }
        Ok(())
    });
    ok?;
    if !*accepted.lock().unwrap_or_else(|e| e.into_inner()) {
        return Ok(false);
    }

    let mut out = std::fs::File::create(part)?;
    for (i, _) in plan.iter().enumerate() {
        let mut f = std::fs::File::open(chunk_path(&root, i))?;
        std::io::copy(&mut f, &mut out)?;
    }
    out.flush()?;
    Ok(true)
}

/// Drop every chunk file after a fallback or a checksum failure.
pub fn discard(part: &Path) {
    if let Ok(dir) = std::fs::read_dir(part.parent().unwrap_or_else(|| Path::new("."))) {
        let stem = part.file_name().and_then(|s| s.to_str()).unwrap_or("");
        for entry in dir.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(stem) && name.contains(".c") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}
