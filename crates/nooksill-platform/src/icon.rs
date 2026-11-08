//! HICON to RGBA: `GetDIBits` color bitmap, mask-derived alpha for 24-bit
//! icons, then `DrawIconEx` into a DIB. JUMBO (256) precedes EXTRALARGE (48).
//! GDI+ is avoided: `GdipCreateBitmapFromHICON` leaves a black background.

use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, GetDC, GetDIBits,
    GetObjectW, ReleaseDC, SelectObject, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
    HBITMAP, HGDIOBJ,
};
use windows::Win32::UI::Controls::{IImageList, ILD_TRANSPARENT};
use windows::Win32::UI::Shell::{
    SHGetFileInfoW, SHGetImageList, SHFILEINFOW, SHGFI_DISPLAYNAME, SHGFI_SYSICONINDEX,
    SHIL_EXTRALARGE, SHIL_JUMBO,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, DrawIconEx, GetIconInfo, DI_NORMAL, HICON, ICONINFO,
};

/// Straight-alpha bitmap; `rgba` is row-major with 4 bytes per pixel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconBitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl IconBitmap {
    /// Convenience accessor for one alpha value (diagnostics and tests).
    #[must_use]
    pub fn alpha_at(&self, x: u32, y: u32) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.rgba[((y * self.width + x) * 4 + 3) as usize]
    }

    /// Whether any pixel is non-transparent. A fully transparent image means no
    /// icon was obtained.
    #[must_use]
    pub fn has_visible_pixels(&self) -> bool {
        self.rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 0)
    }
}

/// Explorer display name of a file (`SHGFI_DISPLAYNAME`), used to name running
/// apps that were never pinned: `chrome.exe` reports "Google Chrome", which
/// beats the bare file name and needs no version-resource parsing.
///
/// `None` when unavailable; callers fall back to the file name.
#[must_use]
pub fn display_name(path: &str) -> Option<String> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut sfi = SHFILEINFOW::default();
    // SAFETY: the output struct lives on this frame and the size comes from it.
    let ok = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            Default::default(),
            Some(&raw mut sfi),
            u32::try_from(std::mem::size_of::<SHFILEINFOW>()).ok()?,
            SHGFI_DISPLAYNAME,
        )
    };
    if ok == 0 {
        return None;
    }
    let len = sfi
        .szDisplayName
        .iter()
        .position(|c| *c == 0)
        .unwrap_or(sfi.szDisplayName.len());
    if len == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&sfi.szDisplayName[..len]))
}

/// Shell icon, trying JUMBO (256 px) first and EXTRALARGE (48 px) after.
///
/// `path` may be an exe, folder, `.lnk` or URL: anything Explorer has an icon for.
#[must_use]
pub fn shell_icon(path: &str) -> Option<IconBitmap> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    let trace = std::env::var_os("NOOKSILL_ICON_TRACE").is_some();
    let mut sfi = SHFILEINFOW::default();
    let ok = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            Default::default(),
            Some(&raw mut sfi),
            u32::try_from(std::mem::size_of::<SHFILEINFOW>()).ok()?,
            SHGFI_SYSICONINDEX,
        )
    };
    if ok == 0 {
        if trace {
            eprintln!("  [icon] SHGetFileInfoW 失败: {path}");
        }
        return None;
    }
    if trace {
        eprintln!("  [icon] iIcon={} attrs={:#x}", sfi.iIcon, sfi.dwAttributes);
    }

    // Both sizes are tried: JUMBO fails on some older systems, EXTRALARGE
    // almost always works.
    for list in [SHIL_JUMBO, SHIL_EXTRALARGE] {
        let il: windows::core::Result<IImageList> =
            unsafe { SHGetImageList(i32::try_from(list).ok()?) };
        let il = match il {
            Ok(v) => v,
            Err(e) => {
                if trace {
                    eprintln!("  [icon] SHGetImageList({list}) 失败: {e}");
                }
                continue;
            }
        };
        let hicon = match unsafe { il.GetIcon(sfi.iIcon, ILD_TRANSPARENT.0) } {
            Ok(v) => v,
            Err(e) => {
                if trace {
                    eprintln!("  [icon] GetIcon({}, list={list}) 失败: {e}", sfi.iIcon);
                }
                continue;
            }
        };
        let bmp = hicon_to_bitmap(hicon);
        let _ = unsafe { DestroyIcon(hicon) };
        match bmp {
            Some(b) if b.has_visible_pixels() => {
                if trace {
                    eprintln!("  [icon] 取自 list={list}: {}x{}", b.width, b.height);
                }
                return Some(b);
            }
            Some(b) => {
                if trace {
                    eprintln!(
                        "  [icon] list={list} 取到 {}x{} 但全透明",
                        b.width, b.height
                    );
                }
            }
            None => {
                if trace {
                    eprintln!("  [icon] list={list} 的 HICON 转位图失败");
                }
            }
        }
    }
    None
}

/// Extract icon `index` from the resources of an exe, DLL or `.ico`.
///
/// Shortcuts store `iconLoc` + `iconIdx` that often point into an exe's
/// resources, which the shell image list does not cover.
#[must_use]
pub fn resource_icon(file: &str, index: i32) -> Option<IconBitmap> {
    use windows::Win32::UI::WindowsAndMessaging::PrivateExtractIconsW;

    let wide: Vec<u16> = file.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = [0u16; 260];
    let n = wide.len().min(buf.len());
    buf[..n].copy_from_slice(&wide[..n]);
    if n == buf.len() {
        // A truncated path is a guess; return nothing instead.
        return None;
    }

    // Largest size first; 256 matches jumbo, smaller ones follow.
    for size in [256, 128, 64, 48] {
        let mut hicon = HICON::default();
        let got = unsafe {
            PrivateExtractIconsW(
                &buf,
                index,
                size,
                size,
                Some(std::slice::from_mut(&mut hicon)),
                None,
                0,
            )
        };
        if got != 1 || hicon.is_invalid() {
            continue;
        }
        let bmp = hicon_to_bitmap(hicon);
        let _ = unsafe { DestroyIcon(hicon) };
        // Smaller sizes still rescue old exes that ship only 48 px icons.
        if let Some(b) = bmp {
            if b.has_visible_pixels() {
                return Some(b);
            }
        }
    }
    None
}

/// Convert a HICON to RGBA; the fallback order is described in the module docs.
#[must_use]
pub fn hicon_to_bitmap(hicon: HICON) -> Option<IconBitmap> {
    if hicon.is_invalid() {
        return None;
    }
    let mut ii = ICONINFO::default();
    if unsafe { GetIconInfo(hicon, &raw mut ii) }.is_err() {
        return None;
    }

    let color = if !ii.hbmColor.is_invalid() {
        from_color_bitmap(ii.hbmColor, ii.hbmMask)
    } else {
        None
    };

    // Free the two bitmaps that GetIconInfo duplicated.
    unsafe {
        if !ii.hbmColor.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
        }
        if !ii.hbmMask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
        }
    }

    color.or_else(|| draw_into_dib(hicon))
}

/// Layer one: color bitmap plus, when needed, alpha rebuilt from the mask.
fn from_color_bitmap(hbm_color: HBITMAP, hbm_mask: HBITMAP) -> Option<IconBitmap> {
    let mut bm = BITMAP::default();
    let got = unsafe {
        GetObjectW(
            HGDIOBJ(hbm_color.0),
            i32::try_from(std::mem::size_of::<BITMAP>()).ok()?,
            Some((&raw mut bm).cast()),
        )
    };
    if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
        return None;
    }
    let w = u32::try_from(bm.bmWidth).ok()?;
    let h = u32::try_from(bm.bmHeight).ok()?;
    let pixels = usize::try_from(w).ok()? * usize::try_from(h).ok()?;

    let mut bgra = read_dib(hbm_color, w, h)?;

    // All-zero alpha means a legacy 24-bit icon (or mask-based alpha). Fill it
    // from the mask: 0 means draw, non-zero means transparent.
    if !bgra.as_chunks::<4>().0.iter().any(|p| p[3] != 0) && !hbm_mask.is_invalid() {
        let mask = read_dib(hbm_mask, w, h)?;
        for i in 0..pixels {
            bgra[i * 4 + 3] = if mask[i * 4] == 0 { 255 } else { 0 };
        }
    }

    let rgba = bgra_to_rgba(&bgra);
    Some(IconBitmap {
        width: w,
        height: h,
        rgba,
    })
}

/// BGRA to RGBA; fully transparent pixels are zeroed.
fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    let mut rgba = vec![0u8; bgra.len()];
    // Ignore the trailing remainder (callers always pass a multiple of 4).
    for (src, dst) in bgra
        .as_chunks::<4>()
        .0
        .iter()
        .zip(rgba.as_chunks_mut::<4>().0)
    {
        if src[3] == 0 {
            continue;
        }
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }
    rgba
}

/// Read a 32-bit bitmap's pixels as BGRA (GDI's native order).
fn read_dib(hbm: HBITMAP, w: u32, h: u32) -> Option<Vec<u8>> {
    let pixels = usize::try_from(w).ok()? * usize::try_from(h).ok()?;
    let mut buf = vec![0u8; pixels * 4];

    let mut bi = BITMAPINFO::default();
    bi.bmiHeader.biSize = u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>()).ok()?;
    bi.bmiHeader.biWidth = i32::try_from(w).ok()?;
    // A negative height means top-down, saving one flip.
    bi.bmiHeader.biHeight = -i32::try_from(h).ok()?;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = 0; // BI_RGB

    let dc = unsafe { GetDC(None) };
    if dc.is_invalid() {
        return None;
    }
    let lines = unsafe {
        GetDIBits(
            dc,
            hbm,
            0,
            h,
            Some(buf.as_mut_ptr().cast()),
            &raw mut bi,
            DIB_RGB_COLORS,
        )
    };
    unsafe { ReleaseDC(None, dc) };
    if lines == 0 {
        return None;
    }
    Some(buf)
}

/// Fallback: draw the icon into a 32-bit DIB with `DrawIconEx`.
/// The DIB must be selected into a memory DC before drawing, and `GdiFlush`
/// must run before reading `bits` (Microsoft Learn, CreateDIBSection Remarks).
fn draw_into_dib(hicon: HICON) -> Option<IconBitmap> {
    // Resolve every fallible computation before creating GDI objects; an early
    // return past that point would leak the DIB handle.
    let size = icon_size(hicon).unwrap_or(256);
    let side = i32::try_from(size).ok()?;
    let pixels = usize::try_from(size).ok()? * usize::try_from(size).ok()?;

    let mut bi = BITMAPINFO::default();
    bi.bmiHeader.biSize = u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>()).ok()?;
    bi.bmiHeader.biWidth = side;
    bi.bmiHeader.biHeight = -side;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = 0;

    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let dib =
        unsafe { CreateDIBSection(None, &raw const bi, DIB_RGB_COLORS, &raw mut bits, None, 0) }
            .ok()?;
    if bits.is_null() {
        let _ = unsafe { DeleteObject(HGDIOBJ(dib.0)) };
        return None;
    }

    // Memory DC compatible with the screen; the DIB is the render target.
    let screen = unsafe { GetDC(None) };
    if screen.0.is_null() {
        let _ = unsafe { DeleteObject(HGDIOBJ(dib.0)) };
        return None;
    }
    let dc = unsafe { CreateCompatibleDC(Some(screen)) };
    unsafe { ReleaseDC(None, screen) };
    if dc.0.is_null() {
        let _ = unsafe { DeleteObject(HGDIOBJ(dib.0)) };
        return None;
    }

    let old = unsafe { SelectObject(dc, HGDIOBJ(dib.0)) };
    let drawn = unsafe { DrawIconEx(dc, 0, 0, hicon, side, side, 0, None, DI_NORMAL) };
    unsafe {
        // `bits` is shared with GDI; flush before reading it.
        let _ = GdiFlush();
        SelectObject(dc, old); // restore before deleting the DIB
        let _ = DeleteDC(dc);
    }
    if drawn.is_err() {
        let _ = unsafe { DeleteObject(HGDIOBJ(dib.0)) };
        return None;
    }

    let raw = unsafe { std::slice::from_raw_parts(bits.cast::<u8>(), pixels * 4) };
    let rgba = bgra_to_rgba(raw);
    let _ = unsafe { DeleteObject(HGDIOBJ(dib.0)) };

    let bmp = IconBitmap {
        width: size,
        height: size,
        rgba,
    };
    bmp.has_visible_pixels().then_some(bmp)
}

/// The icon's own size, which is the size of `GetIconInfo`'s color bitmap.
fn icon_size(hicon: HICON) -> Option<u32> {
    let mut ii = ICONINFO::default();
    unsafe { GetIconInfo(hicon, &raw mut ii) }.ok()?;
    let mut bm = BITMAP::default();
    let got = unsafe {
        GetObjectW(
            HGDIOBJ(ii.hbmColor.0),
            i32::try_from(std::mem::size_of::<BITMAP>()).ok()?,
            Some((&raw mut bm).cast()),
        )
    };
    unsafe {
        if !ii.hbmColor.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
        }
        if !ii.hbmMask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
        }
    }
    if got == 0 || bm.bmWidth <= 0 {
        return None;
    }
    u32::try_from(bm.bmWidth).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{LoadIconW, IDI_APPLICATION};

    #[test]
    fn bgra_conversion_swaps_channels_and_clears_transparent_rgb() {
        let bgra = [
            10, 20, 30, 255, // opaque
            40, 50, 60, 0, // alpha 0 -> zeroed
            1, 2, 3, 128, // alpha preserved
        ];
        assert_eq!(
            bgra_to_rgba(&bgra),
            vec![30, 20, 10, 255, 0, 0, 0, 0, 3, 2, 1, 128]
        );
    }

    #[test]
    fn bgra_conversion_tolerates_empty_and_ragged_input() {
        assert!(bgra_to_rgba(&[]).is_empty());
        // A trailing partial pixel is dropped, not read out of bounds.
        assert_eq!(bgra_to_rgba(&[1, 2, 3, 4, 9, 9]), vec![3, 2, 1, 4, 0, 0]);
    }

    /// Real HICON must produce visible pixels in the DIB.
    /// Run: `cargo test -p nooksill-platform --lib icon -- --ignored`
    #[test]
    #[ignore = "需要桌面会话，显式 --ignored 运行"]
    fn dib_fallback_draws_into_the_dib_not_the_screen() {
        // Shared system icon; do not call DestroyIcon.
        let hicon = unsafe { LoadIconW(None, IDI_APPLICATION) }.expect("加载系统图标");
        let bmp = draw_into_dib(hicon).expect("DIB 里应当有可见像素");
        assert!(bmp.has_visible_pixels());
        assert_eq!(bmp.width, bmp.height);
    }
}
