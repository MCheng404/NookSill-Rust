//! Test-only gates for tests that query live OS state or spawn processes.

/// Interactive desktop session: skip live taskbar, shell and power queries on
/// headless CI. Pure functions still run everywhere.
#[must_use]
pub fn interactive() -> bool {
    std::env::var_os("CI").is_none()
}
