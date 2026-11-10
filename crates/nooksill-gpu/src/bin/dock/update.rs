//! Self-update: checking, downloading and staging a new build.

use super::*;

/// Messages the updater thread sends back to the main thread.
///
/// Every variant carries plain data, the worker knows nothing about the UI
/// and the translation belongs to the main thread.
pub(super) enum UpdateMsg {
    /// One line of progress text.
    Status(String),
    /// A new version was found, with the release info the download needs.
    Available(Box<nooksill_platform::update::Release>),
    /// Already up to date, nothing to download.
    UpToDate,
    /// Download and verification finished, the file waits on disk for the swap.
    Staged(std::path::PathBuf),
    /// Any step failed, the content is the reason shown to the user.
    Failed(String),
}

impl App {
    /// Starts the update check on a thread and returns to the main loop.
    pub(super) fn start_update_check(&mut self) {
        if self.update_rx.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.update_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(UpdateMsg::Status(
                nooksill_core::i18n::t("Checking…").to_owned(),
            ));
            match nooksill_platform::update::latest_release() {
                Ok(rel) => {
                    let installed =
                        nooksill_platform::update::Version::parse(nooksill_core::VERSION);
                    // Channel rule: a stable release never offers a
                    // prerelease (see `update::is_offered`).
                    if installed.is_some_and(|v| nooksill_platform::update::is_offered(&rel, &v)) {
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
        std::thread::spawn(move || {
            let allow_direct =
                !(secure_dns && nooksill_platform::update::dns_looks_poisoned("github.com"));
            let _ = tx.send(UpdateMsg::Status(if allow_direct {
                nooksill_core::i18n::t("Benchmarking mirrors…").to_owned()
            } else {
                nooksill_core::i18n::t("System DNS looks poisoned; using mirrors only…").to_owned()
            }));
            let base =
                nooksill_platform::update::pick_mirror(&release.sha256.url, test_all, allow_direct);
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
                &[label.to_string().as_str()],
            )));
            match nooksill_platform::update::stage(&base, &release) {
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

        let mut finished = false;
        for msg in messages {
            let mut view = self
                .settings_slot
                .as_ref()
                .map(|sw| sw.state.update.clone())
                .unwrap_or_default();
            match msg {
                UpdateMsg::Status(text) => {
                    view.status = text;
                    view.busy = true;
                }
                UpdateMsg::Available(release) => {
                    let kind = if release.prerelease {
                        nooksill_core::i18n::t("New pre-release")
                    } else {
                        nooksill_core::i18n::t("New version")
                    };
                    view.status = nooksill_core::i18n::fmt(
                        "Found {0} {1}",
                        &[
                            kind.to_string().as_str(),
                            release.version.text.to_string().as_str(),
                        ],
                    );
                    view.available = true;
                    view.busy = false;
                    self.update_release = Some(*release);
                    finished = true;
                }
                UpdateMsg::UpToDate => {
                    view.status = nooksill_core::i18n::t("Already up to date").to_owned();
                    view.available = false;
                    view.busy = false;
                    finished = true;
                }
                UpdateMsg::Staged(path) => {
                    view.status =
                        nooksill_core::i18n::t("Downloaded and verified; applies after restart")
                            .to_owned();
                    view.ready = true;
                    view.busy = false;
                    self.update_staged = Some(path);
                    finished = true;
                }
                UpdateMsg::Failed(why) => {
                    view.status =
                        nooksill_core::i18n::fmt("Failed: {0}", &[why.to_string().as_str()]);
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
        // SAFETY: `PostQuitMessage` only queues a WM_QUIT on this thread.
        unsafe { PostQuitMessage(0) };
    }
}
