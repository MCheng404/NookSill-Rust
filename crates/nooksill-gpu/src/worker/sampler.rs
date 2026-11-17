//! System sampler thread: one `SysInfo` snapshot per second off the UI thread.
//!
//! `GetIfTable2`, the Toolhelp snapshot and the PDH query are the expensive
//! calls; keeping them here honours ADR-037 (UI thread only does `try_recv`).
//! Sampling pauses while no resource widget is visible, so an idle dock keeps
//! its zero-frame, zero-poll promise.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nooksill_platform::sysinfo::{self, CpuSampler, NetSampler, PdhSampler};
use nooksill_platform::SysInfo;

use super::Waker;

/// Sample interval while a resource consumer is visible.
const INTERVAL: Duration = Duration::from_secs(1);

/// Poll interval while disabled; costs one atomic load per wake.
const IDLE_POLL: Duration = Duration::from_millis(500);

/// Owns the sampler thread and the newest snapshot channel.
pub struct SysSampler {
    rx: Receiver<SysInfo>,
    enabled: Arc<AtomicBool>,
    _join: JoinHandle<()>,
}

impl std::fmt::Debug for SysSampler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SysSampler")
            .field("enabled", &self.enabled.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl SysSampler {
    pub fn spawn(waker: Waker) -> Self {
        let (tx, rx) = mpsc::channel();
        let enabled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&enabled);
        let join = thread::spawn(move || {
            let mut cpu = CpuSampler::new();
            let mut net = NetSampler::new();
            let mut pdh = PdhSampler::new();
            loop {
                if !flag.load(Ordering::Relaxed) {
                    thread::sleep(IDLE_POLL);
                    continue;
                }
                let info = sysinfo::sample_all(&mut cpu, &mut net, &mut pdh);
                // The receiver is gone: the app is shutting down.
                if tx.send(info).is_err() {
                    break;
                }
                waker.wake();
                thread::sleep(INTERVAL);
            }
        });
        Self {
            rx,
            enabled,
            _join: join,
        }
    }

    /// Enable or pause sampling. Cheap enough to call every frame.
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    /// Newest snapshot since the last call, or `None`.
    pub fn try_latest(&self) -> Option<SysInfo> {
        let mut latest = None;
        while let Ok(info) = self.rx.try_recv() {
            latest = Some(info);
        }
        latest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_sampler_stays_silent() {
        // A zero handle only means the wake call is a no-op; sampling is off.
        let sampler = SysSampler::spawn(Waker::new(windows::Win32::Foundation::HWND(
            std::ptr::null_mut(),
        )));
        sampler.set_enabled(false);
        std::thread::sleep(Duration::from_millis(120));
        assert!(sampler.try_latest().is_none());
    }

    #[test]
    fn enabled_sampler_produces_a_snapshot() {
        let sampler = SysSampler::spawn(Waker::new(windows::Win32::Foundation::HWND(
            std::ptr::null_mut(),
        )));
        sampler.set_enabled(true);
        let mut got = None;
        for _ in 0..30 {
            if let Some(info) = sampler.try_latest() {
                got = Some(info);
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let info = got.expect("采样线程应在一秒内产出快照");
        assert!(info.mem_total_mb > 0);
        sampler.set_enabled(false);
    }
}
