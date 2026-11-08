//! wgpu device and color format.
//!
//! No `wgpu::Surface`: Windows has no alpha swapchain (DX12/Vulkan report
//! `Opaque`), so frames go to an offscreen texture blitted by `present.rs` with
//! `UpdateLayeredWindow`, which restores per-pixel alpha.

use std::sync::atomic::{AtomicU32, Ordering};

use egui_wgpu::wgpu;

/// MSAA preference from config: 0 = auto, 1/2/4 = forced. Process-wide,
/// read when each `FrameRenderer` is created.
static MSAA_PREF: AtomicU32 = AtomicU32::new(0);

pub fn set_msaa_preference(v: i32) {
    MSAA_PREF.store(v.clamp(0, 4) as u32, Ordering::Relaxed);
}

#[must_use]
pub fn msaa_preference() -> u32 {
    MSAA_PREF.load(Ordering::Relaxed)
}

/// GPU initialization failure.
#[derive(Debug)]
pub enum GpuError {
    Wgpu(String),
    Win32(&'static str),
}

impl std::fmt::Display for GpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wgpu(s) => write!(f, "wgpu: {s}"),
            Self::Win32(s) => write!(f, "Win32: {s}"),
        }
    }
}

impl std::error::Error for GpuError {}

/// Which adapter ended up in use, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    /// Configured as automatic.
    Auto,
    /// Matched by name.
    Pinned,
    /// A name was configured but not found; fell back to automatic.
    Fallback {
        wanted: String,
        available: Vec<String>,
    },
}

impl Selection {
    /// One-line description for logs and the UI.
    ///
    /// Only the first three candidates are listed: the settings UI shows this
    /// as an orange hint and a full list overflows the layout.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Auto => "自动".to_owned(),
            Self::Pinned => "按配置指定".to_owned(),
            Self::Fallback { wanted, available } => {
                const SHOW: usize = 3;
                let head: Vec<&str> = available.iter().take(SHOW).map(String::as_str).collect();
                let more = available.len().saturating_sub(SHOW);
                let tail = if more > 0 {
                    format!(" 等 {} 项", available.len())
                } else {
                    String::new()
                };
                format!(
                    "配置的「{wanted}」未找到，已回退自动；可选：{}{tail}",
                    head.join(" / ")
                )
            }
        }
    }

    /// Whether a fallback happened; the UI colors the hint with this.
    #[must_use]
    pub const fn fell_back(&self) -> bool {
        matches!(self, Self::Fallback { .. })
    }

    /// Whether the ini `[Graphics] Adapter` still matches the adapter picked at
    /// startup.
    ///
    /// The device is created once per process, so runtime config edits never
    /// switch it; the settings UI warns about the required restart instead.
    /// `actual` is `Gpu::info.name`, which wgpu may suffix with a vendor name,
    /// hence the mutual-contains comparison used in `pick_by_name` too.
    #[must_use]
    pub fn matches_config(&self, configured: &str, actual: &str) -> bool {
        match self {
            Self::Auto => configured.is_empty(),
            Self::Fallback { wanted, .. } => wanted.eq_ignore_ascii_case(configured),
            Self::Pinned => {
                let a = configured.trim().to_lowercase();
                let b = actual.trim().to_lowercase();
                !a.is_empty() && (a == b || a.contains(&b) || b.contains(&a))
            }
        }
    }
}

pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: wgpu::AdapterInfo,
    pub selection: Selection,
}

impl std::fmt::Debug for Gpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gpu")
            .field("backend", &self.info.backend)
            .field("device", &self.info.name)
            .field("selection", &self.selection)
            .finish_non_exhaustive()
    }
}

impl Gpu {
    /// Color format for offscreen rendering.
    ///
    /// Must be sRGB: with `Rgba8Unorm` egui gamma-encodes into a non-sRGB target
    /// itself and everything brightens (24 grey read back as 81). With
    /// `Rgba8UnormSrgb` the hardware encodes and the bytes match the screen.
    pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

    /// The only tightened limit: the non-sampler binding budget.
    ///
    /// wgpu defaults to 1_000_000. Per its docs this affects DX12 only, which
    /// pre-allocates the global descriptor heap at device creation; on integrated
    /// GPUs that comes out of system RAM. Measured on an AMD 610M: 1_000_000
    /// costs 164 MB, 4096 costs 133 MB, lower values gain nothing. The dock uses
    /// one or two bind groups per frame.
    pub const DESCRIPTOR_BUDGET: u32 = 4096;

    /// Build limits from the adapter's own capabilities, tightening only the
    /// descriptor budget.
    ///
    /// Do not use `Limits::downlevel_defaults()`: its `max_texture_dimension_2d`
    /// is 2048, while a 2560-wide dock at 150% DPI needs a 3840px offscreen
    /// texture and would fail on ultrawide screens. Adapter limits are safe and
    /// cost no extra memory.
    fn device_limits(adapter: &wgpu::Adapter) -> wgpu::Limits {
        wgpu::Limits {
            max_non_sampler_bindings: Self::DESCRIPTOR_BUDGET,
            ..adapter.limits()
        }
    }

    /// Create the instance, pick an adapter by `wanted` (empty = automatic) and
    /// request a device.
    pub fn new(wanted: &str) -> Result<Self, GpuError> {
        let backends = requested_backends();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: wgpu::BackendOptions::default(),
            display: None,
        });

        let wanted = wanted.trim();
        let (adapter, selection) = if wanted.is_empty() {
            (pick_auto(&instance)?, Selection::Auto)
        } else {
            match pick_by_name(&instance, wanted) {
                Some(adapter) => (adapter, Selection::Pinned),
                None => {
                    let available: Vec<String> =
                        pollster::block_on(instance.enumerate_adapters(backends))
                            .into_iter()
                            .map(|a| a.get_info().name)
                            .collect();
                    (
                        pick_auto(&instance)?,
                        Selection::Fallback {
                            wanted: wanted.to_owned(),
                            available,
                        },
                    )
                }
            }
        };

        let info = adapter.get_info();
        // 2x MSAA needs adapter-specific format features: WebGPU only guarantees
        // [1, 4] for `Rgba8UnormSrgb`, and large windows use 2x to save VRAM.
        // Requesting the feature is free; without it `perf::resolve_samples`
        // falls back to the guaranteed counts.
        let mut features = wgpu::Features::empty();
        if adapter
            .features()
            .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
        {
            features |= wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("nooksill"),
            required_features: features,
            required_limits: Self::device_limits(&adapter),
            // One or two bind groups per frame, so no allocator headroom needed.
            // Measured on an AMD 610M: Performance 164 MB, MemoryUsage 102 MB.
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .map_err(|e| GpuError::Wgpu(format!("请求设备失败: {e}")))?;

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            info,
            selection,
        })
    }
}

/// Backends this process will use.
///
/// wgpu abstracts the graphics API, but `Backends` is fixed at instance
/// creation. The Windows default is deliberately not `PRIMARY`: on machines with
/// a Vulkan driver wgpu would prefer it, and the DX12 path is the one tuned
/// against DWM composition.
///
/// `NOOKSILL_BACKEND` is for port validation only, not a user setting: it decides
/// whether the process starts at all, which a settings UI cannot help with.
fn requested_backends() -> wgpu::Backends {
    use wgpu::Backends;
    let raw = std::env::var("NOOKSILL_BACKEND").unwrap_or_default();
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "auto" => default_backends(),
        "dx12" => Backends::DX12,
        "vulkan" | "vk" => Backends::VULKAN,
        "gl" | "opengl" => Backends::GL,
        "metal" => Backends::METAL,
        other => {
            eprintln!("[gpu] 未知后端 `{other}`（可选 auto/dx12/vulkan/gl/metal），按 auto 处理");
            default_backends()
        }
    }
}

/// Platform default: DX12 on Windows, wgpu's `PRIMARY` elsewhere.
const fn default_backends() -> wgpu::Backends {
    if cfg!(windows) {
        wgpu::Backends::DX12
    } else {
        wgpu::Backends::PRIMARY
    }
}

/// Automatic: let the backend pick by power preference. The dock always runs, so
/// keep the discrete GPU asleep.
fn pick_auto(instance: &wgpu::Instance) -> Result<wgpu::Adapter, GpuError> {
    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .map_err(|e| GpuError::Wgpu(format!("找不到可用适配器: {e}")))
}

/// Pick by name.
///
/// Exact match (case-insensitive) first, then mutual containment: wgpu may
/// append a vendor suffix while the user picked the raw DXGI name.
fn pick_by_name(instance: &wgpu::Instance, wanted: &str) -> Option<wgpu::Adapter> {
    let needle = wanted.to_lowercase();
    let adapters = pollster::block_on(instance.enumerate_adapters(requested_backends()));

    let exact = adapters
        .iter()
        .position(|a| a.get_info().name.to_lowercase() == needle);
    let index = exact.or_else(|| {
        adapters.iter().position(|a| {
            let name = a.get_info().name.to_lowercase();
            name.contains(&needle) || needle.contains(&name)
        })
    })?;

    adapters.into_iter().nth(index)
}

#[cfg(test)]
mod tests {
    use super::Selection;

    #[test]
    fn selection_describes_itself() {
        assert_eq!(Selection::Auto.describe(), "自动");
        assert!(!Selection::Auto.fell_back());

        let fb = Selection::Fallback {
            wanted: "某个不存在的卡".to_owned(),
            available: vec!["A".to_owned(), "B".to_owned()],
        };
        assert!(fb.fell_back());
        let text = fb.describe();
        assert!(text.contains("某个不存在的卡"));
        assert!(text.contains("A / B"));
    }

    /// A runtime adapter change must be detected, or the UI never warns about the
    /// restart.
    #[test]
    fn matches_config_detects_a_pending_adapter_change() {
        let actual = "AMD Radeon(TM) 610M";
        assert!(Selection::Auto.matches_config("", actual));
        assert!(!Selection::Auto.matches_config("NVIDIA RTX 5070", actual));

        let fb = Selection::Fallback {
            wanted: "不存在的卡".to_owned(),
            available: vec!["AMD".to_owned()],
        };
        assert!(fb.matches_config("不存在的卡", actual));
        assert!(!fb.matches_config("AMD", actual));

        // A vendor suffix still identifies the same card.
        assert!(Selection::Pinned.matches_config("AMD Radeon", actual));
        assert!(Selection::Pinned.matches_config("AMD Radeon(TM) 610M", actual));
        assert!(!Selection::Pinned.matches_config("NVIDIA RTX 5070", actual));
        assert!(!Selection::Pinned.matches_config("", actual));
    }

    #[test]
    fn descriptor_budget_stays_small() {
        // 4096 is the measured knee: lower values save nothing more, higher ones
        // waste memory. Restoring wgpu's 1_000_000 default throws away 31 MB.
        let budget = super::Gpu::DESCRIPTOR_BUDGET;
        assert!(
            (1024..=65536).contains(&budget),
            "描述符预算 {budget} 不在实测区间内，改之前先重跑 probe"
        );
    }

    #[test]
    fn fallback_lists_at_most_three_candidates() {
        let fb = Selection::Fallback {
            wanted: "X".to_owned(),
            available: vec![
                "A".to_owned(),
                "B".to_owned(),
                "C".to_owned(),
                "D".to_owned(),
                "E".to_owned(),
            ],
        };
        let text = fb.describe();
        assert!(text.contains("A / B / C"));
        assert!(!text.contains('D'), "第四个不应出现: {text}");
        assert!(text.contains("等 5 项"), "总数量要交代清楚: {text}");
    }
}
