//! Device memory probe.
//!
//! Answers one question: how much memory does the process pay for creating a
//! single graphics device (no window, no egui, no frame)? That number bounds
//! the "lightweight" claim of the GPU route and how much a backend swap saves.
//!
//! ```text
//! PROBE_BACKEND=d3d11 PROBE_ALLOC=1 cargo run --release -p nooksill-gpu --bin probe
//! PROBE_LIST=1  target/release/probe.exe     # list the adapters on this machine
//! ```
//!
//! | Variable | Values | Meaning |
//! |---|---|---|
//! | `PROBE_BACKEND` | `dx12` (default) / `vulkan` / `wgl` / `d3d11` / `d3d11-warp` / `gl` | graphics backend |
//! | `PROBE_ADAPTER` | name fragment, e.g. `AMD` / `NVIDIA` | pins one GPU; empty = backend default |
//! | `PROBE_ALLOC` | `1` / `0` | also allocate the same buffers as the real app |
//! | `PROBE_HOLD` | seconds, default 45 | sampling window |
//! | `PROBE_LIST` | `1` | list the adapters and exit |
//! | `PROBE_NON_SAMPLER` | integer, default `1000000` | `max_non_sampler_bindings`, DX12 only |
//! | `PROBE_HINTS` | `perf` (default) / `memory` | `MemoryHints`: whether the allocator keeps headroom |
//! | `PROBE_LIMITS` | `downlevel` (default) / `default` / `wide` / `webgl2` | limit tier |
//!
//! `wgl` uses wgpu's own GL backend (`wgpu-hal/gles/wgl.rs`, no ANGLE needed on
//! Windows) and answers "what would keeping egui-wgpu but swapping the backend
//! save?". `gl` is a hand-written WGL context, the lower bound for dropping
//! wgpu entirely.
//!
//! `PROBE_ALLOC=1` allocates exactly what `offscreen.rs` + `present.rs` use: a
//! 1320x1080 render target (8.4 MB), a same-size readback buffer (5.4 MB) and a
//! same-size DIB (5.4 MB). That makes "device + buffers" the real memory floor.
//!
//! Pin the adapter before comparing backends: on a hybrid laptop wgpu and D3D11
//! can land on different GPUs, and part of the gap is then "a different chip"
//! rather than a backend difference.
//!
//! `d3d11-warp` is the pure software rasteriser: the minimum cost with no GPU
//! acceleration, and the lower bound for the D3D11 baseline.

use std::time::Duration;

use nooksill_gpu::mem::{Delta, MemStats};
use nooksill_gpu::present::LayeredPresenter;

/// Render target size in physical pixels, matching the spike's client area
/// (880x720 logical at 150%).
const TEX_W: u32 = 1320;
const TEX_H: u32 = 1080;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if env_or("PROBE_LIST", "0") == "1" {
        list_adapters();
        return Ok(());
    }

    let backend = env_or("PROBE_BACKEND", "dx12");
    let want = env_or("PROBE_ADAPTER", "");
    let alloc = env_or("PROBE_ALLOC", "0") == "1";
    let hold: u64 = env_or("PROBE_HOLD", "45").parse().unwrap_or(45);

    println!(
        "[probe] 后端={backend} 限定适配器={} 附加缓冲={} 采样窗口={hold}s",
        if want.is_empty() {
            "<后端默认>"
        } else {
            &want
        },
        if alloc { "是" } else { "否" }
    );

    let start = MemStats::now();
    println!("[probe] ① 进程启动   {}", start.summary());

    let (device, desc) = create_device(&backend, &want)?;
    let after_device = MemStats::now();
    println!("[probe] ② 设备就绪   {desc}");
    println!(
        "[probe]    设备开销   {}",
        Delta::between(start, after_device).summary()
    );

    // Must stay alive until the process exits: releasing early measures nothing.
    let mut keep = Guard::default();
    if alloc {
        let before = MemStats::now();
        keep = allocate(&device)?;
        // Drivers commit memory asynchronously; wait before sampling so the
        // delta is not attributed to the next step.
        std::thread::sleep(Duration::from_millis(1500));
        let after = MemStats::now();
        println!("[probe] ③ 缓冲就绪   {}", after.summary());
        println!(
            "[probe]    缓冲开销   {}",
            Delta::between(before, after).summary()
        );
    }

    let steady = MemStats::now();
    println!("[probe] ④ 稳态       {}", steady.summary());
    // One machine-readable line for scripts and docs; do not insert other
    // fields in the middle.
    println!(
        "[probe] RESULT backend={backend} alloc={} ws_mb={:.1} private_mb={:.1} peak_ws_mb={:.1}",
        u8::from(alloc),
        steady.ws_mb(),
        steady.private_mb(),
        steady.peak_ws_mb()
    );

    // Reprint every 5 s during the window: if the numbers keep climbing the
    // driver is still committing and a single sample would understate it. The
    // end-of-window peak is what decides whether it explodes.
    let mut left = hold;
    while left > 0 {
        let step = left.min(5);
        std::thread::sleep(Duration::from_secs(step));
        left -= step;
        println!(
            "[probe]    t+{}s -> {}",
            hold - left,
            MemStats::now().summary()
        );
    }

    println!(
        "[probe] ⑤ 结束       峰值 {:.1} MB",
        MemStats::now().peak_ws_mb()
    );
    drop(keep);
    Ok(())
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}

/// Lists the adapters both APIs see. Check this first on a hybrid GPU machine,
/// then decide which one to pin.
fn list_adapters() {
    let instance = wgpu_instance(egui_wgpu::wgpu::Backends::all());
    println!("[adapter] —— wgpu ——");
    for adapter in pollster::block_on(instance.enumerate_adapters(egui_wgpu::wgpu::Backends::all()))
    {
        let info = adapter.get_info();
        println!(
            "[adapter] wgpu {:?} | {} | VEN {:04X} DEV {:04X} | {:?} | driver {}",
            info.backend, info.name, info.vendor, info.device, info.device_type, info.driver
        );
    }
    println!("[adapter] —— DXGI（D3D11 用）——");
    match d3d11_device::adapters() {
        Ok(rows) => {
            for row in &rows {
                println!(
                    "[adapter] dxgi #{} | {} | VEN {:04X} DEV {:04X} | 专用显存 {} MB",
                    row.index, row.name, row.vendor, row.device_id, row.dedicated_mb
                );
            }
        }
        Err(e) => println!("[adapter] DXGI 枚举失败: {e}"),
    }
}

/// Device plus its queue or context. The backend kinds are stored separately
/// because their handle types have nothing in common.
enum Device {
    Wgpu(wgpu_device::Device),
    D3d11(d3d11_device::Device),
    Gl(gl_device::Device),
}

/// Keeps resources alive. `Box<dyn Any>` because the only purpose is to stop
/// them from being dropped.
#[derive(Default)]
struct Guard(Vec<Box<dyn std::any::Any>>);

impl Guard {
    fn push<T: 'static>(&mut self, value: T) {
        self.0.push(Box::new(value));
    }
}

fn create_device(
    backend: &str,
    want: &str,
) -> Result<(Device, String), Box<dyn std::error::Error>> {
    match backend {
        "dx12" | "vulkan" | "wgl" => wgpu_device::create(backend, want),
        "d3d11" | "d3d11-warp" => d3d11_device::create(backend == "d3d11-warp", want),
        "gl" => gl_device::create(),
        other => {
            Err(format!("未知后端 `{other}`，可选 dx12 / vulkan / d3d11 / d3d11-warp / gl").into())
        }
    }
}

fn allocate(device: &Device) -> Result<Guard, Box<dyn std::error::Error>> {
    let mut keep = Guard::default();
    match device {
        Device::Wgpu(d) => d.allocate(&mut keep),
        Device::D3d11(d) => d.allocate(&mut keep)?,
        Device::Gl(d) => d.allocate(&mut keep),
    }
    // The DIB of the present path: reuses the real code so the probe does not
    // measure something the app never allocates.
    keep.push(LayeredPresenter::new(TEX_W as i32, TEX_H as i32)?);
    Ok(keep)
}

fn wgpu_instance(backends: egui_wgpu::wgpu::Backends) -> egui_wgpu::wgpu::Instance {
    egui_wgpu::wgpu::Instance::new(egui_wgpu::wgpu::InstanceDescriptor {
        backends,
        flags: egui_wgpu::wgpu::InstanceFlags::default(),
        memory_budget_thresholds: egui_wgpu::wgpu::MemoryBudgetThresholds::default(),
        backend_options: egui_wgpu::wgpu::BackendOptions::default(),
        display: None,
    })
}

/// wgpu side: `dx12` and `vulkan` share the creation logic, only the backend
/// bit differs.
mod wgpu_device {
    use egui_wgpu::wgpu;

    use super::{Guard, TEX_H, TEX_W};

    pub struct Device {
        pub device: wgpu::Device,
        /// Keep the queue alive; dropping it lets the driver reclaim memory.
        #[expect(dead_code, reason = "queue must outlive the measurement")]
        pub queue: wgpu::Queue,
    }

    impl Device {
        pub fn allocate(&self, keep: &mut Guard) {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("probe-rt"),
                size: wgpu::Extent3d {
                    width: TEX_W,
                    height: TEX_H,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // Kept in sync with `Gpu::COLOR_FORMAT`; the pipeline depends on it.
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            keep.push(texture);

            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("probe-readback"),
                size: u64::from(TEX_W) * u64::from(TEX_H) * 4,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            keep.push(buffer);
        }
    }

    pub fn create(
        which: &str,
        want: &str,
    ) -> Result<(super::Device, String), Box<dyn std::error::Error>> {
        let backends = match which {
            "vulkan" => wgpu::Backends::VULKAN,
            // wgpu's own WGL implementation (wgpu-hal/gles/wgl.rs); no ANGLE
            // needed on Windows.
            "wgl" => wgpu::Backends::GL,
            _ => wgpu::Backends::DX12,
        };
        let instance = super::wgpu_instance(backends);

        // Pinning an adapter cannot go through `request_adapter`: the driver
        // owns that choice and would pick the iGPU on a hybrid machine, a
        // different chip than the D3D11 default.
        let adapter = if want.is_empty() {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            }))?
        } else {
            let needle = want.to_lowercase();
            let all = pollster::block_on(instance.enumerate_adapters(backends));
            let names: Vec<String> = all.iter().map(|a| a.get_info().name).collect();
            all.into_iter()
                .find(|a| {
                    let info = a.get_info();
                    // Match by name; `driver` is the fallback because some
                    // backends only put the vendor name in the driver string.
                    info.name.to_lowercase().contains(&needle)
                        || info.driver.to_lowercase().contains(&needle)
                })
                .ok_or_else(|| {
                    format!(
                        "没有匹配 `{want}` 的 wgpu 适配器，可选: {}",
                        names.join(" / ")
                    )
                })?
        };

        let info = adapter.get_info();
        // The two knobs wgpu documents as memory hungry:
        // - `max_non_sampler_bindings`: the DX12 backend preallocates a global
        //   descriptor heap of that size, 1_000_000 by default, which eats
        //   system memory on an iGPU (wgpu's own wording).
        // - `memory_hints`: the default Performance hint makes the allocator
        //   keep more headroom.
        let non_sampler: u32 = std::env::var("PROBE_NON_SAMPLER")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1_000_000);
        let hints = if std::env::var("PROBE_HINTS").as_deref() == Ok("memory") {
            wgpu::MemoryHints::MemoryUsage
        } else {
            wgpu::MemoryHints::Performance
        };
        // `PROBE_LIMITS=webgl2` is the most aggressive tier (2048 texture
        // limit and so on), to see whether the limits themselves can still
        // free memory.
        let which_limits = std::env::var("PROBE_LIMITS").unwrap_or_default();
        let limits = if which_limits == "default" {
            // WebGPU standard defaults: 8192 textures, but still a 1M
            // descriptor budget.
            wgpu::Limits {
                max_non_sampler_bindings: non_sampler,
                ..wgpu::Limits::default()
            }
        } else if which_limits == "wide" {
            // downlevel with textures raised to 8192 (needed by ultra-wide
            // multi-monitor docks).
            wgpu::Limits {
                max_non_sampler_bindings: non_sampler,
                max_texture_dimension_2d: 8192,
                ..wgpu::Limits::downlevel_defaults()
            }
        } else if std::env::var("PROBE_LIMITS").as_deref() == Ok("webgl2") {
            wgpu::Limits {
                max_non_sampler_bindings: non_sampler,
                ..wgpu::Limits::downlevel_webgl2_defaults()
            }
        } else {
            wgpu::Limits {
                max_non_sampler_bindings: non_sampler,
                ..wgpu::Limits::downlevel_defaults()
            }
        };
        eprintln!("[probe]    非采样绑定上限={non_sampler} hints={hints:?}");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("probe"),
                required_limits: limits,
                memory_hints: hints,
                ..Default::default()
            }))?;
        let desc = format!(
            "{:?} / {} / {:?} / driver {}",
            info.backend, info.name, info.device_type, info.driver
        );
        Ok((super::Device::Wgpu(Device { device, queue }), desc))
    }
}

/// D3D11 side: calls `D3D11CreateDevice` directly, since wgpu has no D3D11
/// backend.
mod d3d11_device {
    use windows::core::Interface;
    use windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL,
        D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
    };
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
        D3D11_BIND_RENDER_TARGET, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
    };
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, DXGI_SAMPLE_DESC,
    };
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter, IDXGIFactory1};

    use super::{Guard, TEX_H, TEX_W};

    /// One DXGI adapter record.
    pub struct Row {
        pub index: u32,
        pub name: String,
        pub vendor: u32,
        pub device_id: u32,
        pub dedicated_mb: usize,
        pub adapter: IDXGIAdapter,
    }

    pub struct Device {
        pub device: ID3D11Device,
        /// D3D11 device and immediate context are a pair; keep both.
        #[expect(dead_code, reason = "context must outlive the measurement")]
        pub context: ID3D11DeviceContext,
    }

    impl Device {
        pub fn allocate(&self, keep: &mut Guard) -> Result<(), Box<dyn std::error::Error>> {
            let rt = D3D11_TEXTURE2D_DESC {
                Width: TEX_W,
                Height: TEX_H,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let mut texture: Option<ID3D11Texture2D> = None;
            unsafe { self.device.CreateTexture2D(&rt, None, Some(&mut texture))? };
            keep.push(texture);

            // Readback staging texture: `Map` only reads staging resources in
            // D3D11.
            let staging = D3D11_TEXTURE2D_DESC {
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                ..rt
            };
            let mut readback: Option<ID3D11Texture2D> = None;
            unsafe {
                self.device
                    .CreateTexture2D(&staging, None, Some(&mut readback))?
            };
            keep.push(readback);
            Ok(())
        }
    }

    /// Enumerates the DXGI adapters of the system.
    pub fn adapters() -> Result<Vec<Row>, Box<dyn std::error::Error>> {
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
        let mut rows = Vec::new();
        // Until `DXGI_ERROR_NOT_FOUND`; enumeration has no count.
        for index in 0..16u32 {
            let Ok(adapter1) = (unsafe { factory.EnumAdapters1(index) }) else {
                break;
            };
            let Ok(desc) = (unsafe { adapter1.GetDesc1() }) else {
                break;
            };
            let end = desc
                .Description
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(desc.Description.len());
            rows.push(Row {
                index,
                name: String::from_utf16_lossy(&desc.Description[..end]),
                vendor: desc.VendorId,
                device_id: desc.DeviceId,
                dedicated_mb: desc.DedicatedVideoMemory / (1024 * 1024),
                adapter: adapter1.cast()?,
            });
        }
        Ok(rows)
    }

    pub fn create(
        warp: bool,
        want: &str,
    ) -> Result<(super::Device, String), Box<dyn std::error::Error>> {
        // With an explicit adapter the driver type must be UNKNOWN: passing
        // HARDWARE is rejected as E_INVALIDARG.
        let (driver, adapter) = if warp {
            (D3D_DRIVER_TYPE_WARP, None)
        } else if want.is_empty() {
            (D3D_DRIVER_TYPE_HARDWARE, None)
        } else {
            let needle = want.to_lowercase();
            let rows = adapters()?;
            let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
            let row = rows
                .iter()
                .find(|r| r.name.to_lowercase().contains(&needle))
                .ok_or_else(|| {
                    format!(
                        "没有匹配 `{want}` 的 DXGI 适配器，可选: {}",
                        names.join(" / ")
                    )
                })?;
            (D3D_DRIVER_TYPE_UNKNOWN, Some(row.adapter.clone()))
        };

        let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        let mut level = D3D_FEATURE_LEVEL::default();
        unsafe {
            D3D11CreateDevice(
                adapter.as_ref(),
                driver,
                // Do not bypass to the D3D11 software rasteriser; WARP is
                // chosen by the driver type.
                Default::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                Some(&mut level),
                Some(&mut context),
            )?;
        }
        let device = device.ok_or("D3D11CreateDevice 未返回设备")?;
        let context = context.ok_or("D3D11CreateDevice 未返回上下文")?;

        let desc = format!(
            "D3D11{} / feature level 0x{:x} / {}",
            if warp { " (WARP)" } else { "" },
            level.0,
            adapter_of(&device)
        );
        Ok((super::Device::D3d11(Device { device, context }), desc))
    }

    /// Name of the effective adapter. Failing is not an error: the probe's
    /// first job is measuring memory.
    fn adapter_of(device: &ID3D11Device) -> String {
        let Ok(dxgi) = device.cast::<windows::Win32::Graphics::Dxgi::IDXGIDevice>() else {
            return "未知".to_owned();
        };
        let Ok(adapter) = (unsafe { dxgi.GetAdapter() }) else {
            return "未知".to_owned();
        };
        let Ok(desc) = (unsafe { adapter.GetDesc() }) else {
            return "未知".to_owned();
        };
        let end = desc
            .Description
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(desc.Description.len());
        let name = String::from_utf16_lossy(&desc.Description[..end]);
        format!(
            "{name} (VEN {:04X} DEV {:04X}, 专用显存 {} MB)",
            desc.VendorId,
            desc.DeviceId,
            desc.DedicatedVideoMemory / (1024 * 1024)
        )
    }
}

/// OpenGL side: builds a compatibility context on a hidden window via WGL.
///
/// `egui_glow` is egui's official backend, so this measures the lower bound for
/// dropping wgpu entirely: if the GL baseline is clearly below D3D12, that
/// saves more than a framework swap.
///
/// Only GL 1.1 is used (exported directly by opengl32.dll) and no extension is
/// loaded, so the number is the driver's minimal context cost rather than some
/// loader's overhead.
mod gl_device {
    use windows::core::w;
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::{GetDC, HDC};
    use windows::Win32::Graphics::OpenGL::{
        wglCreateContext, wglMakeCurrent, ChoosePixelFormat, SetPixelFormat, HGLRC,
        PFD_DRAW_TO_WINDOW, PFD_SUPPORT_OPENGL, PFD_TYPE_RGBA, PIXELFORMATDESCRIPTOR,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, RegisterClassW, CS_OWNDC, WINDOW_EX_STYLE, WNDCLASSW,
        WS_POPUP,
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
    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
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
}
