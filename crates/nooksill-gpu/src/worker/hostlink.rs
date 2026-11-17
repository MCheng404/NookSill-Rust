//! Off-UI-thread Host Link commands.
//!
//! `--host-probe` and `--host-activate` are separate GUI processes. Waiting on
//! their stdout pipes from the UI thread freezes the dock when the app is busy
//! or showing a first-run permission prompt, so only a job id crosses here.

use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;
use std::thread;

/// What to run for one published app id.
#[derive(Clone, Debug)]
pub struct HostLinkJob {
    pub id: String,
    pub probe: bool,
    pub activate: bool,
}

#[derive(Clone, Debug)]
pub struct HostLinkOpener {
    tx: Sender<HostLinkJob>,
}

impl HostLinkOpener {
    #[must_use]
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<HostLinkJob>();
        thread::spawn(move || {
            while let Ok(job) = rx.recv() {
                let Some(link) = nooksill_platform::hostlink::link_by_id(&job.id) else {
                    continue;
                };
                let ok = nooksill_platform::hostlink::run_detached(link, job.activate);
                nooksill_platform::hostlink::store_ready(&job.id, ok);
            }
        });
        Self { tx }
    }

    pub fn submit(&self, job: HostLinkJob) {
        let _ = self.tx.send(job);
    }
}

impl Default for HostLinkOpener {
    fn default() -> Self {
        Self::new()
    }
}

static GLOBAL: OnceLock<HostLinkOpener> = OnceLock::new();

/// Queues a job from any UI path. The thread starts on first use.
pub fn submit(id: &str, probe: bool, activate: bool) {
    GLOBAL.get_or_init(HostLinkOpener::new).submit(HostLinkJob {
        id: id.to_owned(),
        probe,
        activate,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn submit_never_blocks_the_ui_thread() {
        let start = Instant::now();
        submit("com.example.absent", true, true);
        assert!(start.elapsed() < Duration::from_millis(50));
    }
}
