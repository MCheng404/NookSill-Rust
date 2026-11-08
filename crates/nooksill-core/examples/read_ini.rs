//! Read a `NookSill.ini` and print the parse result, to debug "the config was
//! written but has no effect".
//!
//! # Why this exists
//!
//! Looking at the ini file is not enough: it does not show whether the
//! **encoding was recognized**, whether the **key landed in the `[Settings]`
//! section**, or **how many items were parsed**. Any one of those failing
//! looks the same from the outside.
//!
//! # Usage
//!
//! ```text
//! cargo run -q -p nooksill-core --example read_ini -- path/to/NookSill.ini
//! ```
//!
//! The `raw X=` lines print the **raw string**, so comparing them with the
//! parsed values tells "not read" apart from "read but not recognized".

use nooksill_core::config::Config;
use nooksill_core::ini::Ini;
use nooksill_core::store;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("用法: cargo run -p nooksill-core --example read_ini -- <ini 路径>");
    let ini = Ini::load(std::path::Path::new(&path)).expect("读取失败");

    // Parsed at a fixed 144 dpi: only "did the config get read" matters here,
    // not the DPI scaling.
    let cfg = Config::from_ini(&ini);

    println!(
        "Edge={:?} Mode={:?} Reserve={}",
        cfg.edge, cfg.mode, cfg.reserve
    );
    println!(
        "IconSize={} ZoomSize={} Clock={}",
        cfg.icon_size, cfg.zoom_size, cfg.clock
    );
    // Both the new and the legacy formats are listed, so the generation of
    // this file is visible at a glance.
    println!(
        "[meta] 键数={} / [Settings] 旧段键数={} / [Items] 键数={}",
        ini.len_of(nooksill_core::config::META_SECTION),
        ini.len_of(nooksill_core::config::LEGACY_SECTION),
        ini.len_of("Items")
    );
    println!(
        "格式版本={:?}（{} = 新格式）",
        ini.get(nooksill_core::config::META_SECTION, "version"),
        nooksill_core::config::CONFIG_VERSION
    );

    // Raw values: a parsed default cannot tell "not read" from "not
    // recognized". Both the new and the legacy key names are printed, which
    // shows which branch was taken.
    for key in ["layout/hide_mode", "layout/edge", "appearance/icon_size"] {
        let (sec, k) = key.split_once('/').expect("带货");
        println!("raw {sec}.{k}={:?}", ini.get(sec, k));
    }
    for key in ["Mode", "Edge", "IconSize"] {
        println!("raw Settings.{key}(旧)={:?}", ini.get("Settings", key));
    }
    println!("raw Items.Count={:?}", ini.get("Items", "Count"));

    let items = store::load_items(&ini);
    println!("解析出 {} 个条目", items.len());
    for (i, it) in items.iter().enumerate() {
        println!("  [{i}] {:?} name={} path={}", it.kind, it.name, it.path);
    }
}
