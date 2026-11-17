//! Fault-injection acceptance tests from `docs/Dock-Integration-Guide.md` section 6.

use super::*;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn manifest(id: &str, name: &str, exe: &Path, size: u64, schema: &str) -> String {
    let dir = exe.parent().unwrap_or(Path::new(""));
    let exe = json_path(exe);
    let link = json_path(&dir.join("host-link.json"));
    let probe = json_path(&dir.join("host-probe.json"));
    let last = json_path(&dir.join("host-last.json"));
    format!(
        r#"{{
  "schema": "{schema}",
  "app": {{ "id": "{id}", "name": "{name}", "version": "1.0" }},
  "executable": {{ "path": "{exe}", "size": {size}, "modified_unix": 1700000000 }},
  "commands": {{
    "probe": {{ "exe": "{exe}", "args": ["--host-probe"] }},
    "publish": {{ "exe": "{exe}", "args": ["--host-publish"] }},
    "doctor": {{ "exe": "{exe}", "args": ["--host-doctor"] }},
    "activate": {{ "exe": "{exe}", "args": ["--host-activate"] }}
  }},
  "endpoints": {{ "files": {{
    "link": "{link}",
    "probe": "{probe}",
    "last_result": "{last}"
  }} }},
  "discovery": {{ "env": "MODULAR_CLIPBOARD_HOST_LINK" }}
}}"#
    )
}

fn probe_report(ok: bool, code: &str, repair: &[&str], ui: bool) -> String {
    let steps = repair
        .iter()
        .map(|cmd| format!(r#"{{ "command": "{cmd}", "reason": "injected" }}"#))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{
  "schema": "mc.host-probe/v1",
  "ok": {ok},
  "error": {{ "code": "{code}", "message": "injected" }},
  "repair": [{steps}],
  "checks": [{{ "name": "ui.running", "ok": {ui} }}]
}}"#
    )
}

fn json_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\")
}

struct Fixture {
    dir: PathBuf,
    exe: PathBuf,
    link: PathBuf,
    probe_file: PathBuf,
    last_file: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("nooksill-hl-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self {
            exe: dir.join("clipboard.exe"),
            link: dir.join("host-link.json"),
            probe_file: dir.join("host-probe.json"),
            last_file: dir.join("host-last.json"),
            dir,
        }
    }

    fn write(&self, text: &str) {
        fs::write(&self.link, text).unwrap();
    }

    fn write_valid(&self, size: u64) {
        self.write(&manifest(
            "com.example.clip",
            "Clipboard",
            &self.exe,
            size,
            SCHEMA,
        ));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[derive(Clone, Debug, Default)]
struct FakeRunner {
    state: Arc<Mutex<FakeState>>,
}

#[derive(Debug, Default)]
struct FakeState {
    calls: Vec<Vec<String>>,
    replies: HashMap<String, VecDeque<Option<CommandOutput>>>,
}

impl FakeRunner {
    fn reply(&self, switch: &str, stdout: &str, code: i32) {
        self.state
            .lock()
            .unwrap()
            .replies
            .entry(switch.to_owned())
            .or_default()
            .push_back(Some(CommandOutput {
                code,
                stdout: stdout.to_owned(),
            }));
    }

    fn no_output(&self, switch: &str) {
        self.state
            .lock()
            .unwrap()
            .replies
            .entry(switch.to_owned())
            .or_default()
            .push_back(None);
    }

    fn called(&self, switch: &str) -> bool {
        self.state
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|args| args.first().is_some_and(|s| s == switch))
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.state.lock().unwrap().calls.clone()
    }
}

impl Runner for FakeRunner {
    fn run(&self, _exe: &Path, args: &[String]) -> Option<CommandOutput> {
        let mut state = self.state.lock().unwrap();
        state.calls.push(args.to_vec());
        let key = args.first().cloned().unwrap_or_default();
        state.replies.get_mut(&key)?.pop_front().flatten()
    }
}

fn load(runner: &FakeRunner, fix: &Fixture) -> HostLinkProvider {
    let mut provider = HostLinkProvider::with_runner(Box::new(runner.clone()));
    assert!(provider
        .discover_from(std::slice::from_ref(&fix.link))
        .link
        .is_some());
    provider
}

/// Test 1 - Deleting the primary manifest must not remove the app: another channel
/// with the same id still yields a usable link.
#[test]
fn acceptance_1_primary_deleted_other_channel_wins() {
    let fix = Fixture::new("1");
    fix.write_valid(100);
    let copy_dir = fix.dir.join("copy");
    fs::create_dir_all(&copy_dir).unwrap();
    let copy = copy_dir.join("host-link.json");
    fs::write(&copy, fs::read_to_string(&fix.link).unwrap()).unwrap();
    fs::remove_file(&fix.link).unwrap();

    let mut provider = HostLinkProvider::default();
    let found = provider.discover_from(&[fix.link.clone(), copy]);
    assert!(
        found.link.is_some(),
        "a missing channel is not 'not installed'"
    );
    assert_eq!(found.visited.len(), 2, "every channel is attempted");
    assert_eq!(provider.entries().len(), 1);
    assert_eq!(provider.entries()[0].name, "Clipboard");
}

/// Test 2 - Invalid JSON everywhere reports a protocol code and stays retryable.
#[test]
fn acceptance_2_invalid_json_unreadable_then_doctor() {
    let fix = Fixture::new("2");
    fix.write_valid(100);
    let runner = FakeRunner::default();
    let mut provider = load(&runner, &fix);

    // All manifest copies become invalid; the cached link still carries the
    // published doctor command.
    fs::write(&fix.link, "{not json").unwrap();
    runner.reply(
        "--host-probe",
        &probe_report(false, "LINK_UNREADABLE", &["doctor"], true),
        1,
    );
    let report = provider.probe("com.example.clip");
    assert_eq!(report.error.as_ref().unwrap().code, "LINK_UNREADABLE");

    runner.reply("--host-doctor", &probe_report(true, "", &[], true), 0);
    assert!(provider.run_named("com.example.clip", "doctor"));
    fix.write_valid(100);
    assert!(provider
        .discover_from(std::slice::from_ref(&fix.link))
        .link
        .is_some());
}

/// Test 3 - Moving the folder: EXE_CHANGED forces a re-read of the fresh manifest.
#[test]
fn acceptance_3_moved_folder_republishes() {
    let fix = Fixture::new("3");
    fix.write_valid(100);
    let runner = FakeRunner::default();
    runner.reply(
        "--host-probe",
        &probe_report(false, "EXE_CHANGED", &["publish"], true),
        1,
    );
    let mut provider = load(&runner, &fix);

    // The provider-side publish rewrote the manifest at the new exe path.
    let moved = fix.dir.join("moved.exe");
    fix.write(&manifest(
        "com.example.clip",
        "Clipboard",
        &moved,
        140,
        SCHEMA,
    ));
    assert!(provider.probe("com.example.clip").needs_reread());
    provider.discover_from(std::slice::from_ref(&fix.link));
    assert_eq!(provider.links.get("com.example.clip").unwrap().exe, moved);
    assert_eq!(
        provider.links.get("com.example.clip").unwrap().size,
        Some(140)
    );
}

/// Test 4 - Daemon killed: repair runs activate, the following probe is green.
#[test]
fn acceptance_4_daemon_restart_via_repair() {
    let fix = Fixture::new("4");
    fix.write_valid(100);
    let runner = FakeRunner::default();
    runner.reply(
        "--host-probe",
        &probe_report(false, "DAEMON_NOT_RUNNING", &["activate"], false),
        1,
    );
    runner.reply("--host-activate", "{}", 0);
    runner.reply("--host-probe", &probe_report(true, "", &[], false), 0);
    let mut provider = load(&runner, &fix);

    assert!(provider.activate(&fix.exe));
    assert!(runner.called("--host-activate"));
    let activations = runner
        .calls()
        .iter()
        .filter(|args| args.first().is_some_and(|s| s == "--host-activate"))
        .count();
    assert_eq!(activations, 1, "repair already activated; do not repeat it");
    assert!(provider.probe("com.example.clip").ok);
}

/// Test 5 - Exe replaced: EXE_CHANGED -> publish refreshes the fingerprint.
#[test]
fn acceptance_5_exe_changed_refreshes_fingerprint() {
    let fix = Fixture::new("5");
    fix.write_valid(100);
    let runner = FakeRunner::default();
    runner.reply(
        "--host-probe",
        &probe_report(false, "EXE_CHANGED", &["publish"], true),
        1,
    );
    let mut provider = load(&runner, &fix);

    fix.write_valid(200);
    provider.discover_from(std::slice::from_ref(&fix.link));
    assert_eq!(
        provider.links.get("com.example.clip").unwrap().size,
        Some(200)
    );
    assert!(provider.probe("com.example.clip").needs_reread());
}

/// Test 6 - Unknown schema degrades without panic or guessed fields.
#[test]
fn acceptance_6_unknown_schema_degrades() {
    let fix = Fixture::new("6");
    fix.write(&manifest(
        "com.example.clip",
        "Clipboard",
        &fix.exe,
        100,
        "mc.host-link/v99",
    ));
    let mut provider = HostLinkProvider::default();
    assert!(provider
        .discover_from(std::slice::from_ref(&fix.link))
        .link
        .is_none());
    assert!(provider.entries().is_empty());
}

/// Test 7 - No console / no stdout: the mirrored probe file is used.
#[test]
fn acceptance_7_file_fallback() {
    let fix = Fixture::new("7");
    fix.write_valid(100);
    fs::write(&fix.probe_file, probe_report(true, "", &[], false)).unwrap();
    let runner = FakeRunner::default();
    runner.no_output("--host-probe");
    let provider = load(&runner, &fix);

    let report = provider.probe("com.example.clip");
    assert!(report.ok);
    assert!(report.source.starts_with("file:"));
}

/// Test 8 - Click uses the manifest activate args verbatim.
#[test]
fn acceptance_8_lnk_args_preserved() {
    let fix = Fixture::new("8");
    fix.write_valid(100);
    let runner = FakeRunner::default();
    runner.reply("--host-probe", &probe_report(true, "", &[], true), 0);
    runner.reply("--host-activate", "{}", 0);
    let mut provider = load(&runner, &fix);

    assert!(provider.activate(&fix.exe));
    let calls = runner.calls();
    let activate = calls
        .iter()
        .find(|args| args.first().is_some_and(|s| s == "--host-activate"))
        .unwrap();
    assert_eq!(activate.as_slice(), &["--host-activate".to_owned()]);
}

/// Test 9 - A hidden UI (ui.running: false) is normal, not an error.
#[test]
fn acceptance_9_hidden_ui_is_not_an_error() {
    let fix = Fixture::new("9");
    fix.write_valid(100);
    let runner = FakeRunner::default();
    runner.reply("--host-probe", &probe_report(true, "", &[], false), 0);
    let provider = load(&runner, &fix);

    let report = provider.probe("com.example.clip");
    assert!(report.ok);
    assert!(!report.ui_running);
    assert_eq!(report.error.as_ref().unwrap().code, "");
}

/// The last-result mirror wraps the report in `mc.host-result/v1`.
#[test]
fn last_result_wrapper_is_accepted() {
    let fix = Fixture::new("wrap");
    fix.write_valid(100);
    let wrapped = format!(
        r#"{{ "schema": "mc.host-result/v1", "command": "probe", "result": {} }}"#,
        probe_report(true, "", &[], true)
    );
    fs::write(&fix.probe_file, "not json").unwrap();
    fs::write(&fix.last_file, wrapped).unwrap();
    let runner = FakeRunner::default();
    runner.no_output("--host-probe");
    let provider = load(&runner, &fix);

    assert!(provider.probe("com.example.clip").ok);
}

#[test]
fn widget_path_roundtrips() {
    assert_eq!(
        id_from_path(&widget_path("com.example.clip")),
        Some("com.example.clip")
    );
    assert_eq!(id_from_path("nooksill:notes"), None);
    assert_eq!(id_from_path("mc.host-link:"), None);
}

#[test]
fn stale_mirrored_probe_is_ignored() {
    let fix = Fixture::new("stale");
    fix.write_valid(100);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let stale = probe_report(true, "", &[], true).replace(
        "  \"ok\": true,",
        &format!("  \"ok\": true, \"checked_unix\": {},", now - 3000),
    );
    fs::write(&fix.probe_file, stale).unwrap();
    let runner = FakeRunner::default();
    runner.no_output("--host-probe");
    let provider = load(&runner, &fix);

    let report = provider.probe("com.example.clip");
    assert!(!report.ok);
    assert_eq!(report.error.unwrap().code, "LINK_UNREADABLE");
}
