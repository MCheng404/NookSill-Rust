//! Device measurement for the gl device path.

use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetDC, HDC};
use windows::Win32::Graphics::OpenGL::{
    wglCreateContext, wglMakeCurrent, ChoosePixelFormat, SetPixelFormat, HGLRC, PFD_DRAW_TO_WINDOW,
    PFD_SUPPORT_OPENGL, PFD_TYPE_RGBA, PIXELFORMATDESCRIPTOR,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassW, CS_OWNDC, WINDOW_EX_STYLE, WNDCLASSW, WS_POPUP,
};

use super::{Guard, TEX_H, TEX_W};

const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_RGBA: u32 = 0x1908;
const GL_RGBA8: i32 = 0x8058;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_RENDERER: u32 = 0x1F01;
const GL_VERSION: u32 = 0x1F02;

// opengl32.dll only exports GL 1.1 directly; no extensions here.
#[link(name = "opengl32")]
unsafe extern "system" {
    fn glGetString(name: u32) -> *const u8;
    fn glGenTextures(n: i32, textures: *mut u32);
    fn glBindTexture(target: u32, texture: u32);
    fn glTexImage2D(
        target: u32,
        level: i32,
        internalformat: i32,
        width: i32,
        height: i32,
        border: i32,
        format: u32,
        ty: u32,
        pixels: *const std::ffi::c_void,
    );
}

pub struct Device {
    pub _hwnd: HWND,
    pub _hdc: HDC,
    pub _hglrc: HGLRC,
}

impl Device {
    pub fn allocate(&self, keep: &mut Guard) {
        let mut texture = 0u32;
        unsafe {
            glGenTextures(1, &mut texture);
            glBindTexture(GL_TEXTURE_2D, texture);
            // Null pixels: the driver allocates storage only. The real app
            // also creates the texture before drawing into it.
            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                GL_RGBA8,
                TEX_W as i32,
                TEX_H as i32,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                std::ptr::null(),
            );
        }
        keep.push(texture);
        // Readback target. The real path uses a PBO (needs a GL 1.5
        // extension); an equal-size heap buffer is equivalent for a
        // memory question.
        keep.push(vec![0u8; (TEX_W as usize) * (TEX_H as usize) * 4]);
    }
}

/// Window procedure. `DefWindowProcW` is a safe Rust wrapper in the
/// `windows` crate and cannot be used as a `WNDPROC` pointer directly, so
/// it needs this trampoline.
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

pub fn create() -> Result<(super::Device, String), Box<dyn std::error::Error>> {
    let instance = unsafe { GetModuleHandleW(None)? };
    let class = w!("NookSillProbeGL");
    let wc = WNDCLASSW {
        // `CS_OWNDC`: the pixel format binds to the window DC, kept private.
        style: CS_OWNDC,
        lpfnWndProc: Some(wndproc),
        hInstance: HINSTANCE(instance.0),
        lpszClassName: class,
        // An empty class cursor shows the busy cursor.
        hCursor: nooksill_gpu::cursor::class_cursor(),
        ..Default::default()
    };
    // Re-registering the same class name is not fatal; it happens when
    // several backends run in a row.
    unsafe { RegisterClassW(&wc) };

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("probe"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance.into()),
            None,
        )?
    };
    let hdc = unsafe { GetDC(Some(hwnd)) };

    let pfd = PIXELFORMATDESCRIPTOR {
        nSize: std::mem::size_of::<PIXELFORMATDESCRIPTOR>() as u16,
        nVersion: 1,
        dwFlags: PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL,
        iPixelType: PFD_TYPE_RGBA,
        cColorBits: 32,
        cAlphaBits: 8,
        ..Default::default()
    };
    let format = unsafe { ChoosePixelFormat(hdc, &pfd) };
    if format == 0 {
        return Err("ChoosePixelFormat 失败：没有可用的 GL 像素格式".into());
    }
    unsafe { SetPixelFormat(hdc, format, &pfd)? };

    let hglrc = unsafe { wglCreateContext(hdc)? };
    unsafe { wglMakeCurrent(hdc, hglrc)? };

    let desc = format!(
        "OpenGL / {} / GL {}",
        gl_string(GL_RENDERER).unwrap_or_else(|| "未知渲染器".to_owned()),
        gl_string(GL_VERSION).unwrap_or_else(|| "?".to_owned())
    );
    Ok((
        super::Device::Gl(Device {
            _hwnd: hwnd,
            _hdc: hdc,
            _hglrc: hglrc,
        }),
        desc,
    ))
}

/// Reads a GL string constant. Returns `None` when the context is not
/// current.
fn gl_string(name: u32) -> Option<String> {
    let ptr = unsafe { glGetString(name) };
    if ptr.is_null() {
        return None;
    }
    Some(
        unsafe { std::ffi::CStr::from_ptr(ptr.cast()) }
            .to_string_lossy()
            .into_owned(),
    )
}
