//! Test-only gates for tests that query live OS state or spawn processes.

/// Interactive desktop session: skip live taskbar, shell and power queries on
/// headless CI. Pure functions still run everywhere.
#[must_use]
pub fn interactive() -> bool {
    std::env::var_os("CI").is_none()
}

/// Windows 11 24H2 puts the WLAN current-connection query behind precise
/// location consent, which raises a per-executable system dialog and blocks
/// the run; cargo renames the test binary, so it returns after every rebuild.
#[must_use]
pub fn consent_probes() -> bool {
    std::env::var_os("NOOKSILL_CONSENT_PROBES").is_some()
}
