//! Does dropping the whole wgpu device hand working set back to the OS?
//!
//! ADR-117 measured that releasing a GPU *target* returned 0.00 MiB of working
//! set. The device is a different question, and CPU mode depends on the answer:
//! opening any panel creates one, and if dropping it returns nothing then the
//! driver floor is permanent for the rest of the process.
//!
//! Run: cargo run -p nooksill-gpu --release --example gpu_drop
//! Pin the card with PROBE_ADAPTER=AMD or PROBE_ADAPTER=NVIDIA.

use nooksill_gpu::gpu::Gpu;
use nooksill_gpu::mem::MemStats;

/// Median of several reads: one read is a coin flip under driver jitter.
/// Local rather than the shared probe helper, which also builds window sets
/// this probe has no use for.
fn sample() -> MemStats {
    let mut ws = Vec::new();
    let mut private = Vec::new();
    for _ in 0..7 {
        let s = MemStats::now();
        ws.push(s.working_set);
        private.push(s.private);
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    ws.sort_unstable();
    private.sort_unstable();
    MemStats {
        working_set: ws[ws.len() / 2],
        peak_working_set: ws[ws.len() / 2],
        private: private[private.len() / 2],
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wanted = std::env::var("PROBE_ADAPTER").unwrap_or_default();
    let settle = std::time::Duration::from_millis(600);
    println!("adapter filter {wanted:?}");

    let base = sample();
    println!(
        "baseline            ws {:7.2}   priv {:7.2}",
        base.ws_mb(),
        base.private_mb()
    );

    for round in 0..3 {
        let gpu = Gpu::new(&wanted)?;
        std::thread::sleep(settle);
        let up = sample();
        println!(
            "round {round} device up    ws {:7.2}   priv {:7.2}   (+{:6.2} ws)",
            up.ws_mb(),
            up.private_mb(),
            up.ws_mb() - base.ws_mb()
        );
        drop(gpu);
        std::thread::sleep(settle);
        let down = sample();
        println!(
            "round {round} device down  ws {:7.2}   priv {:7.2}   ({:+6.2} ws vs baseline)",
            down.ws_mb(),
            down.private_mb(),
            down.ws_mb() - base.ws_mb()
        );
    }

    // The premise the idle release rests on: a hidden panel holds a clone, and
    // one live clone is enough to keep every driver allocation alive.
    let gpu = Gpu::new(&wanted)?;
    let clone = gpu.clone();
    std::thread::sleep(settle);
    let both = sample();
    drop(gpu);
    std::thread::sleep(settle);
    let one = sample();
    println!(
        "clone held          ws {:7.2}   priv {:7.2}   ({:+6.2} ws after dropping the original)",
        one.ws_mb(),
        one.private_mb(),
        one.ws_mb() - both.ws_mb()
    );
    drop(clone);
    std::thread::sleep(settle);
    let none = sample();
    println!(
        "last clone dropped  ws {:7.2}   priv {:7.2}   ({:+6.2} ws)",
        none.ws_mb(),
        none.private_mb(),
        none.ws_mb() - one.ws_mb()
    );
    Ok(())
}
