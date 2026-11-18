//! Pins the text and trait surface of the error types after the thiserror move.
//!
//! The derive must produce exactly what the hand-written `Display` produced:
//! same wording, same interpolations, same i18n lookup at display time.

use std::error::Error;
use std::sync::{Mutex, MutexGuard};

use nooksill_core::i18n::{self, Language};
use nooksill_platform::json::ParseError;
use nooksill_platform::launch::LaunchError;
use nooksill_platform::net::NetError;
use nooksill_platform::paths::PathsError;
use nooksill_platform::single_instance::MutexError;
use nooksill_platform::tray::TrayError;

/// The i18n language is process-global, so tests that read translated text must
/// not run concurrently with one that switches it.
static LANG: Mutex<()> = Mutex::new(());

fn lock_lang() -> MutexGuard<'static, ()> {
    LANG.lock().unwrap_or_else(|e| e.into_inner())
}

/// English keeps the key as the text, so the assertions below read as literals.
fn english() -> MutexGuard<'static, ()> {
    let guard = lock_lang();
    i18n::set_language(Language::En);
    guard
}

/// `GroupError` and `LaunchError` used to implement `Display` only, which kept
/// them out of `Box<dyn Error>` and out of any `source()` chain.
#[test]
fn every_error_type_implements_the_error_trait() {
    fn assert_error<T: Error>(_: &T) {}
    assert_error(&ParseError);
    assert_error(&MutexError { code: 0 });
    assert_error(&PathsError::ExePath);
    assert_error(&TrayError::Add);
    assert_error(&NetError::BadUrl);
    assert_error(&LaunchError::EmptyTarget);
}

#[test]
fn unit_and_flag_variants_keep_their_wording() {
    let _lang = english();
    assert_eq!(ParseError.to_string(), "JSON parsing failed");
    assert_eq!(
        PathsError::ExePath.to_string(),
        "Cannot determine the executable path"
    );
    assert_eq!(
        PathsError::AppData.to_string(),
        "Cannot determine the %APPDATA% path"
    );
    assert_eq!(TrayError::Add.to_string(), "The system tray is unavailable");
    assert_eq!(
        TrayError::NoIcon.to_string(),
        "The application icon could not be loaded"
    );
    assert_eq!(NetError::BadUrl.to_string(), "The URL is malformed");
    assert_eq!(NetError::Sink.to_string(), "Writing the download failed");
}

#[test]
fn interpolated_variants_keep_their_arguments() {
    let _lang = english();
    assert_eq!(
        MutexError { code: 0x1234 }.to_string(),
        "CreateMutexW failed with code 0x00001234"
    );
    assert_eq!(
        NetError::Win32("WinHttpSendRequest").to_string(),
        "Request failed: WinHttpSendRequest"
    );
    assert_eq!(
        NetError::TooLarge(37 * 1024 * 1024).to_string(),
        "The response exceeds the 37 MB limit"
    );
    assert_eq!(
        LaunchError::Unsupported("group").to_string(),
        "Not implemented: group"
    );
    assert_eq!(
        LaunchError::NotFound("C:/missing".to_owned()).to_string(),
        "The path does not exist: C:/missing"
    );
}

/// Status codes stay language-neutral: they are diagnostics, not prose.
#[test]
fn http_status_is_not_translated() {
    let _lang = lock_lang();
    i18n::set_language(Language::ZhCn);
    assert_eq!(NetError::Status(404).to_string(), "HTTP 404");
}

/// `#[from]` replaces the hand-written `From` and additionally wires `source()`,
/// which the previous empty `Error` impl dropped.
#[test]
fn paths_io_keeps_the_underlying_error_as_source() {
    let _lang = english();
    let err = PathsError::from(std::io::Error::other("disk gone"));
    assert_eq!(err.to_string(), "File operation failed: disk gone");
    assert!(err.source().is_some(), "io error must stay reachable");
}

/// The one type that keeps a hand-written `Display`: it dispatches on the
/// ShellExecuteW return value, which a per-variant derive cannot express.
#[test]
fn shell_codes_keep_their_per_value_text() {
    let _lang = english();
    assert_eq!(LaunchError::Shell(0).to_string(), "Out of memory");
    assert_eq!(LaunchError::Shell(2).to_string(), "File not found");
    assert_eq!(LaunchError::Shell(3).to_string(), "Path not found");
    assert_eq!(LaunchError::Shell(5).to_string(), "Access denied");
    assert!(LaunchError::Shell(1234).to_string().contains("1234"));
}

/// Display must resolve the language at call time, not at construction time.
#[test]
fn display_follows_the_current_language() {
    let _lang = lock_lang();
    i18n::set_language(Language::ZhCn);
    let zh = TrayError::Add.to_string();
    i18n::set_language(Language::En);
    let en = TrayError::Add.to_string();
    assert_ne!(zh, en, "translation must be looked up per display call");
    assert_eq!(en, "The system tray is unavailable");
}
