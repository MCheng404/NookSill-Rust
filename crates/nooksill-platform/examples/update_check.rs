//! Manually verify the whole updater chain: fetch the channel's newest release,
//! pick a mirror, download and verify.
//!
//! ```text
//! cargo run -p nooksill-platform --example update_check             # stable, fetch only
//! cargo run -p nooksill-platform --example update_check -- beta     # beta channel
//! cargo run -p nooksill-platform --example update_check -- stage    # fetch, download, verify
//! ```
//!
//! This makes real network requests. `stage` downloads the whole asset next to
//! the exe as `NookSill.update.exe`; a checksum mismatch is an error and
//! nothing is overwritten.

use nooksill_platform::update::Channel;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let want_stage = args.iter().any(|a| a == "stage");
    let channel = if args.iter().any(|a| a == "beta") {
        Channel::Beta
    } else {
        Channel::Stable
    };
    let installed = nooksill_platform::update::Version::parse(nooksill_core::VERSION)
        .ok_or("当前版本号解析失败")?;
    println!("installed: {}", installed.text);

    let release = nooksill_platform::update::latest_release(channel)?;
    let checksum = release
        .sha256
        .as_ref()
        .map(|a| a.name.as_str())
        .unwrap_or("(API digest)");
    println!(
        "latest:    {}  exe={}  sha={}  prerelease={}",
        release.version.text, release.exe.name, checksum, release.prerelease
    );
    println!(
        "offered:   {}",
        if nooksill_platform::update::is_offered(&release, &installed, channel) {
            "yes"
        } else {
            "no"
        }
    );

    let target = &release.exe.url;
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
