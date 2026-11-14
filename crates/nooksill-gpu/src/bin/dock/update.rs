//! Self-update: channel selection, daily background checks, staged installs.
//!
//! The worker only sends plain data; translation and window wake-ups belong to
//! the UI thread. Automatic updates never quit the process: the staged binary
//! is swapped in after the normal shutdown path runs.

use super::*;
use nooksill_platform::update::{self, Channel};

/// A background update that sends nothing for this long is abandoned.
///
/// Messages only arrive at start, on progress and at the end, so a panicked
/// worker or a wedged transfer would leave `update_rx` set forever and block
/// every later check. The stamp is refreshed by each message, so a slow but
/// working download is never cut off.
const UPDATE_STALL: Duration = Duration::from_secs(15 * 60);

/// Messages the updater thread sends back to the main thread.
///
/// Every variant carries plain data, the worker knows nothing about the UI
/// and the translation belongs to the main thread.
pub(super) enum UpdateMsg {
    /// One line of progress text.
    Status(String),
    /// Bytes received so far, and the asset size from the release metadata.
    Progress { done: u64, total: u64 },
    /// The daily check finished successfully; store its UTC day stamp.
    Checked(i32),
    /// A new version was found, with the release info the download needs.
    Available(Box<update::Release>),
    /// Already up to date, nothing to download.
    UpToDate,
    /// Download and verification finished, the file waits on disk for the swap.
    Staged(std::path::PathBuf),
    /// Any step failed, the content is the reason shown to the user.
    Failed(String),
}

impl App {
    fn update_channel(&self) -> Channel {
        Channel::from_i32(self.dock.cfg.get_i32(Key::UPD_CHANNEL))
    }

    /// Starts a manual check; it does not advance the daily stamp.
    pub(super) fn start_update_check(&mut self) {
        self.spawn_check(None);
    }

    /// One automatic check per calendar day, started during boot.
    ///
    /// The day stamp is written only after a successful answer: a rate-limited
    /// or offline check must not consume the whole day's retry budget.
    pub(super) fn maybe_auto_check(&mut self) {
        if self.dock.cfg.get_i32(Key::UPD_CHK) == 0 {
            return;
        }
        let day = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| (d.as_secs() / 86_400) as i32)
            .unwrap_or(0);
        if self.dock.cfg.get_i32(Key::UPD_LAST) == day {
            return;
        }
        self.spawn_check(Some(day));
    }

    fn spawn_check(&mut self, day: Option<i32>) {
        if self.update_rx.is_some() {
            return;
        }
        let channel = self.update_channel();
        let (tx, rx) = std::sync::mpsc::channel();
        self.update_rx = Some(rx);
        self.update_started = Some(Instant::now());
        std::thread::spawn(move || {
            let _ = tx.send(UpdateMsg::Status(
                nooksill_core::i18n::t("Checking…").to_owned(),
            ));
            match update::latest_release(channel) {
                Ok(rel) => {
                    if let Some(day) = day {
                        let _ = tx.send(UpdateMsg::Checked(day));
                    }
                    let installed = update::Version::parse(nooksill_core::VERSION);
                    // Channel selection is explicit now: a stable install can
                    // opt into beta, and a beta install can opt out.
                    let offered = installed.is_some_and(|v| update::is_offered(&rel, &v, channel));
                    if offered {
                        let _ = tx.send(UpdateMsg::Available(Box::new(rel)));
                    } else {
                        let _ = tx.send(UpdateMsg::UpToDate);
                    }
                }
                Err(e) => {
                    let _ = tx.send(UpdateMsg::Failed(e.to_string()));
                }
            }
        });
    }

    /// Downloads: picks a mirror by the switches (no direct connection when
    /// the secure DNS check says the name is poisoned), then downloads and
    /// verifies.
    pub(super) fn start_update_download(&mut self) {
        if self.update_rx.is_some() {
            return;
        }
        let Some(release) = self.update_release.clone() else {
            return;
        };
        let secure_dns = self.dock.cfg.get_i32(Key::NET_SECURE_DNS) != 0;
        let test_all = self.dock.cfg.get_i32(Key::NET_MIRROR_ALL) != 0;
        let (tx, rx) = std::sync::mpsc::channel();
        self.update_rx = Some(rx);
        self.update_started = Some(Instant::now());
        std::thread::spawn(move || {
            let allow_direct = !(secure_dns && update::dns_looks_poisoned("github.com"));
            let _ = tx.send(UpdateMsg::Status(if allow_direct {
                nooksill_core::i18n::t("Benchmarking mirrors…").to_owned()
            } else {
                nooksill_core::i18n::t("System DNS looks poisoned; using mirrors only…").to_owned()
            }));
            // The exe asset is the download target; the checksum asset is only
            // a fallback when the API has no digest.
            let base = update::pick_mirror(&release.exe.url, test_all, allow_direct);
            let Some(base) = base else {
                let _ = tx.send(UpdateMsg::Failed(
                    nooksill_core::i18n::t("No usable mirror").to_owned(),
                ));
                return;
            };
            let label = if base.is_empty() {
                nooksill_core::i18n::t("GitHub direct")
            } else {
                base.as_str()
            };
            let _ = tx.send(UpdateMsg::Status(nooksill_core::i18n::fmt(
                "Downloading from {0}…",
                &[label],
            )));
            match update::stage_with_progress(&base, &release, &mut |done, total| {
                let _ = tx.send(UpdateMsg::Progress { done, total });
            }) {
                Ok(path) => {
                    let _ = tx.send(UpdateMsg::Staged(path));
                }
                Err(e) => {
                    let _ = tx.send(UpdateMsg::Failed(e.to_string()));
                }
            }
        });
    }

    /// Moves the background thread messages into the settings window state.
    pub(super) fn pump_update(&mut self) {
        // A thread that panicked or a transfer that wedged sends nothing; the
        // channel alone would then stay set for the rest of the session and
        // every later check or download would return early.
        if self.update_rx.is_some()
            && self
                .update_started
                .is_some_and(|at| at.elapsed() >= UPDATE_STALL)
        {
            self.update_rx = None;
            self.update_started = None;
            let text = nooksill_core::i18n::fmt(
                "Failed: {0}",
                &[nooksill_core::i18n::t("The update timed out")],
            );
            if let Some(sw) = self.settings_slot.as_mut() {
                sw.state.update.status = text;
                sw.state.update.busy = false;
                sw.wake();
            }
            return;
        }
        let Some(rx) = self.update_rx.as_ref() else {
            return;
        };
        let mut messages = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            messages.push(msg);
        }
        if messages.is_empty() {
            return;
        }
        // Any message means the worker is alive and making progress.
        self.update_started = Some(Instant::now());

        let auto = self.dock.cfg.get_i32(Key::UPD_AUTO) != 0;
        let mut finished = false;
        let mut auto_download = false;
        for msg in messages {
            let mut view = self
                .settings_slot
                .as_ref()
                .map(|sw| sw.state.update.clone())
                .unwrap_or_default();
            match msg {
                UpdateMsg::Checked(day) => {
                    self.dock.cfg.apply_i32(Key::UPD_LAST, day);
                    self.touch_cfg();
                }
                UpdateMsg::Status(text) => {
                    view.status = text;
                    view.busy = true;
                }
                UpdateMsg::Progress { done, total } => {
                    let now = Instant::now();
                    match self.update_rate {
                        Some((at, bytes)) => {
                            let dt = now.duration_since(at).as_secs_f32();
                            // 250 ms keeps the number readable instead of
                            // jittering on every 256 KiB chunk.
                            if dt >= 0.25 {
                                let delta = done.saturating_sub(bytes);
                                let rate = (delta as f32 / dt) as u64;
                                view.bytes_per_sec = rate;
                                view.eta_secs = if rate > 0 && total > done {
                                    ((total - done) / rate).min(u64::from(u32::MAX)) as u32
                                } else {
                                    0
                                };
                                self.update_rate = Some((now, done));
                            }
                        }
                        None => self.update_rate = Some((now, done)),
                    }
                    view.bytes_done = done;
                    view.bytes_total = total;
                    view.busy = true;
                }
                UpdateMsg::Available(release) => {
                    view.bytes_done = 0;
                    view.bytes_total = 0;
                    view.bytes_per_sec = 0;
                    view.eta_secs = 0;
                    self.update_rate = None;
                    let kind = if release.prerelease {
                        nooksill_core::i18n::t("New pre-release")
                    } else {
                        nooksill_core::i18n::t("New version")
                    };
                    view.status = nooksill_core::i18n::fmt(
                        "Found {0} {1}",
                        &[kind, release.version.text.as_str()],
                    );
                    view.version = release.version.text.clone();
                    view.notes = release.notes.clone();
                    view.prerelease = release.prerelease;
                    view.available = true;
                    view.busy = false;
                    self.update_release = Some(*release);
                    // A new offer is a new reason to prompt, but automatic
                    // updates skip the popup entirely.
                    if !auto {
                        if let Some(sw) = self.settings_slot.as_mut() {
                            sw.state.update_prompted = false;
                        }
                    }
                    auto_download = auto;
                    finished = true;
                }
                UpdateMsg::UpToDate => {
                    view.status = nooksill_core::i18n::t("Already up to date").to_owned();
                    view.available = false;
                    view.busy = false;
                    finished = true;
                }
                UpdateMsg::Staged(path) => {
                    view.status = if auto {
                        nooksill_core::i18n::t("Downloaded and verified; applies on next start")
                    } else {
                        nooksill_core::i18n::t("Downloaded and verified; applies after restart")
                    }
                    .to_owned();
                    view.ready = true;
                    view.busy = false;
                    self.update_staged = Some(path.clone());
                    // Always queue the exit-time swap. Closing the window
                    // without pressing "restart and update" must still install
                    // the verified file; apply_update() overrides this with a
                    // restart when the user asks for one.
                    self.pending_update = Some(path);
                    self.pending_silent = true;
                    self.update_rate = None;
                    if !auto {
                        if let Some(sw) = self.settings_slot.as_mut() {
                            sw.state.update_ready_prompted = false;
                        }
                    }
                    finished = true;
                }
                UpdateMsg::Failed(why) => {
                    self.update_rate = None;
                    view.status = nooksill_core::i18n::fmt("Failed: {0}", &[why.as_str()]);
                    view.busy = false;
                    finished = true;
                }
            }
            if let Some(sw) = self.settings_slot.as_mut() {
                sw.state.update = view;
                sw.wake();
            }
        }
        if finished {
            self.update_rx = None;
            self.update_started = None;
        }
        if auto_download {
            self.start_update_download();
        }
    }

    /// Replaces the running binary with the downloaded file and restarts.
    /// Does not return on success, the process exits.
    pub(super) fn apply_update(&mut self) {
        let Some(staged) = self.update_staged.clone() else {
            return;
        };
        // Only records the intent and quits the loop. The real swap happens in
        // `main` after `App` is dropped: `process::exit` skips the
        // destructors, so the AppBar ABM_REMOVE and the config write are lost.
        self.pending_update = Some(staged);
        self.pending_silent = false;
        // SAFETY: `PostQuitMessage` only queues a WM_QUIT on this thread.
        unsafe { PostQuitMessage(0) };
    }
}
