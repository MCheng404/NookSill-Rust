//! Stamps the build version so `nooksill_core::VERSION` matches the EXE
//! resource and the artifact name. See `scripts/version.rs` for the rules.

include!("../../scripts/version.rs");

fn main() {
    println!("cargo:rerun-if-env-changed=NOOKSILL_VERSION");
    println!("cargo:rerun-if-env-changed=NOOKSILL_BUILD");
    println!("cargo:rerun-if-changed=../../scripts/version.rs");
    // Commits move the number; watching the reflog is cheaper than a full scan.
    println!("cargo:rerun-if-changed=../../.git/logs/HEAD");

    let v = resolve(env!("CARGO_PKG_VERSION"));
    println!("cargo:rustc-env=NOOKSILL_VERSION={}", v.text);

    // Hand the resolved value to `scripts/package.sh` instead of re-deriving it
    // in shell: the artifact name must be the version actually stamped into the
    // binary. OUT_DIR gained a directory level in rust 1.100
    // (`build/<pkg>/<hash>/out`), so its depth cannot locate the profile dir.
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = std::env::var("TARGET").unwrap_or_default();
    let profile = std::env::var("PROFILE").unwrap_or_default();
    let path = root
        .join("target")
        .join(target)
        .join(profile)
        .join("NOOKSILL_VERSION");
    let _ = std::fs::write(path, format!("{}\n", v.text));
}
