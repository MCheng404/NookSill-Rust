//! Manually verify the whole updater chain: fetch latest, pick a mirror, download,
//! verify.
//!
//! ```text
//! cargo run -p nooksill-platform --example update_check           # fetch only
//! cargo run -p nooksill-platform --example update_check -- stage  # fetch, download, verify
//! ```
//!
//! This makes real network requests. `stage` downloads the whole asset next to the
//! exe as `NookSill.update.exe`; a checksum mismatch is an error and nothing is
//! overwritten.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let want_stage = std::env::args().nth(1).as_deref() == Some("stage");
    let installed = nooksill_platform::update::Version::parse(nooksill_core::VERSION)
        .ok_or("当前版本号解析失败")?;
    println!("installed: {}", installed.text);

    let release = nooksill_platform::update::latest_release()?;
    println!(
        "latest:    {}  exe={}  sha={}  prerelease={}",
        release.version.text, release.exe.name, release.sha256.name, release.prerelease
    );
    println!(
        "offered:   {}",
        if nooksill_platform::update::is_offered(&release, &installed) {
            "yes"
        } else {
            "no"
        }
    );

    let target = &release.sha256.url;
    let base = nooksill_platform::update::pick_mirror(target, false, true)
        .ok_or("没有可用镜像（连 GitHub 直连都失败）")?;
    println!(
        "mirror:    {}",
        if base.is_empty() { "(direct)" } else { &base }
    );

    if want_stage {
        let path = nooksill_platform::update::stage(&base, &release)?;
        let size = std::fs::metadata(&path)?.len();
        println!("staged:    {} ({} bytes)", path.display(), size);
    }
    Ok(())
}
