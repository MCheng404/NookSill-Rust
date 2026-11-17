//! Test-only environment gates: hardware tests run where hardware exists and
//! quietly return on headless CI instead of being marked `#[ignore]`.

/// A D3D12 device is available; this also covers software adapters.
#[must_use]
pub fn gpu() -> bool {
    crate::gpu::Gpu::new("").is_ok()
}

/// A device plus an interactive desktop session (no `CI` environment flag).
#[must_use]
pub fn desktop() -> bool {
    gpu() && std::env::var_os("CI").is_none()
}
