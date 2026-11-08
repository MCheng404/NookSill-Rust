//! Embeds `assets/nooksill.ico` and the version info into the resource table
//! of `nooksill-dock.exe`; that icon is what Explorer, the taskbar and Alt+Tab
//! show. The version comes from `scripts/version.rs`, the same source as
//! `nooksill_core::VERSION` and the artifact name.
//!
//! Windows only. A missing `rc.exe` (no Windows SDK) only warns: the icon is
//! cosmetic and must not break the build.

include!("../../scripts/version.rs");

fn main() {
    println!("cargo:rerun-if-env-changed=NOOKSILL_VERSION");
    println!("cargo:rerun-if-env-changed=NOOKSILL_BUILD");
    println!("cargo:rerun-if-changed=../../scripts/version.rs");
    println!("cargo:rerun-if-changed=../../.git/logs/HEAD");

    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("windows") {
        return;
    }

    let version = resolve(env!("CARGO_PKG_VERSION"));
    let manifest = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_owned()),
    );
    let icon = manifest.join("../../assets/nooksill.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    if !icon.exists() {
        println!(
            "cargo:warning=找不到 {}，exe 将没有应用图标",
            icon.display()
        );
        return;
    }

    let mut res = winresource::WindowsResource::new();
    res.set_icon(icon.to_str().unwrap_or("assets/nooksill.ico"));
    res.set("FileDescription", "NookSill");
    res.set("ProductName", "NookSill");
    res.set("CompanyName", "NookSill Rust port contributors");
    res.set("LegalCopyright", "GNU GPL v3.0 or Later");
    res.set("OriginalFilename", "nooksill-dock.exe");
    res.set("InternalName", "nooksill-dock");
    // The numeric version drives the sort order in the property sheet, the
    // string version the displayed text; both are needed, otherwise Explorer
    // swallows the dev suffix.
    res.set_version_info(winresource::VersionInfo::FILEVERSION, packed(&version));
    res.set_version_info(winresource::VersionInfo::PRODUCTVERSION, packed(&version));
    res.set("FileVersion", &version.text);
    res.set("ProductVersion", &version.text);

    // Only the main binary gets the icon; helper exes like probe do not.
    if let Err(e) = res.compile_for("nooksill-dock") {
        println!("cargo:warning=应用图标资源编译失败（不影响运行）：{e}");
    }
}
