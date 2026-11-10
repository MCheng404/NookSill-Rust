//! Device measurement for the d3d11 device path.

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
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, DXGI_SAMPLE_DESC};
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
