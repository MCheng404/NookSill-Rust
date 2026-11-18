//! Decomposes resident memory into the DIB and the GPU targets, and times DIB
//! creation. Answers what the A/B probe cannot:
//!
//! * how much of the working set is the layered DIB, and how much is the
//!   offscreen texture / MSAA attachment / readback buffer;
//! * what `CreateDIBSection` actually costs on reopen;
//! * how big the dock bar's own DIB is.
//!
//! ```text
//! cargo run -p nooksill-gpu --release --example mem_surface -- [ppp] [msaa]
//! ```

#[path = "common/surfaces.rs"]
mod surfaces;

use std::time::{Duration, Instant};

use egui_wgpu::wgpu;
use nooksill_gpu::gpu::Gpu;
use nooksill_gpu::{LayeredPresenter, Offscreen};

use surfaces::{mb, physical, sample};

/// Touch one byte per page so readback pages become resident without
/// allocating a full-frame copy that would pollute the measurement.
fn touch_pages(data: &[u8]) -> u8 {
    let mut acc = 0u8;
    let mut i = 0;
    while i < data.len() {
        acc = acc.wrapping_add(data[i]);
        i += 4096;
    }
    acc
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    nooksill_core::i18n::configure("", nooksill_platform::locale::ui_language_id());
    let mut args = std::env::args().skip(1);
    let ppp: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1.5_f32);
    let msaa: i32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(4);
    let adapter = std::env::var("PROBE_ADAPTER").unwrap_or_default();

    let gpu = Gpu::new(&adapter)?;
    nooksill_gpu::gpu::set_msaa_preference(msaa);
    println!("{} ppp={ppp} msaa={msaa}", gpu.selection.describe());
    let sizes = physical(ppp);

    dib_cost(&sizes)?;
    dib_residency(&sizes)?;
    gpu_residency(&gpu, &sizes, msaa)?;
    dock_dib(ppp);
    Ok(())
}

/// What a reopen pays: `CreateDIBSection` plus the DC setup, repeatedly.
fn dib_cost(sizes: &[(String, u32, u32)]) -> Result<(), Box<dyn std::error::Error>> {
    let (_, w, h) = sizes
        .iter()
        .max_by_key(|(_, w, h)| u64::from(*w) * u64::from(*h))
        .expect("window set is not empty");
    let mut samples: Vec<Duration> = Vec::new();
    for _ in 0..200 {
        let t = Instant::now();
        let p = LayeredPresenter::new(*w as i32, *h as i32)?;
        samples.push(t.elapsed());
        drop(p);
    }
    samples.sort_unstable();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    println!(
        "\nCreateDIBSection {w}x{h} x200: p50 {:.3} ms  p95 {:.3} ms  max {:.3} ms",
        ms(samples[samples.len() / 2]),
        ms(samples[samples.len() * 95 / 100]),
        ms(samples[samples.len() - 1])
    );
    Ok(())
}

/// Working-set contribution of the DIB alone, with every page written.
fn dib_residency(sizes: &[(String, u32, u32)]) -> Result<(), Box<dyn std::error::Error>> {
    // The source frame stays alive across both samples so its own pages are
    // not part of the delta.
    let biggest = sizes
        .iter()
        .map(|(_, w, h)| (*w as usize) * (*h as usize) * 4)
        .max()
        .expect("window set is not empty");
    let src = vec![0x80u8; biggest];

    let mut presenters: Vec<LayeredPresenter> = Vec::new();
    for (_, w, h) in sizes {
        let mut p = LayeredPresenter::new(*w as i32, *h as i32)?;
        p.upload_rgba_strided(&src[..(*w as usize) * (*h as usize) * 4], *w as usize * 4);
        presenters.push(p);
    }
    let held = sample();
    presenters.clear();
    let freed = sample();
    report("DIB only", held, freed);
    Ok(())
}

/// Working-set contribution of texture + MSAA + readback, with the readback
/// pages mapped and read the way the present path does.
fn gpu_residency(
    gpu: &Gpu,
    sizes: &[(String, u32, u32)],
    msaa: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut targets: Vec<Offscreen> = Vec::new();
    let mut analytic: u64 = 0;
    // Acquisition and reclamation are not symmetric: allocating faults pages
    // in, freeing may leave them in a retained allocator block. Measure both.
    let before = sample();
    for (_, w, h) in sizes {
        let off = Offscreen::new(&gpu.device, *w, *h, Gpu::COLOR_FORMAT, msaa.unsigned_abs());
        let row = u64::from(*w) * 4;
        analytic += row * u64::from(*h) * (1 + u64::from(off.samples()))
            + row.div_ceil(256) * 256 * u64::from(*h);
        let mut enc = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("mem_surface"),
            });
        off.copy_to_buffer(&mut enc);
        let _ = gpu.queue.submit([enc.finish()]);
        let acc = off.with_mapped_pixels(&gpu.device, |data, _| Ok(touch_pages(data)))?;
        std::hint::black_box(acc);
        targets.push(off);
    }
    println!("\nGPU targets analytic {:.2} MiB", mb(analytic));
    report("GPU acquire", before, sample());
    // Repeat the alloc/free cycle: gpu-allocator keeps the last general block
    // alive, so a saving seen on the first round may not survive churn.
    for round in 1..=3 {
        let held = sample();
        targets.clear();
        let _ = gpu.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(5)),
        });
        let freed = sample();
        report(&format!("GPU free {round}"), held, freed);
        if round < 3 {
            let pre = sample();
            for (_, w, h) in sizes {
                let off =
                    Offscreen::new(&gpu.device, *w, *h, Gpu::COLOR_FORMAT, msaa.unsigned_abs());
                let mut enc = gpu
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("mem_surface"),
                    });
                off.copy_to_buffer(&mut enc);
                let _ = gpu.queue.submit([enc.finish()]);
                let acc = off.with_mapped_pixels(&gpu.device, |data, _| Ok(touch_pages(data)))?;
                std::hint::black_box(acc);
                targets.push(off);
            }
            report(&format!("GPU reacq {round}"), pre, sample());
        }
    }
    Ok(())
}

fn report(label: &str, held: nooksill_gpu::mem::MemStats, freed: nooksill_gpu::mem::MemStats) {
    println!(
        "{label:<18} held {:>7.2}/{:<7.2} freed {:>7.2}/{:<7.2} delta {:>6.2}/{:<6.2} MiB (ws/priv)",
        held.ws_mb(),
        held.private_mb(),
        freed.ws_mb(),
        freed.private_mb(),
        held.ws_mb() - freed.ws_mb(),
        held.private_mb() - freed.private_mb()
    );
}

/// The dock bar keeps its DIB; this is what that decision actually costs.
fn dock_dib(ppp: f32) {
    let cfg = nooksill_core::config::Config::default();
    let dpi = (ppp * 96.0).round() as i32;
    let work = nooksill_core::layout::Rect::new(0, 0, 2560, 1600);
    let metrics = nooksill_core::layout::calc_metrics(&cfg, dpi, 12, 2, work);
    let (lw, lh) = nooksill_core::layout::physical_size(cfg.edge, metrics.lw, metrics.lh);
    println!(
        "\ndock bar {lw}x{lh} -> DIB {:.2} MiB (default config, 12 items, dpi {dpi})",
        mb(u64::from(lw.unsigned_abs()) * u64::from(lh.unsigned_abs()) * 4)
    );
}
