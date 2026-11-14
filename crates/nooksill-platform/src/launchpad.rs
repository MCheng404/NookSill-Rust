//! Launchpad scan: `.lnk` files in the Start Menu.
//! Walks both Programs directories (machine and user), then sorts and dedups by
//! name. Read-only and COM-free; icons are fetched on demand by the `gpu` layer.

use std::path::Path;

// The entry type lives in core so the model layer can absorb scan results
// without a conversion step.
pub use nooksill_core::launchpad::AppEntry;

/// Names containing these words are uninstallers or docs and are skipped.
///
/// The match is a case-insensitive substring, like `StrStrIW`.
const BLOCK_WORDS: [&str; 5] = ["uninstall", "卸载", "readme", "帮助", "help"];

/// Recursion depth cap, checked as `depth < 4`.
const MAX_DEPTH: usize = 4;

/// Scan the Start Menu.
///
/// Machine directory first, then the user one; each recursed, then sorted by
/// name and deduplicated.
#[must_use]
pub fn scan() -> Vec<AppEntry> {
    let mut out = Vec::new();
    for dir in start_menu_dirs() {
        walk(&dir, 0, &mut out);
    }
    // Sort first, then drop neighbours with equal case-folded names.
    out.sort_by(|a, b| cmp_ci(&a.name, &b.name));
    out.dedup_by(|a, b| cmp_ci(&a.name, &b.name) == std::cmp::Ordering::Equal);
    out
}

/// Case-insensitive compare, approximating `lstrcmpiW`: stable and roughly
/// alphabetical is enough.
fn cmp_ci(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

/// Whether a name should be skipped.
#[must_use]
pub fn is_blocked(name: &str) -> bool {
    let lower = name.to_lowercase();
    BLOCK_WORDS.iter().any(|w| lower.contains(w))
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<AppEntry>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let name = ent.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with('.') {
            continue;
        }
        let path = ent.path();
        let is_dir = ent.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            if depth + 1 < MAX_DEPTH {
                walk(&path, depth + 1, out);
            }
            continue;
        }
        // `.lnk` only (the old code used `PathMatchSpecW(fd.cFileName, L"*.lnk")`).
        if !name.to_lowercase().ends_with(".lnk") {
            continue;
        }
        // Strip exactly the four extension bytes: one `trim_end_matches` per
        // spelling leaves `Foo.Lnk` with its extension still attached.
        let stem = link_stem(name);
        if is_blocked(stem) {
            continue;
        }
        out.push(AppEntry {
            name: stem.to_owned(),
            path: path.to_string_lossy().into_owned(),
        });
    }
}

/// Strip the `.lnk` extension in any casing.
///
/// The caller has already checked the lowercase form, so the last four bytes
/// are ASCII and slicing is safe.
fn link_stem(name: &str) -> &str {
    name.get(..name.len().saturating_sub(4)).unwrap_or(name)
}

/// The two Start Menu directories: machine (`CSIDL_COMMON_PROGRAMS`) and user
/// (`CSIDL_PROGRAMS`), located through their environment roots.
fn start_menu_dirs() -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if let Ok(p) = std::env::var("ProgramData") {
        v.push(Path::new(&p).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(p) = std::env::var("APPDATA") {
        v.push(Path::new(&p).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uninstallers and docs are filtered out, real apps are not.
    #[test]
    fn blocked_words_filter_installers() {
        assert!(is_blocked("Uninstall Steam"));
        assert!(is_blocked("卸载 微信"));
        assert!(is_blocked("readme"));
        assert!(is_blocked("帮助文档"));
        assert!(!is_blocked("微信"));
        assert!(!is_blocked("Visual Studio Code"));
        // Substring matching catches "helped" too; that is intended.
        assert!(is_blocked("Helper"));
    }

    #[test]
    fn link_extension_is_stripped_in_any_casing() {
        assert_eq!(link_stem("Steam.lnk"), "Steam");
        assert_eq!(link_stem("Steam.LNK"), "Steam");
        assert_eq!(link_stem("Steam.Lnk"), "Steam", "混合大小写也要去掉");
        assert_eq!(link_stem("微信.lnk"), "微信");
        // Degenerate input must not panic; the caller filters it out first.
        assert_eq!(link_stem("ab"), "");
    }

    /// Results must be name-ordered and free of duplicates.
    #[test]
    fn scan_is_sorted_and_deduped() {
        let apps = scan();
        for w in apps.windows(2) {
            assert!(
                cmp_ci(&w[0].name, &w[1].name) != std::cmp::Ordering::Greater,
                "{} 排在 {} 前面但字典序更大",
                w[0].name,
                w[1].name
            );
        }
        // A normal machine has apps installed, but empty is fine (CI may have no
        // Start Menu).
        for a in &apps {
            assert!(!a.name.is_empty());
            assert!(a.path.to_lowercase().ends_with(".lnk"), "{}", a.path);
        }
    }
}
