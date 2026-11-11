//! The live capture thread and its frame merging.

use super::capture::{color_field, count_diff, dirty_columns, report, trace, ColorField};
use super::*;

/// One captured frame. Bytes are shared with the UI thread, not copied again.
#[derive(Clone, Debug)]
pub struct FrostFrame {
    pub bits: Arc<Vec<u8>>,
    pub w: u32,
    pub h: u32,
    /// Changed columns `[x0, x1)` of the source image; a full frame is `(0, w)`.
    /// The GPU convolution recomputes only these columns, the still area
    /// keeps the previous texture.
    pub x0: u32,
    pub x1: u32,
    /// Low-frequency colour field of the frame, unused by the classic path.
    pub field: Arc<ColorField>,
}

/// Background capture thread for a visible panel.
#[derive(Debug)]
pub struct Live {
    /// Dropping the sender unblocks `recv_timeout` when the panel closes.
    tx: Option<Sender<()>>,
    rx: Receiver<FrostFrame>,
    /// Frame taken by [`Self::has_frame`] but not consumed yet.
    pending: Option<FrostFrame>,
    /// Never joined: a GDI capture may still be in flight at shutdown.
    _thread: JoinHandle<()>,
}

impl Live {
    #[must_use]
    pub fn start(hwnd: HWND) -> Option<Self> {
        let (tx, cmd_rx) = mpsc::channel::<()>();
        let (img_tx, rx) = mpsc::channel::<FrostFrame>();
        let waker = Waker::new(hwnd);
        // `HWND` is a raw pointer; only its numeric value crosses threads.
        let raw = hwnd.0 as isize;
        let thread = std::thread::Builder::new()
            .name("nooksill-frost".to_owned())
            .spawn(move || run(raw, &img_tx, waker, &cmd_rx))
            .ok()?;
        Some(Self {
            tx: Some(tx),
            rx,
            pending: None,
            _thread: thread,
        })
    }

    /// Whether a capture is queued, keeping it for [`Self::latest`].
    ///
    /// The main loop asks this instead of polling: the thread only sends a
    /// frame when the desktop changed, so an idle desktop costs nothing.
    pub(crate) fn has_frame(&mut self) -> bool {
        if self.pending.is_some() {
            return true;
        }
        match self.rx.try_recv() {
            Ok(frame) => {
                self.pending = Some(frame);
                true
            }
            Err(_) => false,
        }
    }

    /// Keeps only the newest frame; a stale background is worse than a drop.
    ///
    /// Dropped frames must merge their dirty columns: a column changed in the
    /// skipped frame but not in the newest one would never be recomputed and
    /// would stay frozen on the background of two frames ago.
    pub(crate) fn latest(&mut self) -> Option<FrostFrame> {
        let mut newest: Option<FrostFrame> = self.pending.take();
        while let Ok(frame) = self.rx.try_recv() {
            newest = Some(match newest {
                Some(prev) => merge_frames(prev, frame),
                None => frame,
            });
        }
        newest
    }
}

/// Merges dropped intermediate frames: newest content, union of dirty columns.
pub(super) fn merge_frames(prev: FrostFrame, frame: FrostFrame) -> FrostFrame {
    let (x0, x1) = if (prev.x0 == 0 && prev.x1 == prev.w) || (frame.x0 == 0 && frame.x1 == frame.w)
    {
        (0, frame.w)
    } else {
        (prev.x0.min(frame.x0), prev.x1.max(frame.x1))
    };
    FrostFrame { x0, x1, ..frame }
}

impl Drop for Live {
    fn drop(&mut self) {
        // Dropping the sender makes the thread's `recv_timeout` return
        // `Disconnected` right away, which ends the loop.
        self.tx = None;
    }
}

/// Cheap probe for "did the desktop change".
///
/// A full capture reads the whole window rect (up to 2560x1600) and backs off
/// to 800 ms while nothing moves, which makes the blur lag half a second
/// behind. This reads 20 pixels from the screen DC every 25 ms and wakes the
/// full capture as soon as they change. The window is excluded from capture
/// by `WDA_EXCLUDEFROMCAPTURE`, so the pixels come from the desktop behind it.
pub(super) struct Probe {
    hdc: HDC,
    last: u32,
}

impl Probe {
    fn new() -> Option<Self> {
        let hdc = unsafe { GetDC(None) };
        if hdc.0.is_null() {
            return None;
        }
        Some(Self { hdc, last: 0 })
    }

    /// Samples a 5x4 grid of pixels and folds them into one checksum.
    /// Returns `true` when the window geometry is unavailable, to err on the
    /// side of an extra capture.
    fn changed(&mut self, hwnd: isize) -> bool {
        let handle = HWND(hwnd as *mut std::ffi::c_void);
        let mut rc = RECT::default();
        if unsafe { GetClientRect(handle, &mut rc) }.is_err() {
            return true;
        }
        let mut origin = POINT {
            x: rc.left,
            y: rc.top,
        };
        if !unsafe { ClientToScreen(handle, &mut origin) }.as_bool() {
            return true;
        }
        let w = (rc.right - rc.left).max(1);
        let h = (rc.bottom - rc.top).max(1);
        let mut acc = 0u32;
        for gy in 0..4 {
            for gx in 0..5 {
                let x = origin.x + w * (gx * 2 + 1) / 10;
                let y = origin.y + h * (gy * 2 + 1) / 8;
                let c = unsafe { GetPixel(self.hdc, x, y) }.0;
                acc = acc.wrapping_mul(31).wrapping_add(c & 0x00ff_ffff);
            }
        }
        let changed = acc != self.last;
        self.last = acc;
        changed
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseDC(None, self.hdc);
        }
    }
}

/// Runs the capture loop. Blur and upload happen on the UI thread in
/// `FrameRenderer`.
pub(super) fn run(hwnd: isize, out: &Sender<FrostFrame>, waker: Waker, stop: &Receiver<()>) {
    let Ok(mut cap) = DesktopCapture::new(true) else {
        return;
    };
    let mut probe = Probe::new();
    let mut prev: Option<Arc<Vec<u8>>> = None;
    let mut dims = (0u32, 0u32);
    let mut interval = LIVE_INTERVAL;
    let mut next_full = Instant::now();
    let mut last_full = Instant::now() - PROBE_FORCE_GAP;
    let mut seq = 0u64;

    loop {
        let now = Instant::now();
        // The probe runs every tick: it is the only way a sudden desktop
        // change is noticed at once.
        let hit = probe.as_mut().is_some_and(|p| p.changed(hwnd));
        let force = now.duration_since(last_full) >= PROBE_FORCE_GAP;
        let can_capture = now.duration_since(last_full) >= MIN_CAPTURE_GAP;
        if now >= next_full || ((hit || force) && can_capture) {
            let started = Instant::now();
            last_full = now;
            if let Ok(Some((bits, w, h))) = cap.capture_behind_pct(hwnd, CAPTURE_PCT) {
                let same = dims == (w, h) && prev.as_deref().is_some_and(|p| p.as_slice() == bits);
                if same {
                    interval = (interval * 2).min(LIVE_IDLE_MAX);
                } else {
                    let (x0, x1) = match prev.as_deref() {
                        Some(p) if dims == (w, h) => dirty_columns(p, bits, w as usize, h as usize),
                        _ => (0, w),
                    };
                    let diff = if trace() {
                        count_diff(prev.as_deref().map_or(&[][..], Vec::as_slice), bits)
                    } else {
                        0
                    };
                    let field = Arc::new(color_field(bits, w as usize, h as usize));
                    dims = (w, h);
                    let snapshot = Arc::new(bits.to_vec());
                    prev = Some(snapshot.clone());
                    interval = LIVE_ACTIVE;
                    if trace() {
                        report(seq, bits, w, h, started.elapsed(), diff);
                        seq += 1;
                    }
                    if out
                        .send(FrostFrame {
                            bits: snapshot,
                            w,
                            h,
                            x0,
                            x1,
                            field,
                        })
                        .is_err()
                    {
                        return;
                    }
                    waker.wake();
                }
            }
            next_full = last_full + interval;
        }

        // Fixed 25 ms tick: the probe drives it, while the full capture is
        // throttled by `next_full`. `recv_timeout` doubles as the stop
        // signal, so the thread exits as soon as the panel closes.
        match stop.recv_timeout(PROBE_INTERVAL) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}
