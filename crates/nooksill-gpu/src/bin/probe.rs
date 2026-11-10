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
// A binary crate root resolves mod x beside the root file, not under probe/.
#[path = "probe/wgpu_device.rs"]
mod wgpu_device;

/// D3D11 side: calls `D3D11CreateDevice` directly, since wgpu has no D3D11
/// backend.
#[path = "probe/d3d11_device.rs"]
mod d3d11_device;

/// OpenGL side: builds a compatibility context on a hidden window via WGL.
///
/// `egui_glow` is egui's official backend, so this measures the lower bound for
/// dropping wgpu entirely: if the GL baseline is clearly below D3D12, that
/// saves more than a framework swap.
///
/// Only GL 1.1 is used (exported directly by opengl32.dll) and no extension is
/// loaded, so the number is the driver's minimal context cost rather than some
/// loader's overhead.
#[path = "probe/gl_device.rs"]
mod gl_device;
