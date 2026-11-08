//! Background workers for the blocking Win32 and IO paths.
//!
//! One thread per kind of work: shell icon fetches must be serial, the scan
//! is periodic, disk writes must stay ordered. Results come back over
//! `std::sync::mpsc` and the UI thread only ever does `try_recv`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use nooksill_core::icon::{apply_round_mask, fit_icon, FitMode, IconFit};
use nooksill_core::ini::Ini;
use nooksill_core::item::{Item, ItemKind};
use nooksill_core::layout::Rect;
use nooksill_core::running;
use nooksill_core::store;
use nooksill_platform::app_window;
use nooksill_platform::icon::{self, IconBitmap};
use nooksill_platform::launch;
use nooksill_platform::occlusion;
use nooksill_platform::win::ensure_com;
use rayon::prelude::*;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::gpu::Gpu;
use crate::shaders::icon::IconShader;

/// Custom message meaning "a background result is ready".
///
/// The main loop uses it to break out of its wait, never to force a frame:
/// an extra render per scan result would break the "idle at zero frames"
/// promise.
pub const WM_APP_WAKE: u32 = WM_APP + 1;

/// Wake handle for the UI thread, copyable across threads.
///
/// Without it the main loop would have to poll for results; with it
/// `MsgWaitForMultipleObjectsEx` returns as soon as one arrives and sleeps
/// otherwise.
#[derive(Clone, Copy, Debug)]
pub struct Waker(isize);

impl Waker {
    #[must_use]
    pub fn new(hwnd: HWND) -> Self {
        Self(hwnd.0 as isize)
    }

    pub(crate) fn wake(self) {
        // Failure only means the window is gone (the process is exiting).
        let hwnd = HWND(self.0 as *mut core::ffi::c_void);
        // SAFETY: `PostMessageW` returns an error for a destroyed handle
        // instead of crashing; the result is deliberately ignored (see above).
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_APP_WAKE, WPARAM(0), LPARAM(0));
        }
    }
}

/// One icon request. Every field is self-contained: the worker has no access
/// to `Dock`.
#[derive(Clone, Debug)]
pub struct IconRequest {
    pub index: usize,
    pub kind: ItemKind,
    pub path: String,
    /// Custom icon file (`.ico`); empty means take the icon from `path`.
    pub icon_src: String,
    /// Icon fill mode (0 = aspect fit with padding, see `FitBitmap`).
    pub fit: i32,
    /// Children of a group; the first 4 make up its 2x2 grid icon. Always
    /// empty for other kinds.
    pub kids: Vec<IconKid>,
}

/// One child carried along with a group icon request.
#[derive(Clone, Debug)]
pub struct IconKid {
    pub kind: ItemKind,
    pub path: String,
    /// Custom icon file (`.ico`); empty means take the icon from `path`.
    pub icon_src: String,
}

/// An icon that is ready for texture upload.
///
/// Scaling and the rounded mask are already applied on the worker thread, so
/// the UI thread only has to call `load_texture`, which needs an
/// `egui::Context`.
#[derive(Clone, Debug)]
pub struct ReadyIcon {
    pub index: usize,
    /// Path of the item at request time. The UI thread checks that this index
    /// still points at the same item: the list may have changed while the
    /// request was queued.
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub fit: IconFit,
}

/// Thread that loads icons.
#[derive(Debug)]
pub struct IconLoader {
    tx: Option<Sender<IconRequest>>,
    rx: Receiver<ReadyIcon>,
    /// Keys of the queued tasks, `(index, path)`.
    ///
    /// Items that can never yield an icon (URLs, dead `.lnk` files, images
    /// without a decoder) would otherwise be queued again on every list
    /// change. Keys left in this set suppress those retries; a successful
    /// fetch clears its key in `drain`.
    ///
    /// The index is part of the key because group items share an empty
    /// `path`: a path-only key would let the first group suppress the others.
    inflight: Mutex<HashSet<(usize, String)>>,
    /// Kept for its name only; the thread is not joined on drop.
    _thread: JoinHandle<()>,
}

impl IconLoader {
    /// Starts the icon thread. Returns an error if the thread cannot be
    /// created, letting the caller fall back to synchronous loading.
    pub fn spawn(waker: Waker, gpu: Option<&Gpu>) -> std::io::Result<Self> {
        let shader: Option<Arc<IconShader>> =
            gpu.map(|g| Arc::new(IconShader::new(&g.device, &g.queue)));
        let (req_tx, req_rx) = mpsc::channel::<IconRequest>();
        let (res_tx, res_rx) = mpsc::channel::<ReadyIcon>();
        let thread = thread::Builder::new()
            .name("nooksill-icons".to_owned())
            .spawn(move || icon_thread(req_rx, res_tx, waker, shader))?;
        Ok(Self {
            tx: Some(req_tx),
            rx: res_rx,
            inflight: Mutex::new(HashSet::new()),
            _thread: thread,
        })
    }

    /// Queues one icon request.
    ///
    /// The same path is queued only once until its result comes back. Requests
    /// are dropped silently when the queue is full or the thread is gone: a
    /// missing icon only means falling back to the letter placeholder.
    pub fn submit(&self, req: IconRequest) {
        // The lock is held only here and in `drain`, never across a Win32
        // call; a poisoned lock is recovered so one panic cannot lock the
        // icon path for good.
        let mut set = match self.inflight.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if !set.insert((req.index, req.path.clone())) {
            return;
        }
        drop(set);
        if let Some(tx) = &self.tx {
            let _ = tx.send(req);
        }
    }

    /// Makes one item requestable again (icon source changed).
    pub fn forget(&self, index: usize, path: &str) {
        let mut set = match self.inflight.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        set.remove(&(index, path.to_owned()));
    }

    /// Takes the icons finished in this round without blocking. Successfully
    /// loaded paths leave the in-flight set.
    pub fn drain(&self) -> Vec<ReadyIcon> {
        let out: Vec<ReadyIcon> = self.rx.try_iter().collect();
        if !out.is_empty() {
            let mut set = match self.inflight.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            for r in &out {
                set.remove(&(r.index, r.path.clone()));
            }
        }
        out
    }

    /// Number of suppressed keys (tests only).
    #[cfg(test)]
    fn inflight_len(&self) -> usize {
        self.inflight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

impl Drop for IconLoader {
    fn drop(&mut self) {
        // Dropping the sender makes `recv` fail, so the worker exits after
        // the request in flight. The thread is not joined: a shell call can
        // block (network share, antivirus) and waiting for it would hold up
        // process exit. It only touches its own data, so detaching is safe.
        self.tx = None;
    }
}

mod disk;
mod icons;
mod open;
mod scan;
use self::icons::icon_thread;

pub use self::disk::{DiskWriter, WriteJob};
pub use self::open::{OpenJob, Opener};
pub use self::scan::{ScanInput, ScanOutput, Scanner};

#[cfg(test)]
mod tests {
    use super::disk::{write, DiskWriter, WriteJob};
    use super::icons::process;
    use super::scan::Scanner;
    use super::*;
    use nooksill_core::config::Config;

    /// Writing `[Items]` must not touch `[Settings]`, and vice versa: both
    /// paths share one file, so a full in-memory overwrite would drop the
    /// other side's changes.
    #[test]
    fn write_merges_instead_of_overwriting() {
        let dir = std::env::temp_dir().join("nooksill_worker_write");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("NookSill.ini");

        let mut ini = Ini::new();
        ini.set("Settings", "Opacity", "55");
        ini.set("Items", "Count", "1");
        ini.set("Items", "I0", r"0|0|旧|C:\old.exe||0|");
        ini.set("Future", "Keep", "me");
        ini.save(&path).expect("准备文件");

        // Settings only: items and unknown sections must survive.
        let cfg = Config::defaults();
        write(
            &path,
            &WriteJob {
                items: None,
                config: Some(Box::new(cfg)),
                tool: None,
                notes: None,
            },
        )
        .expect("写设置");
        let after = Ini::load(&path).expect("读回");
        assert_eq!(after.get("Items", "I0"), Some(r"0|0|旧|C:\old.exe||0|"));
        assert_eq!(after.get("Future", "Keep"), Some("me"));
        assert_eq!(
            after.get("Settings", "Opacity"),
            Some("55"),
            "设置段应当被本次写入覆盖"
        );

        // Items only: the settings and unknown sections must survive.
        write(
            &path,
            &WriteJob {
                items: Some(vec![Item::new(ItemKind::File, "新", r"C:\new.exe")]),
                config: None,
                tool: None,
                notes: None,
            },
        )
        .expect("写条目");
        let after = Ini::load(&path).expect("读回");
        assert_eq!(after.get("Future", "Keep"), Some("me"));
        assert_eq!(
            after.get("Items", "I0").map(|s| s.contains("new.exe")),
            Some(true)
        );
        assert!(
            after.get("Settings", "Opacity").is_some(),
            "设置段不该被条目写入抹掉"
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    /// Icon requests are deduplicated per slot: the same index and path queue
    /// once, groups (empty path) do not suppress each other, and `forget`
    /// lets an item with a new icon queue again.
    ///
    /// Guards against a path-only dedup key: all groups would share the empty
    /// string and only the first would get its 2x2 icon.
    #[test]
    fn icon_requests_are_deduplicated_per_slot() {
        let loader = IconLoader::spawn(Waker::new(HWND::default()), None).expect("起线程");
        let req = |index, path: &str| IconRequest {
            index,
            kind: ItemKind::File,
            path: path.to_owned(),
            icon_src: String::new(),
            fit: 0,
            kids: Vec::new(),
        };

        loader.submit(req(0, r"C:\a.exe"));
        loader.submit(req(0, r"C:\a.exe")); // exact duplicate
        assert_eq!(loader.inflight_len(), 1);

        // Group paths are empty strings, so two groups must queue at once.
        loader.submit(req(1, ""));
        loader.submit(req(2, ""));
        assert_eq!(loader.inflight_len(), 3, "不同下标不该被抑制");

        loader.forget(0, r"C:\a.exe");
        assert_eq!(loader.inflight_len(), 2, "forget 要解绑");
        loader.submit(req(0, r"C:\a.exe"));
        assert_eq!(loader.inflight_len(), 3, "解绑后能再排一次");
    }

    /// Runs a shell icon fetch on the worker thread.
    ///
    /// `SHGetImageList` hands out shell COM objects, so COM must be
    /// initialised on this thread; single-threaded code hid the problem, but
    /// on a worker it shows up as "the icon is sometimes missing".
    ///
    /// `#[ignore]`: it calls the shell and fails without an interactive
    /// session (some CI). Run it manually with
    /// `cargo test -p nooksill-gpu -- --ignored`.
    #[test]
    #[ignore = "要调 shell，手动运行"]
    fn icon_loader_extracts_on_a_worker_thread() {
        let loader = IconLoader::spawn(Waker::new(HWND::default()), None).expect("起线程");
        loader.submit(IconRequest {
            index: 0,
            kind: ItemKind::File,
            icon_src: String::new(),
            path: r"C:\Windows\System32\notepad.exe".to_owned(),
            fit: 0,
            kids: Vec::new(),
        });

        let mut got = Vec::new();
        for _ in 0..500 {
            got = loader.drain();
            if !got.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(got.len(), 1, "应当收到一张图标");
        assert_eq!(got[0].index, 0);
        assert!(
            got[0].width > 0 && got[0].height > 0,
            "记事本一定有图标，取不到说明工作线程的 COM 没初始化好"
        );
    }

    /// The scanner must really enumerate something in the background.
    #[test]
    #[ignore = "依赖桌面会话，手动运行"]
    fn scanner_reports_running_apps() {
        let scanner = Scanner::spawn(0, Waker::new(HWND::default())).expect("起线程");
        let mut saw_running = false;
        for _ in 0..200 {
            if scanner.drain().iter().any(|o| o.running.is_some()) {
                saw_running = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(saw_running, "扫描线程应当在 2 秒内给出一次运行状态");
    }

    /// End-to-end disk writer: start the thread, submit a job, collect the
    /// result.
    ///
    /// Covers the wiring (channel, thread start, result hand-off), not the
    /// file content itself, which `write_merges_instead_of_overwriting`
    /// covers. The `Waker` uses a null handle so `PostMessageW` fails, which
    /// also proves a failed wake does not affect the result.
    #[test]
    fn disk_writer_round_trips_a_job() {
        let dir = std::env::temp_dir().join("nooksill_worker_roundtrip");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("NookSill.ini");

        let writer = DiskWriter::spawn(path.clone(), Waker::new(HWND::default())).expect("起线程");
        writer.submit(WriteJob {
            items: Some(vec![Item::new(ItemKind::File, "a", r"C:.exe")]),
            config: None,
            tool: None,
            notes: None,
        });

        // Wait up to 2 s; the normal latency is milliseconds.
        let mut got = Vec::new();
        for _ in 0..200 {
            got = writer.drain();
            if !got.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(got.len(), 1, "应当收到一条写盘结果");
        assert!(got[0].is_ok(), "写盘应当成功: {got:?}");
        assert!(path.exists(), "文件应当已生成");

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    /// Icon processing keeps the index and path: the UI thread uses both to
    /// check who the icon is for.
    #[test]
    fn process_keeps_index_and_path() {
        let req = IconRequest {
            index: 3,
            kind: ItemKind::File,
            path: r"C:\x\a.exe".to_owned(),
            icon_src: String::new(),
            fit: 0,
            kids: Vec::new(),
        };
        let bmp = IconBitmap {
            width: 2,
            height: 2,
            rgba: vec![255u8; 16],
        };
        let ready = process(&req, bmp, None);
        assert_eq!(ready.index, 3);
        assert_eq!(ready.path, r"C:\x\a.exe");
        assert_eq!((ready.width, ready.height), (2, 2));
        assert_eq!(ready.rgba.len(), 16, "圆角遮罩不该改变缓冲区长度");
    }
}
