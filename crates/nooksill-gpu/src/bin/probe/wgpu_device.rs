//! Device measurement for the wgpu device path.

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
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
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
