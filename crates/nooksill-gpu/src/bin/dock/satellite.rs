//! Lazily created satellite windows: launchpad and settings.

use super::*;

/// Build the launchpad window on first open, reuse it afterwards.
///
/// Lazy creation keeps the tens of milliseconds of font loading and GPU
/// pipeline compilation off the dock's startup; most sessions never open it.
pub(super) fn ensure_launchpad<'a>(
    gpu: &Gpu,
    slot: &'a mut Option<Pin<Box<LaunchpadWindow>>>,
    paths: &Paths,
    ppp: f32,
    dark: bool,
) -> Result<&'a mut LaunchpadWindow, Box<dyn std::error::Error>> {
    if slot.is_none() {
        let dat = paths.dir.join("launchpad.dat");
        *slot = Some(LaunchpadWindow::new(gpu, ppp, dark, &dat)?);
    }
    // An error points at the logic bug better than a panic.
    let lp = slot.as_mut().ok_or("launchpad window not initialised")?;
    Ok(&mut **lp)
}

pub(super) fn ensure_settings<'a>(
    gpu: &Gpu,
    slot: &'a mut Option<Pin<Box<SettingsWindow>>>,
    paths: &Paths,
    ppp: f32,
) -> Result<&'a mut SettingsWindow, Box<dyn std::error::Error>> {
    if slot.is_none() {
        let monitors = display::monitors();
        let state = SettingsState {
            config_dir: paths.dir.display().to_string(),
            monitors: monitors
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    if i == 0 {
                        nooksill_core::i18n::t("Primary monitor").to_owned()
                    } else {
                        nooksill_core::i18n::fmt("Monitor {0}", &[(i + 1).to_string().as_str()])
                    }
                })
                .collect(),
            // Show the real registry value, not the config: the user may change
            // it in Task Manager.
            auto_start: theme::get_autostart(),
            // The software rasterizer is excluded: it only produces frames tens
            // of milliseconds apart.
            adapters: nooksill_platform::gpu::adapters()
                .into_iter()
                .filter(|a| a.kind != nooksill_platform::AdapterKind::Software)
                .map(|a| a.name)
                .collect(),
            ..SettingsState::default()
        };
        // `new` returns a pinned box: the window proc finds it by address, which
        // must never move.
        *slot = Some(SettingsWindow::new(gpu, state, ppp)?);
    }
    // An error points at the logic bug better than a panic.
    let sw = slot.as_mut().ok_or("settings window not initialised")?;
    // Each open is a fresh chance to show a pending update; dismissing the
    // popup only suppresses it for the current window session.
    sw.state.update_prompted = false;
    sw.state.update_ready_prompted = false;
    Ok(&mut **sw)
}

/// Give the settings window one pass per round: render a frame and take its
/// command.
///
/// Returns `(changed the config directly, pending command)`. The caller runs the
/// command because it edits `cfg` and items while this function only borrows
/// `dock`; passing all of `App` would couple it to the main loop.
pub(super) fn tick_settings(
    gpu: &Gpu,
    slot: &mut Option<Pin<Box<SettingsWindow>>>,
    dock: &mut Dock,
    items: &[Item],
) -> Result<(bool, Option<Command>), Box<dyn std::error::Error>> {
    let Some(sw) = slot.as_mut() else {
        return Ok((false, None));
    };
    // Adapter hint: the device is created once at startup, so editing the ini
    // at runtime never switches cards. Sync both fields each round and nudge a
    // frame on change so the hint is painted.
    let active = gpu.info.name.clone();
    let note = if !gpu.selection.matches_config(&dock.cfg.adapter, &active) {
        if dock.cfg.adapter.is_empty() {
            nooksill_core::i18n::t("Switched to automatic; applies on next start").to_owned()
        } else {
            nooksill_core::i18n::fmt(
                "Selected {0}; applies on next start",
                &[dock.cfg.adapter.to_string().as_str()],
            )
        }
    } else if gpu.selection.fell_back() {
        gpu.selection.describe()
    } else {
        String::new()
    };
    if sw.state.active_adapter != active || sw.state.adapter_note != note {
        sw.state.active_adapter = active;
        sw.state.adapter_note = note;
        sw.wake();
    }
    let mut changed = false;
    let mut cmd = None;
    if sw.visible() && sw.due() {
        // Push the item list in: the settings UI holds a snapshot while the
        // source of truth stays `raw` here. Without this the items page is empty.
        sw.state.items = items.to_vec();
        // `cfg` is the very object the dock uses; editing it here edits the dock
        // with no IPC, no copy and no save-and-restart.
        sw.render(gpu, &mut dock.cfg)?;
        let out = std::mem::take(&mut sw.outcome);
        changed = out.changed;
        cmd = out.command;

        // Custom scale: rebuild the window at the new DPI in place. Runs after
        // render, so a dropdown change applies on the next frame.
        let want = settings_win::effective_ppp(&dock.cfg);
        if (sw.ppp() - want).abs() > f32::EPSILON {
            sw.rescale(gpu, want)?;
        }
    }
    // Both Esc and Close set this flag. The command is already taken and is
    // settled by the caller after hiding, which does not affect it.
    if sw.close_requested {
        sw.hide();
    }
    Ok((changed, cmd))
}
