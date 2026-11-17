//! Diagnostics for declarative plugin packs.
//!
//! ```text
//! cargo run -p nooksill-platform --example plugin_probe
//! ```

use std::path::Path;

fn main() {
    let example =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plugin/example/plugin.json");
    match std::fs::read_to_string(&example) {
        Ok(text) => match nooksill_platform::plugin::parse_plugin(&text, &example) {
            Ok(plugin) => println!(
                "example ok: id={} name={} settings={} accent={:?}",
                plugin.id,
                plugin.name,
                plugin.settings.len(),
                plugin.accent
            ),
            Err(error) => println!("example rejected: {error}"),
        },
        Err(error) => println!("example missing: {error}"),
    }

    let found = nooksill_platform::plugin::rescan();
    println!("installed packs: {found}");
    for row in nooksill_platform::plugin::snapshot("") {
        println!(
            "- {} {} | {} | {} | {}",
            row.name, row.version, row.id, row.author, row.description
        );
    }
}
