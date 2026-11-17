//! Diagnostics for the Host Link v1 provider.
//!
//! ```text
//! cargo run -p nooksill-platform --example hostlink_probe
//! ```
//!
//! Prints the six bootstrap channels in protocol order and the health report
//! of the first valid manifest found, without spawning anything except the
//! manifest's own `probe` command.

use nooksill_platform::hostlink::HostLinkProvider;

fn main() {
    nooksill_core::i18n::configure("", nooksill_platform::locale::ui_language_id());
    let mut provider = HostLinkProvider::default();
    let found = provider.discover();

    println!("== discovery channels ==");
    for (i, path) in found.visited.iter().enumerate() {
        println!(
            "{}. [{}] {}",
            i + 1,
            if path.exists() { "exists" } else { "missing" },
            path.display()
        );
    }
    println!("shortcut: {:?}", found.shortcut);

    let Some(link) = found.link else {
        println!("== no valid mc.host-link/v1 manifest ==");
        return;
    };
    println!("== manifest ==");
    println!("id: {}", link.id);
    println!("name: {}", link.name);
    println!("exe: {}", link.exe.display());
    println!("source: {}", link.discovered_at.display());
    println!("commands: {:?}", link.commands.keys().collect::<Vec<_>>());
    println!("entries: {:?}", provider.entries());

    let report = provider.probe(&link.id);
    println!("== probe ==");
    println!("ok: {}", report.ok);
    println!("ui_running: {}", report.ui_running);
    println!("error: {:?}", report.error);
    println!("repair: {:?}", report.repair);
    println!("source: {}", report.source);
}
