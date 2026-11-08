//! Icon extraction diagnostic: dump one path's icon as raw RGBA for external
//! inspection.
//!
//! Usage:
//!
//! ```text
//! cargo run -p nooksill-platform --example icon_dump -- <path> [output prefix]
//! ```
//!
//! Writes two files: `<prefix>.rgba` (row-major RGBA) and `<prefix>.txt` (size and
//! statistics). No PNG on purpose: the platform layer links no image library.

use nooksill_core::icon::{fit_icon, FitMode};
use nooksill_platform::icon::{resource_icon, shell_icon, IconBitmap};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("缺少路径参数")?;
    let prefix = args.next().unwrap_or_else(|| "icon".to_owned());

    println!("路径: {path}");

    let shell = shell_icon(&path);
    report("shell", shell.as_ref());

    let res = resource_icon(&path, 0);
    report("resource[0]", res.as_ref());

    // Prefer the shell icon (the dock's normal path), fall back to resources.
    let Some(bmp) = shell.or(res) else {
        println!("两种方式都没取到图标");
        return Ok(());
    };
    dump(&bmp, &prefix)?;
    println!("已写出 {prefix}.rgba / {prefix}.txt");

    // Fit result: decides whether the dock shows a full plate or a centered glyph.
    for (name, mode) in [
        ("自动", FitMode::Auto),
        ("完整", FitMode::Full),
        ("裁切", FitMode::Square),
    ] {
        // The mask changes pixels, so work on a clone.
        let mut work = bmp.clone();
        let fit = fit_icon(&work.rgba, work.width, work.height, mode);
        if let Some(r) = fit.rounded {
            nooksill_core::icon::apply_round_mask(&mut work.rgba, work.width, fit.bbox, r);
        }
        let visible = work
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] != 0)
            .count();
        println!(
            "  {name:<4} bbox=({},{})-({},{}) {}x{}  圆角={:?}  fill={:.2}  遮罩后非透明 {}",
            fit.bbox.x0,
            fit.bbox.y0,
            fit.bbox.x1,
            fit.bbox.y1,
            fit.bbox.width(),
            fit.bbox.height(),
            fit.rounded,
            fit.fill,
            visible
        );
    }
    Ok(())
}

fn report(how: &str, bmp: Option<&IconBitmap>) {
    match bmp {
        Some(b) => {
            let visible = b
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] != 0)
                .count();
            let total = b.rgba.len() / 4;
            println!(
                "  {how:<12} {}x{}  非透明 {visible}/{total}（{:.1}%）",
                b.width,
                b.height,
                visible as f32 * 100.0 / total as f32
            );
        }
        None => println!("  {how:<12} 未取到"),
    }
}

fn dump(b: &IconBitmap, prefix: &str) -> std::io::Result<()> {
    std::fs::write(format!("{prefix}.rgba"), &b.rgba)?;
    std::fs::write(
        format!("{prefix}.txt"),
        format!("{} {}\n", b.width, b.height),
    )?;
    Ok(())
}
