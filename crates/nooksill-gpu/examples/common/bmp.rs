//! Shared BMP writer for the examples.
//!
//! Pulled in with `#[path = "common/bmp.rs"] mod bmp;`. Files under
//! `examples/common/` are not compiled as examples: cargo only picks up
//! `examples/*.rs` and `examples/*/main.rs`.

use std::io::Write;

/// Writes a 24-bit BMP, compositing premultiplied RGBA over a per-pixel
/// background: `(x, y)` returns that pixel's colour.
///
/// A solid colour is a constant closure; the frost shots pass a fake wallpaper.
pub fn write_bmp(
    path: &str,
    width: u32,
    height: u32,
    rgba: &[u8],
    bg: impl Fn(u32, u32) -> [u8; 3],
) -> std::io::Result<()> {
    let row_stride = ((width as usize) * 3).div_ceil(4) * 4;
    let data_len = row_stride * height as usize;
    let mut buf = Vec::with_capacity(54 + data_len);

    // BITMAPFILEHEADER
    buf.extend_from_slice(b"BM");
    buf.extend_from_slice(&(54 + data_len as u32).to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&54u32.to_le_bytes());
    // BITMAPINFOHEADER
    buf.extend_from_slice(&40u32.to_le_bytes());
    buf.extend_from_slice(&(width as i32).to_le_bytes());
    buf.extend_from_slice(&(height as i32).to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&24u16.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes()); // no compression
    buf.extend_from_slice(&(data_len as u32).to_le_bytes());
    buf.extend_from_slice(&2835i32.to_le_bytes()); // 2835 px/m == 72 DPI
    buf.extend_from_slice(&2835i32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());

    // BMP rows are stored bottom-up.
    for y in (0..height).rev() {
        let mut written = 0;
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            let (r, g, b, a) = (rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]);
            let base = bg(x, y);
            // Premultiplied alpha: add the background's 1 - a share.
            let inv = 255 - a as u32;
            let blend =
                |c: u8, back: u8| -> u8 { (c as u32 + back as u32 * inv / 255).min(255) as u8 };
            buf.push(blend(b, base[2]));
            buf.push(blend(g, base[1]));
            buf.push(blend(r, base[0]));
            written += 3;
        }
        // Rows are padded to a 4-byte boundary.
        buf.extend(std::iter::repeat_n(0u8, row_stride - written));
    }

    let mut f = std::fs::File::create(path)?;
    f.write_all(&buf)
}
