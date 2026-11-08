//! Resolve a `.lnk` to its target executable.
//!
//! Needed to match a Dock item against a running process.
//! Uses the generated `windows` 0.62.2 wrappers; their signatures match the
//! COM ABI (ADR-063 supersedes ADR-029).

use windows::core::{Interface, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shortcut {
    /// Empty when the link points to a URL or UWP app.
    pub target: String,
    pub args: String,
}

// COM is per-thread; see crate::win::ensure_com.
fn ensure_com() {
    crate::win::ensure_com();
}

// Decode up to the first NUL. No NUL means a truncated path.
fn wide_to_string(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

fn resolve_lnk(path: &str) -> Option<Shortcut> {
    ensure_com();

    // SAFETY: COM calls with RAII-owned interface pointers. Output buffers
    // live on this stack frame; the wrapper passes their length as `cch`.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist: IPersistFile = link.cast().ok()?;
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        persist.Load(PCWSTR(wide.as_ptr()), STGM_READ).ok()?;

        let mut buf = [0u16; 1024];
        // flags = 0: standard long path. pfd = NULL is allowed by the API.
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let target = wide_to_string(&buf);
        if target.is_empty() {
            return None;
        }

        // Arguments are optional; only the target matters for matching.
        let mut abuf = [0u16; 1024];
        let args = if link.GetArguments(&mut abuf).is_ok() {
            wide_to_string(&abuf)
        } else {
            String::new()
        };

        Some(Shortcut { target, args })
    }
}

/// 读一个 `.url` Internet Shortcut，取出目标网址。
///
/// 拖入 `.url` 时不能当成普通 `File`：那样 `ShellExecute` 虽然也能打开，
/// 但 `ItemKind::File` 会走运行状态扫描、图标按 exe 取，语义全错。
/// 解析成 `ItemKind::Url` 才能走 `Browser` 配置那套浏览器选择。
#[must_use]
pub fn read_url_shortcut(path: &str) -> Option<String> {
    // `.url` 常见是 ANSI/UTF-8，偶尔带 BOM。`from_utf8_lossy` 会把坏字节
    // 换成占位符，但 `URL=` 那一行是纯 ASCII，不受影响。
    let bytes = std::fs::read(path).ok()?;
    parse_url_shortcut(&String::from_utf8_lossy(&bytes))
}

/// 从 `.url` 的文本里找 `URL=`。抽成纯函数，单测不必造临时文件。
#[must_use]
pub fn parse_url_shortcut(content: &str) -> Option<String> {
    let mut in_section = false;
    for raw in content.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.starts_with('[') && line.ends_with(']') {
            in_section = line.eq_ignore_ascii_case("[InternetShortcut]");
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("URL") {
            continue;
        }
        let url = value.trim();
        // 只认真网址：本地路径、空值、奇怪的 scheme 一律不给。
        if url.len() > 8
            && (url.starts_with("http://")
                || url.starts_with("https://")
                || url.starts_with("ftp://"))
        {
            return Some(url.to_owned());
        }
        return None;
    }
    None
}

/// Return `None` when the item has no exe to match against a running process.
#[must_use]
pub fn resolve(path: &str) -> Option<Shortcut> {
    let lower = path.to_lowercase();
    if lower.ends_with(".exe") {
        return Some(Shortcut {
            target: path.to_owned(),
            args: String::new(),
        });
    }
    if !lower.ends_with(".lnk") {
        return None;
    }
    let sc = resolve_lnk(path)?;
    if !sc.target.to_lowercase().ends_with(".exe") {
        return None;
    }
    Some(sc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_standard_url_shortcut() {
        let s = "[InternetShortcut]\nURL=https://example.com/a?b=1\nIconIndex=0\n";
        assert_eq!(
            parse_url_shortcut(s),
            Some("https://example.com/a?b=1".to_owned())
        );
    }

    #[test]
    fn url_shortcut_ignores_other_sections_and_casing() {
        let s = "\u{feff}[InternetShortcut]\r\n url = HTTP://Example.COM \r\n[Other]\r\nURL=file:///etc/passwd\r\n";
        // 只认小写 http/https/ftp 前缀，大小写不一致的 scheme 不给 ——
        // `open_url` 后续按字符串直接拼，宁可保守。
        assert_eq!(parse_url_shortcut(s), None);
        let ok = "[internetshortcut]\nURL=http://a.cn\n[Other]\nURL=file:///x\n";
        assert_eq!(parse_url_shortcut(ok), Some("http://a.cn".to_owned()));
    }

    #[test]
    fn url_shortcut_rejects_local_paths_and_junk() {
        assert_eq!(
            parse_url_shortcut("[InternetShortcut]\nURL=file:///x"),
            None
        );
        assert_eq!(parse_url_shortcut("[InternetShortcut]\nURL="), None);
        assert_eq!(parse_url_shortcut("URL=https://x"), None, "不在段里");
        assert_eq!(parse_url_shortcut("不是 url 文件"), None);
    }

    #[test]
    fn non_lnk_non_exe_has_no_target() {
        assert!(resolve("C:\\Users\\Public\\Documents").is_none());
        assert!(resolve("https://example.com").is_none());
        assert!(resolve("C:\\a\\readme.txt").is_none());
    }

    #[test]
    fn exe_resolves_to_itself() {
        let s = resolve("C:\\Windows\\System32\\notepad.exe").expect("应当解析出自身");
        assert_eq!(s.target, "C:\\Windows\\System32\\notepad.exe");
        assert!(s.args.is_empty());
        let s = resolve("C:\\X\\NOTEPAD.EXE").expect("扩展名比较要忽略大小写");
        assert!(s.target.ends_with("NOTEPAD.EXE"));
    }

    #[test]
    fn missing_lnk_returns_none() {
        assert!(resolve("C:\\definitely\\missing\\thing.lnk").is_none());
    }

    #[test]
    fn wide_buffer_stops_at_first_nul() {
        // Trailing bytes may be residue from a previous call.
        let buf = [
            'C' as u16,
            ':' as u16,
            '\\' as u16,
            'a' as u16,
            0,
            'X' as u16,
            'Y' as u16,
        ];
        assert_eq!(wide_to_string(&buf), "C:\\a");
    }

    #[test]
    fn wide_buffer_empty_and_unterminated() {
        assert_eq!(wide_to_string(&[0, 0, 0]), "");
        assert_eq!(wide_to_string(&[]), "");
        assert_eq!(wide_to_string(&[65, 66, 67]), "ABC");
    }

    /// Real COM round-trip. Needs a desktop session and installed apps:
    /// `cargo test -p nooksill-platform --lib shortcut -- --ignored`
    #[test]
    #[ignore = "依赖本机已安装的应用，需显式 --ignored"]
    fn resolves_a_real_start_menu_shortcut() {
        let root = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".to_owned());
        let dir = std::path::Path::new(&root).join("Microsoft\\Windows\\Start Menu\\Programs");
        let mut tried = 0;
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("lnk") {
                continue;
            }
            tried += 1;
            let Some(sc) = resolve(&p.to_string_lossy()) else {
                continue;
            };
            assert!(
                sc.target.to_lowercase().ends_with(".exe"),
                "解析结果不是 exe: {}",
                sc.target
            );
            assert!(
                std::path::Path::new(&sc.target).exists(),
                "解析出的 exe 不存在: {}",
                sc.target
            );
            println!("{} -> {}", p.display(), sc.target);
            return;
        }
        println!("开始菜单里只有 {tried} 个 .lnk，跳过");
    }
}
