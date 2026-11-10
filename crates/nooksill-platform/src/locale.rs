//! OS display language, used when the config says "auto".

use windows::Win32::Globalization::GetUserDefaultUILanguage;

/// Win32 LANGID of the user's UI language.
#[must_use]
pub fn ui_language_id() -> u16 {
    // SAFETY: the API takes no arguments and returns a LANGID by value.
    unsafe { GetUserDefaultUILanguage() }
}
