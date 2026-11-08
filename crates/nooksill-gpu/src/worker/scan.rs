//! Background scanner for running apps and occlusion.

use super::*;

/// Input for the scan thread; sent on change only, not every tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanInput {
    /// Rect to test for occlusion; `None` disables the check.
    pub cover: Option<Rect>,
    /// Whether to enumerate running apps. False when the indicator dots and the
    /// running bubble are both hidden, where the sweep would be pure waste.
    pub want_running: bool,
}

/// Output of the scan thread.
#[derive(Clone, Debug, Default)]
pub struct ScanOutput {
    /// Lowercased exe paths of running apps.
    pub running: Option<HashSet<String>>,
    /// Whether the rect is covered by a normal window.
    pub covered: Option<bool>,
}

/// Running-app scan period. Each round is an `EnumWindows` sweep plus one
/// `OpenProcess` per window, so a shorter interval buys nothing.
pub(super) const RUNNING_EVERY: Duration = Duration::from_millis(1200);

/// Occlusion period; matches the 100 ms cursor poll of auto-hide.
pub(super) const COVER_EVERY: Duration = Duration::from_millis(100);

/// Runs the periodic thread for running apps and occlusion.
///
/// Both jobs enumerate top-level windows, so one thread avoids two
/// `EnumWindows` calls fighting over the same lock.
#[derive(Debug)]
pub struct Scanner {
    tx: Option<Sender<ScanInput>>,
    rx: Receiver<ScanOutput>,
    _thread: JoinHandle<()>,
}

impl Scanner {
    /// `exclude` is the dock's own HWND as `isize` (HWND is not `Send`).
    pub fn spawn(exclude: isize, waker: Waker) -> std::io::Result<Self> {
        let (in_tx, in_rx) = mpsc::channel::<ScanInput>();
        let (out_tx, out_rx) = mpsc::channel::<ScanOutput>();
        let thread = thread::Builder::new()
            .name("nooksill-scan".to_owned())
            .spawn(move || scan_thread(in_rx, out_tx, exclude, waker))?;
        Ok(Self {
            tx: Some(in_tx),
            rx: out_rx,
            _thread: thread,
        })
    }

    /// Sends new parameters; the thread keeps the latest one only.
    pub fn update(&self, input: ScanInput) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(input);
        }
    }

    /// Drains finished results without blocking.
    pub fn drain(&self) -> Vec<ScanOutput> {
        self.rx.try_iter().collect()
    }
}

impl Drop for Scanner {
    fn drop(&mut self) {
        // Dropping the sender is enough; the thread exits on its next timeout.
        self.tx = None;
    }
}

pub(super) fn scan_thread(
    rx: Receiver<ScanInput>,
    tx: Sender<ScanOutput>,
    exclude: isize,
    waker: Waker,
) {
    let mut input = ScanInput::default();
    // Scan once at start so the indicator dots do not wait a full period.
    let mut next_running = Instant::now();
    let mut next_cover = Instant::now();

    loop {
        // Check deadlines before blocking on input: a steady stream of
        // parameter updates would otherwise starve the scan loop.
        let now = Instant::now();
        let mut out = ScanOutput::default();
        if now >= next_running {
            next_running = now + RUNNING_EVERY;
            if !input.want_running {
                // Keep the beat even when skipped, so re-enabling yields a
                // result within one period without extra state.
            } else {
                // `running_apps()` returns `HashMap<String, HWND>`; HWND is not
                // `Send`, so convert to a set here.
                out.running = Some(app_window::running_apps().into_keys().collect());
            }
        }
        if now >= next_cover {
            next_cover = now + COVER_EVERY;
            if let Some(rect) = input.cover {
                // Rebuilt only to keep the dock's own window out of the test.
                let me = HWND(exclude as *mut core::ffi::c_void);
                out.covered = Some(occlusion::any_window_covers(rect, &[me]));
            }
        }
        if out.running.is_some() || out.covered.is_some() {
            if tx.send(out).is_err() {
                return;
            }
            waker.wake();
        }

        // Sleep until the nearest deadline; incoming updates overwrite, so the
        // latest value wins without queueing.
        let wait = next_running
            .min(next_cover)
            .saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(next) => input = next,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}
