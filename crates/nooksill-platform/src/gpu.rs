//! Graphics adapter enumeration.
//! DXGI is used because the settings UI needs no GPU layer, and it supplies the
//! same names the wgpu DX12 backend matches. Selection stays in `nooksill-gpu`.

use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
};

/// DXGI reports a software adapter through this flag, not through its name.
const SOFTWARE_FLAG: u32 = DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32;

/// Adapter class. A heuristic for sorting and labels, never a logic branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AdapterKind {
    /// Discrete GPU: has its own video memory partition.
    Discrete,
    /// Integrated GPU: shares system memory.
    Integrated,
    /// Software rasterizer (`Microsoft Basic Render Driver`). Cannot back the
    /// dock, but stays listed to explain why nothing is selectable.
    Software,
    /// Undetermined (virtual GPU or bogus driver values).
    Other,
}

impl AdapterKind {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Discrete => nooksill_core::i18n::t("Discrete"),
            Self::Integrated => nooksill_core::i18n::t("Integrated"),
            Self::Software => nooksill_core::i18n::t("Software"),
            Self::Other => nooksill_core::i18n::t("Other"),
        }
    }
}

/// One adapter record, trimmed to what can be shown and matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterInfo {
    pub name: String,
    pub vendor: u32,
    pub device_id: u32,
    pub dedicated_mb: usize,
    pub kind: AdapterKind,
}

impl AdapterInfo {
    /// One line for the UI, e.g. `NVIDIA GeForce RTX 5070 Laptop GPU (Discrete, 8 GB)`.
    #[must_use]
    pub fn display_name(&self) -> String {
        if self.dedicated_mb >= 1024 {
            format!(
                "{}（{} · {} GB）",
                self.name,
                self.kind.label(),
                self.dedicated_mb / 1024
            )
        } else {
            format!("{}（{}）", self.name, self.kind.label())
        }
    }
}

/// Enumerate adapters, sorted discrete, integrated, other, software.
///
/// DXGI lists one GPU once per attached output, so records are deduplicated by
/// `(vendor, device_id, name)`; otherwise the dropdown shows identical entries.
#[must_use]
pub fn adapters() -> Vec<AdapterInfo> {
    let Ok(factory): Result<IDXGIFactory1, _> = (unsafe { CreateDXGIFactory1() }) else {
        return Vec::new();
    };

    let mut out: Vec<AdapterInfo> = Vec::new();
    // There is no count query; stop at DXGI_ERROR_NOT_FOUND. 16 is a defensive cap.
    for index in 0..16u32 {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
            break;
        };
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        let end = desc
            .Description
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(desc.Description.len());
        let name = String::from_utf16_lossy(&desc.Description[..end]);
        if name.is_empty() {
            continue;
        }
        let dedicated_mb = desc.DedicatedVideoMemory / (1024 * 1024);
        let kind = classify(desc.Flags, dedicated_mb);
        let info = AdapterInfo {
            name,
            vendor: desc.VendorId,
            device_id: desc.DeviceId,
            dedicated_mb,
            kind,
        };
        if !out.iter().any(|a| {
            a.vendor == info.vendor && a.device_id == info.device_id && a.name == info.name
        }) {
            out.push(info);
        }
    }

    out.sort_by_key(|a| a.kind);
    out
}

/// DXGI flags decide software adapters; dedicated memory splits the rest.
///
/// The 1 GB threshold works because an iGPU may reserve 256-512 MB while any
/// recent discrete card exceeds 1 GB. A wrong guess only mislabels the UI.
fn classify(flags: u32, dedicated_mb: usize) -> AdapterKind {
    if flags & SOFTWARE_FLAG != 0 {
        AdapterKind::Software
    } else if dedicated_mb >= 1024 {
        AdapterKind::Discrete
    } else if dedicated_mb > 0 {
        AdapterKind::Integrated
    } else {
        AdapterKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::{adapters, classify, AdapterKind, SOFTWARE_FLAG};

    #[test]
    fn classify_uses_dxgi_flags_not_adapter_names() {
        assert_eq!(classify(0, 485), AdapterKind::Integrated);
        assert_eq!(classify(0, 7891), AdapterKind::Discrete);
        assert_eq!(classify(SOFTWARE_FLAG, 0), AdapterKind::Software);
        // A software adapter with reported VRAM is still software.
        assert_eq!(classify(SOFTWARE_FLAG, 4096), AdapterKind::Software);
        assert_eq!(classify(0, 0), AdapterKind::Other);
        // Other DXGI flags must not be mistaken for the software bit.
        assert_eq!(classify(1, 2048), AdapterKind::Discrete);
    }

    #[test]
    fn adapters_are_available_and_deduplicated() {
        let list = adapters();
        // A machine without a GPU may return an empty list, but any entry must be clean.
        for (i, a) in list.iter().enumerate() {
            assert!(!a.name.is_empty());
            for b in &list[i + 1..] {
                assert!(
                    !(a.vendor == b.vendor && a.device_id == b.device_id && a.name == b.name),
                    "重复适配器: {} / {}",
                    a.name,
                    b.name
                );
            }
        }
    }

    #[test]
    fn software_adapters_sort_last() {
        let list = adapters();
        let first_software = list.iter().position(|a| a.kind == AdapterKind::Software);
        if let Some(i) = first_software {
            assert!(
                list[i..].iter().all(|a| a.kind == AdapterKind::Software),
                "软件光栅器必须排在最后"
            );
        }
    }
}
