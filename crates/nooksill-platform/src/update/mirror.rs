//! Mirror ordering and the DNS-poisoning heuristic.
//!
//! Probes are serial and read at most 64 KiB: a first-byte race neither
//! measures throughput nor justifies a full extra download of every asset.

use std::time::Instant;

use super::{cache, FAST_ENOUGH_MS, MIRRORS, PROBE_TIMEOUT_MS, UA};
use crate::{json, net};

/// Prefix `original` with `mirror`; an empty mirror means direct download.
#[must_use]
pub fn url(mirror: &str, original: &str) -> String {
    if mirror.is_empty() {
        original.to_owned()
    } else {
        format!("{mirror}{original}")
    }
}

/// Ordered candidates: last verified source, then the built-in mirrors, then
/// GitHub direct. The direct URL is deliberately last because it is correct
/// but usually the slowest from this product's main audience.
pub fn candidates(allow_direct: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(sticky) = cache::sticky_mirror() {
        if allow_direct || !sticky.is_empty() {
            out.push(sticky);
        }
    }
    for m in MIRRORS {
        out.push((*m).to_owned());
    }
    if allow_direct {
        out.push(String::new());
    }
    let mut seen = Vec::new();
    out.retain(|b| {
        if seen.contains(b) {
            false
        } else {
            seen.push(b.clone());
            true
        }
    });
    out
}

/// Time a small ranged request. A `Range` header is only a hint: servers that
/// ignore it answer 200 with the whole asset, so the sink stops after 64 KiB.
#[must_use]
pub fn probe(target: &str) -> Option<u128> {
    let start = Instant::now();
    let headers = format!("User-Agent: {UA}\r\nRange: bytes=0-1023\r\n");
    let mut got = 0usize;
    let result = net::get_stream(
        target,
        &headers,
        PROBE_TIMEOUT_MS,
        64 * 1024,
        &mut |_status, part| {
            got += part.len();
            if got > 64 * 1024 {
                Err(net::NetError::Sink)
            } else {
                Ok(())
            }
        },
    );
    match result {
        Ok(_) | Err(net::NetError::Sink) | Err(net::NetError::TooLarge(_)) => {
            Some(start.elapsed().as_millis())
        }
        Err(_) => None,
    }
}

/// Pick a download base for `target`. Returns the prefix (`""` = direct).
#[must_use]
pub fn pick(target: &str, test_all: bool, allow_direct: bool) -> Option<String> {
    let mut best: Option<(u128, String)> = None;
    for base in candidates(allow_direct) {
        let Some(ms) = probe(&url(&base, target)) else {
            continue;
        };
        if !test_all && ms <= FAST_ENOUGH_MS {
            return Some(base);
        }
        if best.as_ref().is_none_or(|(best_ms, _)| ms < *best_ms) {
            best = Some((ms, base));
        }
    }
    best.map(|(_, base)| base)
}

/// DoH (Cloudflare) A-record lookup. `None` when DoH itself is unreachable.
pub fn resolve_doh(host: &str) -> Option<Vec<String>> {
    let url = format!("https://cloudflare-dns.com/dns-query?name={host}&type=A");
    let res = net::get(
        &url,
        "Accept: application/dns-json\r\n",
        PROBE_TIMEOUT_MS,
        net::SMALL_BODY,
    )
    .ok()?;
    let text = String::from_utf8_lossy(&res.body);
    let doc = json::parse(&text).ok()?;
    let answers = doc.get("Answer")?.as_arr()?;
    let ips: Vec<String> = answers
        .iter()
        .filter_map(|a| a.get("data").and_then(json::Json::as_str))
        .filter(|ip| ip.parse::<std::net::IpAddr>().is_ok())
        .map(str::to_owned)
        .collect();
    (!ips.is_empty()).then_some(ips)
}

/// What the operating system resolver returns for `host`.
#[must_use]
pub fn system_resolve(host: &str) -> Vec<String> {
    use std::net::ToSocketAddrs;
    (host, 443u16)
        .to_socket_addrs()
        .map(|it| it.map(|a| a.ip().to_string()).collect())
        .unwrap_or_default()
}

/// Do the two resolvers disagree completely?
///
/// WinHTTP validates TLS against the host name from `WinHttpConnect`, so a DoH
/// answer cannot be used as transport; a total mismatch is the classic
/// poisoning signature and only disables the direct path.
#[must_use]
pub fn dns_looks_poisoned(host: &str) -> bool {
    let Some(doh) = resolve_doh(host) else {
        return false;
    };
    let system = system_resolve(host);
    !system.is_empty() && system.iter().all(|ip| !doh.contains(ip))
}
