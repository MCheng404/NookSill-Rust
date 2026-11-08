//! Launch requests: the opener thread and post-launch follow-up.

use super::*;

/// A request to open one item.
#[derive(Clone, Debug)]
pub struct OpenJob {
    pub item: Box<Item>,
    /// Shift-click or middle-click: start a new instance even if one runs.
    pub force: bool,
    /// `ClickToggle=1`: minimize when already in the foreground.
    pub toggle: bool,
    /// `Browser` key for web apps (0 auto / 1 Edge / 2 Chrome / 3 Firefox /
    /// 4 system default); ignored for non-URL items.
    pub browser: i32,
}

/// Runs the launcher thread.
///
/// `ShellExecuteW` takes tens of ms on a cold start, so it must stay off the
/// UI thread.
#[derive(Debug)]
pub struct Opener {
    tx: Option<Sender<OpenJob>>,
    _thread: JoinHandle<()>,
}

impl Opener {
    pub fn spawn() -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel::<OpenJob>();
        let thread = thread::Builder::new()
            .name("nooksill-open".to_owned())
            .spawn(move || open_thread(rx))?;
        Ok(Self {
            tx: Some(tx),
            _thread: thread,
        })
    }

    pub fn submit(&self, job: OpenJob) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(job);
        }
    }
}

impl Drop for Opener {
    fn drop(&mut self) {
        self.tx = None;
    }
}

pub(super) fn open_thread(rx: Receiver<OpenJob>) {
    while let Ok(job) = rx.recv() {
        let item = &job.item;
        let key = running::running_key(item);
        // Same as a plain click: activate if running, else launch. A refused
        // foreground call still counts as running, so this stays two steps
        // instead of `is_some_and(activate)`.
        if !job.force {
            if let Some(exe) = &key {
                let r = app_window::activate(exe, job.toggle);
                if trace() {
                    eprintln!("[launch] 激活 {} → {r:?}", item.name);
                }
                if r.is_running() {
                    continue;
                }
            }
        }
        if let Err(e) = launch::launch(item, job.browser) {
            eprintln!("启动「{}」失败：{e}", item.name);
            continue;
        }
        if trace() {
            eprintln!("[launch] 启动 {}（force={}）", item.name, job.force);
        }
        // Bring the new window forward. `ShellExecuteW` is async and the dock
        // is `WS_EX_NOACTIVATE`, so the click grants no foreground rights.
        //
        // The wait runs on a detached thread: this queue is serial, and a 3 s
        // wait here would stall later clicks. `force` never takes focus.
        if !job.force {
            if let Some(exe) = key {
                let name = item.name.clone();
                std::thread::spawn(move || follow_launch(&name, &exe));
            }
        }
    }
}

/// Waits for the window to appear, then raises it; gives up after [`FOLLOW_MS`].
///
/// Only used after a launch, for windows the foreground lock kept behind.
pub(super) fn follow_launch(name: &str, exe: &str) {
    let start = Instant::now();
    while start.elapsed() < FOLLOW_MS {
        std::thread::sleep(std::time::Duration::from_millis(120));
        if app_window::find_window_of(exe).is_none() {
            continue;
        }
        let r = app_window::activate(exe, false);
        if trace() {
            eprintln!("[launch] {name} 的窗口已出现 → {r:?}");
        }
        return;
    }
    if trace() {
        eprintln!("[launch] {name} 在 {:?} 内没等到窗口", FOLLOW_MS);
    }
}

/// Upper bound for the post-launch wait. Cold starts take 1-2 s; longer means
/// the program probably never opens a window at all.
pub(super) const FOLLOW_MS: Duration = Duration::from_millis(2400);

/// Reports whether launch tracing is on stderr (`NOOKSILL_LAUNCH_TRACE=1`).
pub(super) fn trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NOOKSILL_LAUNCH_TRACE").is_ok_and(|v| v.trim() != "0"))
}
