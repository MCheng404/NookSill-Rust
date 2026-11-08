//! CJK font loading.
//!
//! egui's default fonts carry no CJK glyphs, so without this the UI renders
//! tofu boxes while the AccessKit tree and tests still pass. Font files are
//! read from the system at runtime rather than bundled as a subset.

use std::sync::{Arc, OnceLock};

/// One copy of the font bytes is shared by every `egui::Context` in the
/// process. `FontData` wraps an `Arc<Blob>`, so cloning it is cheap.
static CJK: OnceLock<Option<Arc<egui::FontData>>> = OnceLock::new();

/// System fonts tried in priority order; the first readable one is used.
const CANDIDATES: &[(&str, u32)] = &[
    (r"C:\Windows\Fonts\msyh.ttc", 0),   // Microsoft YaHei
    (r"C:\Windows\Fonts\msyhbd.ttc", 0), // Microsoft YaHei Bold
    (r"C:\Windows\Fonts\Deng.ttf", 0),   // DengXian
    (r"C:\Windows\Fonts\simhei.ttf", 0), // SimHei
    (r"C:\Windows\Fonts\simsun.ttc", 0), // SimSun
];

fn load_cjk() -> Option<Arc<egui::FontData>> {
    let (path, index) = CANDIDATES
        .iter()
        .find(|(p, _)| std::path::Path::new(p).is_file())?;
    let bytes = std::fs::read(path).ok()?;
    let mut data = egui::FontData::from_owned(bytes);
    data.index = *index; // face index inside a TTC collection
    Some(Arc::new(data))
}

/// Install the CJK font, returning false when no system font was found.
pub fn install_cjk(ctx: &egui::Context) -> bool {
    let Some(data) = CJK.get_or_init(load_cjk).as_ref() else {
        return false;
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("system-cjk".to_owned(), data.clone());
    // Prepend to both families: CJK comes from the system font, Latin glyphs
    // still fall through to the bundled ones.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "system-cjk".to_owned());
    }
    ctx.set_fonts(fonts);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_file_is_discoverable_on_this_machine() {
        // Diagnostic only: a missing font renders tofu, which no test can see.
        let found = super::CANDIDATES
            .iter()
            .any(|(p, _)| std::path::Path::new(p).is_file());
        eprintln!("CJK 字体可用: {found}");
    }

    #[test]
    fn font_bytes_are_shared_between_contexts() {
        let a = egui::Context::default();
        let b = egui::Context::default();
        if !install_cjk(&a) || !install_cjk(&b) {
            eprintln!("no CJK font on this machine; skip");
            return;
        }
        let data = CJK.get().unwrap().as_ref().unwrap();
        let refs = Arc::strong_count(data);
        eprintln!("shared CJK font refs: {refs}");
        assert!(refs >= 3, "static + 2 contexts must share one Arc");
    }
}
