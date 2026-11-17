//! Diagnostics for the system brightness backend.
//!
//! ```text
//! cargo run -p nooksill-platform --example brightness_probe
//! ```
//!
//! Lowers the level by 10 points, re-probes, then restores the original value.

use nooksill_platform::brightness::Brightness;

fn main() {
    let Some(mut control) = Brightness::probe() else {
        println!("no WMI or DDC/CI brightness interface");
        return;
    };
    let original = control.percent();
    println!(
        "backend={} initial={}",
        if control.is_wmi() { "wmi" } else { "ddc" },
        original
    );

    let target = original.saturating_sub(10);
    control.set(target);
    println!("set={}", control.percent());

    if let Some(again) = Brightness::probe() {
        println!(
            "reprobe={} backend={}",
            again.percent(),
            if again.is_wmi() { "wmi" } else { "ddc" }
        );
        println!("write_effective={}", again.percent() == target);
    }

    control.set(original);
    println!("restored={}", control.percent());
}
