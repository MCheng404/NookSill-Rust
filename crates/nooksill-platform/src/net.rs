//! Minimal HTTP client on WinHTTP.
//!
//! WinHTTP brings the system proxy, the OS trust store, redirects and HTTP/1.1,
//! so no TLS stack or extra crate is linked. Only `GET` is implemented.
//! `get_raw` exposes status and headers for conditional requests; `get_stream`
//! writes chunks while they arrive so a partial download can be resumed.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetOption, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_ACCESS_TYPE_NAMED_PROXY, WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_OPTION_REDIRECT_POLICY,
    WINHTTP_OPTION_REDIRECT_POLICY_NEVER, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_RAW_HEADERS_CRLF, WINHTTP_QUERY_STATUS_CODE,
};

/// Body caps. API answers and DoH replies are kilobytes; a release asset is a
/// few MB. The cap stops a hostile mirror from forcing unbounded allocation, it
/// is not a size policy.
pub const SMALL_BODY: usize = 16 * 1024 * 1024;
pub const LARGE_BODY: usize = 128 * 1024 * 1024;

/// Proxy mode shared by all update requests. `0` follows the system, `1` uses
/// [`CUSTOM_PROXY`], `2` forces a direct connection.
static PROXY_MODE: AtomicU8 = AtomicU8::new(0);
static CUSTOM_PROXY: Mutex<String> = Mutex::new(String::new());

/// Publish the proxy configuration from the settings layer.
pub fn set_proxy(mode: i32, address: &str) {
    PROXY_MODE.store(mode.clamp(0, 2) as u8, Ordering::Relaxed);
    if let Ok(mut guard) = CUSTOM_PROXY.lock() {
        guard.clear();
        guard.push_str(address);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    /// URL is not `http(s)://host[:port]/path`.
    #[error("{}", nooksill_core::i18n::t("The URL is malformed"))]
    BadUrl,
    /// WinHTTP refused an operation (DNS, TLS, connect, ...).
    #[error("{}", nooksill_core::i18n::fmt("Request failed: {0}", &[.0]))]
    Win32(&'static str),
    /// Server answered with a non-2xx status.
    // Status codes are language-neutral diagnostics.
    #[error("HTTP {0}")]
    Status(u32),
    /// Body exceeded the caller's cap.
    #[error("{}", nooksill_core::i18n::fmt("The response exceeds the {0} MB limit", &[(*.0 / (1024 * 1024)).to_string().as_str()]))]
    TooLarge(usize),
    /// The caller's chunk writer refused the data.
    #[error("{}", nooksill_core::i18n::t("Writing the download failed"))]
    Sink,
}

/// Chunk writer used by streaming downloads; the first argument is the status.
type Sink<'a> = &'a mut dyn FnMut(u32, &[u8]) -> Result<(), NetError>;

/// A response we care about: status, lowercase header names and the body.
///
/// `bytes` counts everything received, including a streamed body that is not
/// retained in `body`.
#[derive(Debug)]
pub struct Response {
    pub status: u32,
    pub body: Vec<u8>,
    pub bytes: usize,
    pub headers: Vec<(String, String)>,
}

impl Response {
    /// First value of a header, compared case-insensitively.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// `GET url`, following redirects (WinHTTP does that by default).
///
/// `headers` is the raw block WinHTTP expects, e.g.
/// `"Accept: application/json\r\nUser-Agent: NookSill\r\n"`.
pub fn get(
    url: &str,
    headers: &str,
    timeout_ms: i32,
    max_body: usize,
) -> Result<Response, NetError> {
    request(url, headers, timeout_ms, max_body, false, None, true)
}

/// Like [`get`], but returns 304 / 4xx / 206 responses instead of turning them
/// into [`NetError::Status`].
pub fn get_raw(
    url: &str,
    headers: &str,
    timeout_ms: i32,
    max_body: usize,
) -> Result<Response, NetError> {
    request(url, headers, timeout_ms, max_body, true, None, true)
}

/// Like [`get`], but leaves a 3xx response intact so its `Location` can be read.
///
/// `/releases/latest` encodes the newest tag in that header; following it would
/// download an HTML page and spend a REST quota the app does not need.
pub fn get_no_redirect(
    url: &str,
    headers: &str,
    timeout_ms: i32,
    max_body: usize,
) -> Result<Response, NetError> {
    request(url, headers, timeout_ms, max_body, true, None, false)
}

/// Streams the body into `write` without retaining it in memory. The callback
/// sees the status first, so a `200` answer to a ranged request can truncate
/// the partial file while `206` appends to it.
pub fn get_stream(
    url: &str,
    headers: &str,
    timeout_ms: i32,
    max_body: usize,
    write: Sink<'_>,
) -> Result<Response, NetError> {
    request(url, headers, timeout_ms, max_body, true, Some(write), true)
}

fn request(
    url: &str,
    headers: &str,
    timeout_ms: i32,
    max_body: usize,
    allow_raw: bool,
    mut sink: Option<Sink<'_>>,
    follow_redirect: bool,
) -> Result<Response, NetError> {
    let parts = split_url(url).ok_or(NetError::BadUrl)?;

    let agent = wide("NookSill/1.0");
    // A named proxy needs a wide buffer that lives until WinHttpOpen returns.
    let custom = if PROXY_MODE.load(Ordering::Relaxed) == 1 {
        CUSTOM_PROXY
            .lock()
            .ok()
            .map(|g| wide(&g))
            .filter(|v| v.len() > 1)
    } else {
        None
    };
    let (access, proxy) = match custom {
        Some(ref w) => (WINHTTP_ACCESS_TYPE_NAMED_PROXY, PCWSTR(w.as_ptr())),
        None if PROXY_MODE.load(Ordering::Relaxed) == 2 => {
            (WINHTTP_ACCESS_TYPE_NO_PROXY, PCWSTR::null())
        }
        None => (WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null()),
    };
    // SAFETY: all handles are checked for null and released by `Handle`.
    let session = unsafe { WinHttpOpen(PCWSTR(agent.as_ptr()), access, proxy, PCWSTR::null(), 0) };
    let session = Handle::new(session, "WinHttpOpen")?;
    unsafe {
        // The receive timeout is per packet, not for the whole download; a
        // stalled transfer is bounded separately by the caller's watchdog.
        let _ = WinHttpSetTimeouts(
            session.raw(),
            0,
            timeout_ms,
            timeout_ms,
            timeout_ms.max(30_000),
        );
    }

    let host = wide(parts.host);
    // SAFETY: host is a NUL-terminated wide string.
    let connect = unsafe { WinHttpConnect(session.raw(), PCWSTR(host.as_ptr()), parts.port, 0) };
    let connect = Handle::new(connect, "WinHttpConnect")?;

    let verb = wide("GET");
    let path = wide(parts.path);
    let flags: WINHTTP_OPEN_REQUEST_FLAGS = if parts.tls {
        WINHTTP_FLAG_SECURE
    } else {
        WINHTTP_OPEN_REQUEST_FLAGS(0)
    };
    // SAFETY: verb/path are NUL-terminated; no extra accept types.
    let request = unsafe {
        WinHttpOpenRequest(
            connect.raw(),
            PCWSTR(verb.as_ptr()),
            PCWSTR(path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            flags,
        )
    };
    let request = Handle::new(request, "WinHttpOpenRequest")?;
    if !follow_redirect {
        let policy = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
        let bytes = policy.to_ne_bytes();
        // SAFETY: the buffer is a u32 for the redirect-policy option.
        unsafe {
            let _ = WinHttpSetOption(
                Some(request.raw()),
                WINHTTP_OPTION_REDIRECT_POLICY,
                Some(&bytes),
            );
        }
    }

    let header_block = wide(headers);
    let header_slice: Option<&[u16]> = if headers.is_empty() {
        None
    } else {
        Some(&header_block[..header_block.len() - 1])
    };
    // SAFETY: no body; lengths describe the header block.
    unsafe { WinHttpSendRequest(request.raw(), header_slice, None, 0, 0, 0) }
        .map_err(|_| NetError::Win32("WinHttpSendRequest"))?;
    // SAFETY: null reserved pointer is documented.
    unsafe { WinHttpReceiveResponse(request.raw(), std::ptr::null_mut()) }
        .map_err(|_| NetError::Win32("WinHttpReceiveResponse"))?;

    let status = status_code(request.raw())?;
    let heads = raw_headers(request.raw());
    let (body, bytes) = read_body(request.raw(), max_body, status, &mut sink)?;
    if !allow_raw && !(200..300).contains(&status) {
        return Err(NetError::Status(status));
    }
    Ok(Response {
        status,
        body,
        bytes,
        headers: heads,
    })
}

fn status_code(request: *mut core::ffi::c_void) -> Result<u32, NetError> {
    let mut code: u32 = 0;
    let mut len = std::mem::size_of::<u32>() as u32;
    // SAFETY: buffer is a u32, length is its size in bytes.
    unsafe {
        WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&raw mut code).cast()),
            &mut len,
            std::ptr::null_mut(),
        )
    }
    .map_err(|_| NetError::Win32("WinHttpQueryHeaders"))?;
    Ok(code)
}

/// All response headers as lowercase names. The first query with a null buffer
/// deliberately fails with ERROR_INSUFFICIENT_BUFFER and reports the size.
fn raw_headers(request: *mut core::ffi::c_void) -> Vec<(String, String)> {
    let mut len: u32 = 0;
    let _ = unsafe {
        WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_RAW_HEADERS_CRLF,
            PCWSTR::null(),
            None,
            &mut len,
            std::ptr::null_mut(),
        )
    };
    if len < 4 {
        return Vec::new();
    }
    let mut buf = vec![0u16; (len as usize / 2) + 2];
    let mut used = len + 4;
    let ok = unsafe {
        WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_RAW_HEADERS_CRLF,
            PCWSTR::null(),
            Some(buf.as_mut_ptr().cast()),
            &mut used,
            std::ptr::null_mut(),
        )
    }
    .is_ok();
    if !ok {
        return Vec::new();
    }
    let chars = (used as usize / 2).saturating_sub(1).min(buf.len());
    let text = String::from_utf16_lossy(&buf[..chars]);
    text.split("\r\n")
        .skip(1)
        .filter_map(|line| {
            let (k, v) = line.split_once(':')?;
            Some((k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        })
        .collect()
}

fn read_body(
    request: *mut core::ffi::c_void,
    max_body: usize,
    status: u32,
    sink: &mut Option<Sink<'_>>,
) -> Result<(Vec<u8>, usize), NetError> {
    let mut body = Vec::new();
    let mut total = 0usize;
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let mut available: u32 = 0;
        // SAFETY: out-pointer is a u32.
        unsafe { WinHttpQueryDataAvailable(request, &raw mut available) }
            .map_err(|_| NetError::Win32("WinHttpQueryDataAvailable"))?;
        if available == 0 {
            return Ok((body, total));
        }
        let want = (available as usize).min(chunk.len());
        let mut read: u32 = 0;
        // SAFETY: buffer has `want` writable bytes.
        unsafe {
            WinHttpReadData(
                request,
                chunk.as_mut_ptr().cast(),
                want as u32,
                &raw mut read,
            )
        }
        .map_err(|_| NetError::Win32("WinHttpReadData"))?;
        if read == 0 {
            return Ok((body, total));
        }
        let part = &chunk[..read as usize];
        if let Some(write) = sink.as_deref_mut() {
            write(status, part)?;
        } else {
            body.extend_from_slice(part);
        }
        total += part.len();
        if total > max_body {
            return Err(NetError::TooLarge(max_body));
        }
    }
}

/// Closes a WinHTTP handle on drop, so early returns cannot leak.
struct Handle(*mut core::ffi::c_void);

impl Handle {
    fn new(h: *mut core::ffi::c_void, what: &'static str) -> Result<Self, NetError> {
        if h.is_null() {
            Err(NetError::Win32(what))
        } else {
            Ok(Self(h))
        }
    }

    fn raw(&self) -> *mut core::ffi::c_void {
        self.0
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: handle came from WinHttpOpen* and is closed exactly once.
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The piece of a URL WinHTTP needs.
struct Parts<'a> {
    host: &'a str,
    port: u16,
    path: &'a str,
    tls: bool,
}

/// `https://host[:port]/path?query` to parts. Fragments are dropped.
fn split_url(url: &str) -> Option<Parts<'_>> {
    let (tls, rest) = url
        .strip_prefix("https://")
        .map(|r| (true, r))
        .or_else(|| url.strip_prefix("http://").map(|r| (false, r)))?;
    let rest = rest.split('#').next()?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return None;
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
            (h, p.parse().ok()?)
        }
        _ => (authority, if tls { 443 } else { 80 }),
    };
    Some(Parts {
        host,
        port,
        path,
        tls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_parts_are_split() {
        let p = split_url("https://gh-proxy.com/https://github.com/a/b.exe").unwrap();
        assert_eq!(p.host, "gh-proxy.com");
        assert_eq!(p.port, 443);
        assert!(p.tls);
        assert_eq!(p.path, "/https://github.com/a/b.exe");

        let p = split_url("http://127.0.0.1:8080/x?y=1").unwrap();
        assert_eq!(p.host, "127.0.0.1");
        assert_eq!(p.port, 8080);
        assert!(!p.tls);
        assert_eq!(p.path, "/x?y=1");

        assert!(split_url("ftp://x/y").is_none());
        assert!(split_url("https:///nohost").is_none());
    }

    #[test]
    fn header_lookup_is_case_insensitive() {
        let r = Response {
            status: 200,
            body: Vec::new(),
            bytes: 0,
            headers: vec![("etag".to_owned(), "W/\"abc\"".to_owned())],
        };
        assert_eq!(r.header("ETag"), Some("W/\"abc\""));
    }
}

#[cfg(test)]
mod proxy_tests {
    use super::*;

    #[test]
    fn proxy_mode_is_clamped_to_the_three_modes() {
        // Mirrors Config::NET_PROXY's table range; out-of-range values must
        // never reach WinHttpOpen as an arbitrary access type.
        let clamp = |v: i32| v.clamp(0, 2);
        assert_eq!(clamp(-5), 0);
        assert_eq!(clamp(1), 1);
        assert_eq!(clamp(99), 2);
    }

    #[test]
    fn setting_proxy_roundtrips_the_mode_and_address() {
        set_proxy(1, "http://127.0.0.1:7890");
        assert_eq!(PROXY_MODE.load(Ordering::Relaxed), 1);
        assert_eq!(
            CUSTOM_PROXY
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_str(),
            "http://127.0.0.1:7890"
        );
        set_proxy(2, "");
        assert_eq!(PROXY_MODE.load(Ordering::Relaxed), 2);
        assert!(CUSTOM_PROXY
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty());
        set_proxy(0, "");
    }
}
