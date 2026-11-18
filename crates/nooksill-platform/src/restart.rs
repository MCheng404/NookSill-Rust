//! Relaunch the dock once this process has exited.
//!
//! The single-instance mutex is global, so a new process started while this one
//! is still alive would only summon the old window and quit (ADR-015). A
//! detached `cmd` therefore waits for the exe to leave the process list first,
//! the same handshake the update swap script uses.
//!
//! Call this only after `App` is dropped: the mutex handle, the AppBar
//! reservation and the pending config write are all released there, and the new
//! process must read the config the old one just flushed.

use std::path::Path;

/// Spawn the waiter and return. The caller still has to exit.
pub fn after_exit() -> Result<(), std::io::Error> {
    let current = std::env::current_exe()?;
    // Distinct from nooksill-update.cmd so an update and a restart cannot
    // overwrite each other's script.
    let script = std::env::temp_dir().join("nooksill-restart.cmd");
    std::fs::write(&script, script_body(&current))?;
    std::process::Command::new("cmd")
        .args(["/C", "start", "", "/min"])
        .arg(&script)
        .spawn()?;
    Ok(())
}

/// Pure so the wait loop can be asserted without spawning `cmd`.
pub(crate) fn script_body(current: &Path) -> String {
    let exe_name = current
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("nooksill-dock.exe");
    format!(
        "@echo off\r\n\
         :wait\r\n\
         tasklist /FI \"IMAGENAME eq {exe_name}\" 2>nul | find /I \"{exe_name}\" >nul\r\n\
         if not errorlevel 1 (\r\n\
         \tping -n 2 127.0.0.1 >nul\r\n\
         \tgoto wait\r\n\
         )\r\n\
         start \"\" \"{current}\"\r\n\
         del \"%~f0\"\r\n",
        current = current.display(),
    )
}

#[cfg(test)]
mod tests {
    use super::script_body;
    use std::path::Path;

    /// The relaunch must come after the wait loop, or the new instance hits the
    /// mutex still held by the old one and only raises its window.
    #[test]
    fn the_relaunch_waits_for_the_old_process_to_leave() {
        let body = script_body(Path::new(r"C:\Dock\NookSill.exe"));
        let wait = body.find(":wait").expect("wait label");
        let start = body.find("start \"\"").expect("relaunch");
        assert!(wait < start, "relaunch must follow the wait loop");
        assert!(body.contains("goto wait"), "the loop must repeat");
        assert!(body.contains("NookSill.exe"), "waits on this exe's name");
        assert!(
            body.contains(r#"start "" "C:\Dock\NookSill.exe""#),
            "relaunches this exe"
        );
        assert!(
            body.ends_with("del \"%~f0\"\r\n"),
            "the script removes itself"
        );
        assert!(!body.contains("move "), "a restart must not touch the exe");
    }

    /// A path with spaces must stay quoted, or `start` treats the first token as
    /// the window title and launches the wrong thing.
    #[test]
    fn a_path_with_spaces_stays_quoted() {
        let body = script_body(Path::new(r"C:\Program Files\Nook Sill\Dock.exe"));
        assert!(body.contains(r#"start "" "C:\Program Files\Nook Sill\Dock.exe""#));
    }
}
