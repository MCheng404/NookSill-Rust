//! PNG / JPEG / BMP / GIF custom icons through WIC.
//!
//! `icon::shell_icon` answers "which icon does Explorer show for this file",
//! which for an image is the generic picture glyph rather than the pixels. A
//! custom icon needs decoding, and WIC ships with Windows, so no image crate is
//! linked (the project deliberately keeps the dependency tree small).
//!
//! `.ico` is not handled here: the shell image list already picks the best size
//! out of a multi-image icon, which WIC's first frame would not.

use std::path::Path;

use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::{GENERIC_READ, S_FALSE};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICBitmapSource, IWICFormatConverter,
    IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom,
    WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};

use crate::icon::{bgra_to_rgba, IconBitmap};

/// Extensions WIC decodes in-box. `webp` needs a Store codec on some systems;
/// decoding simply fails there and the caller falls back to the shell icon.
const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp"];

/// Refuse absurd images before allocating: 16 M pixels is 64 MB of BGRA.
const MAX_PIXELS: u64 = 64 * 1024 * 1024 / 4;

/// Whether `path` is a still image this module can decode.
#[must_use]
pub fn is_supported(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            let lower = e.to_ascii_lowercase();
            EXTENSIONS.contains(&lower.as_str())
        })
}

/// Decode a still image into straight-alpha RGBA. `None` for anything WIC
/// cannot read, including a missing file.
#[must_use]
pub fn decode_image_icon(path: &str) -> Option<IconBitmap> {
    if !is_supported(path) {
        return None;
    }
    decode(path).filter(IconBitmap::has_visible_pixels)
}

fn decode(path: &str) -> Option<IconBitmap> {
    let _com = Com::init()?;
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let factory: IWICImagingFactory = unsafe {
        CoCreateInstance(
            &CLSID_WICImagingFactory,
            None::<&windows::core::IUnknown>,
            CLSCTX_INPROC_SERVER,
        )
    }
    .ok()?;
    let decoder = unsafe {
        factory.CreateDecoderFromFilename(
            PCWSTR(wide.as_ptr()),
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )
    }
    .ok()?;
    let frame = unsafe { decoder.GetFrame(0) }.ok()?;
    let converter: IWICFormatConverter = unsafe { factory.CreateFormatConverter() }.ok()?;
    unsafe {
        converter.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppBGRA,
            WICBitmapDitherTypeNone,
            None,
            // Alpha threshold only applies to palette formats.
            0.0,
            WICBitmapPaletteTypeCustom,
        )
    }
    .ok()?;

    let source: IWICBitmapSource = converter.cast().ok()?;
    let mut width = 0u32;
    let mut height = 0u32;
    unsafe { source.GetSize(&mut width, &mut height) }.ok()?;
    let pixels = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || pixels > MAX_PIXELS {
        return None;
    }
    let mut buf = vec![0u8; usize::try_from(pixels).ok()? * 4];
    let stride = width.checked_mul(4)?;
    unsafe { source.CopyPixels(std::ptr::null(), stride, &mut buf) }.ok()?;
    Some(IconBitmap {
        width,
        height,
        rgba: bgra_to_rgba(&buf),
    })
}

/// RAII COM apartment for the icon worker thread.
struct Com(bool);

impl Com {
    fn init() -> Option<Self> {
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr.is_err() {
            return None;
        }
        // S_FALSE means the thread was already initialised; only a fresh S_OK
        // may be balanced by CoUninitialize.
        Some(Self(hr != S_FALSE))
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1x1 opaque red PNG, generated once and pinned here.
    const RED_DOT_PNG: [u8; 70] = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xf8,
        0xcf, 0xc0, 0xf0, 0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x56, 0xc7, 0x2f, 0x0d, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn only_image_extensions_are_accepted() {
        assert!(is_supported("C:/icons/a.png"));
        assert!(is_supported(r"C:\icons\A.PNG"), "扩展名大小写不敏感");
        assert!(is_supported("a.jpeg"));
        assert!(!is_supported("a.ico"), "ico 走 shell 图像列表");
        assert!(!is_supported("a.exe"));
        assert!(!is_supported("noext"));
        assert!(!is_supported(""));
    }

    /// WIC is an in-box Windows component, so this runs on any desktop session
    /// (CI included) without a display.
    #[test]
    fn decodes_a_real_png_through_wic() {
        let path = std::env::temp_dir().join("nooksill-icon-decode-test.png");
        std::fs::write(&path, RED_DOT_PNG).expect("写入临时 PNG");
        let bitmap =
            decode_image_icon(path.to_str().expect("临时路径是 UTF-8")).expect("WIC 应能解码 PNG");
        assert_eq!((bitmap.width, bitmap.height), (1, 1));
        assert_eq!(bitmap.rgba, vec![255, 0, 0, 255]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_files_and_junk_decode_to_none() {
        assert!(decode_image_icon("C:/definitely/not/here.png").is_none());
        let path = std::env::temp_dir().join("nooksill-icon-junk.png");
        std::fs::write(&path, b"not a png").expect("写入垃圾文件");
        assert!(decode_image_icon(path.to_str().expect("路径是 UTF-8")).is_none());
        let _ = std::fs::remove_file(&path);
    }
}
