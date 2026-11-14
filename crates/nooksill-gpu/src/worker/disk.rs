//! Config writer thread.
//!
//! Jobs run one at a time in submission order, so writes stay ordered. The
//! debounce that decides *when* to submit lives in the UI thread.

use super::*;

/// One write request. `None` means "leave that section alone".
#[derive(Clone, Debug)]
pub struct WriteJob {
    pub items: Option<Vec<Item>>,
    pub config: Option<Box<nooksill_core::config::Config>>,
    /// Timer / to-do (`[Tool]` section) plus the timestamp it was captured at.
    /// `Remain` counts down from that instant, not from the write thread.
    pub tool: Option<(Box<nooksill_core::tool::ToolState>, i64)>,
    pub notes: Option<Box<nooksill_core::note::NoteBook>>,
}

/// Config writer thread. Jobs run serially, so writes stay in order.
#[derive(Debug)]
pub struct DiskWriter {
    tx: Option<Sender<WriteJob>>,
    rx: Receiver<Result<(), String>>,
    /// `None` once [`Self::shutdown`] joined it.
    _thread: Option<JoinHandle<()>>,
}

impl DiskWriter {
    pub fn spawn(ini: PathBuf, waker: Waker) -> std::io::Result<Self> {
        let (job_tx, job_rx) = mpsc::channel::<WriteJob>();
        let (res_tx, res_rx) = mpsc::channel::<Result<(), String>>();
        let thread = thread::Builder::new()
            .name("nooksill-disk".to_owned())
            .spawn(move || disk_thread(ini, job_rx, res_tx, waker))?;
        Ok(Self {
            tx: Some(job_tx),
            rx: res_rx,
            _thread: Some(thread),
        })
    }

    pub fn submit(&self, job: WriteJob) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(job);
        }
    }

    /// Drain finished write results without blocking.
    pub fn drain(&self) -> Vec<Result<(), String>> {
        self.rx.try_iter().collect()
    }

    /// Write `job` and wait until it is on disk. For shutdown only.
    ///
    /// The debounce in the main loop can still hold an edit when the user
    /// quits; without this the last change is silently dropped. Dropping the
    /// sender makes the thread exit once the queue is empty, so joining also
    /// waits for jobs submitted before this call.
    pub fn shutdown(&mut self, job: Option<WriteJob>) -> Result<(), String> {
        if let Some(job) = job {
            self.submit(job);
        }
        self.tx = None;
        if let Some(thread) = self._thread.take() {
            let _ = thread.join();
        }
        // Report the outcome of the last write, if any.
        self.rx.try_iter().last().unwrap_or(Ok(()))
    }
}

impl Drop for DiskWriter {
    fn drop(&mut self) {
        self.tx = None;
    }
}

pub(super) fn disk_thread(
    ini: PathBuf,
    rx: Receiver<WriteJob>,
    tx: Sender<Result<(), String>>,
    waker: Waker,
) {
    while let Ok(job) = rx.recv() {
        if tx.send(write(&ini, &job)).is_err() {
            return;
        }
        waker.wake();
    }
}

/// Reads, patches and writes back the file.
///
/// The file must be re-read on every job: the settings window and the item
/// editor both write, so a cached copy would silently win over the newer
/// content.
pub(super) fn write(ini: &Path, job: &WriteJob) -> Result<(), String> {
    let mut model = Ini::load(ini).map_err(|e| format!("读取 {} 失败：{e}", ini.display()))?;
    if let Some(items) = &job.items {
        store::save_items(items, &mut model);
    }
    if let Some(cfg) = &job.config {
        cfg.write_ini(&mut model);
    }
    if let Some((tool, now_ms)) = &job.tool {
        tool.save(&mut model, *now_ms);
    }
    if let Some(notes) = &job.notes {
        notes.save(&mut model);
    }
    model
        .save(ini)
        .map_err(|e| format!("写入 {} 失败：{e}", ini.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nooksill_core::config::{Config, Key};

    /// Shutdown must finish the queued write before returning: a setting
    /// changed a few milliseconds before quitting would otherwise be dropped.
    #[test]
    fn shutdown_writes_the_last_job_before_returning() {
        let dir = std::env::temp_dir().join("nooksill-disk-shutdown-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let ini = dir.join("NookSill.ini");

        // A null window only makes the wake-up fail; writing is unaffected.
        let mut writer =
            DiskWriter::spawn(ini.clone(), Waker::new(HWND::default())).expect("spawn");
        let mut cfg = Config::defaults();
        cfg.apply_i32(Key::Opacity, 42);
        writer
            .shutdown(Some(WriteJob {
                items: None,
                config: Some(Box::new(cfg)),
                tool: None,
                notes: None,
            }))
            .expect("shutdown must report a successful write");

        let saved = Ini::load(&ini).expect("load written ini");
        assert_eq!(
            saved.get_i32("appearance", "opacity"),
            Some(42),
            "退出前的改动必须已经落盘"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
