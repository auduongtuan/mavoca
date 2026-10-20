//! Reads entries from the dictionaries installed with macOS, the ones
//! Dictionary.app shows. Like `dictionary`, it has no database or Tauri
//! dependencies.
//!
//! The lookup goes through private DictionaryServices functions, bridged in
//! `native/dictionary_services.c`. They are resolved at runtime, so a macOS
//! release that changes them turns every lookup into `Unavailable` and callers
//! fall back to the online sources.

use std::ffi::{c_char, c_int, CStr, CString};

// Mirrors the status codes in native/dictionary_services.h.
const STATUS_OK: c_int = 0;
const STATUS_UNAVAILABLE: c_int = 1;
const STATUS_NO_DICTIONARY: c_int = 2;

/// Separates entries in the string the bridge returns.
const RECORD_SEPARATOR: char = '\u{1e}';

extern "C" {
    fn mavoca_dcs_lookup(
        dictionary_name: *const c_char,
        word: *const c_char,
        html_out: *mut *mut c_char,
    ) -> c_int;
    fn mavoca_dcs_free(html: *mut c_char);
}

#[derive(Debug, PartialEq, Eq)]
pub enum LookupError {
    /// The private functions were not found in this macOS release.
    Unavailable,
    /// No installed dictionary has this name.
    NoDictionary,
    /// The dictionary has no entry for the word.
    NoEntry,
}

impl std::fmt::Display for LookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            LookupError::Unavailable => "the macOS dictionary service is unavailable",
            LookupError::NoDictionary => "the dictionary is not installed",
            LookupError::NoEntry => "no entry",
        })
    }
}

/// Returns the HTML of every entry in `dictionary` whose headword is `word`.
pub fn lookup_html(dictionary: &str, word: &str) -> Result<Vec<String>, LookupError> {
    let (Ok(dictionary), Ok(word)) = (CString::new(dictionary), CString::new(word)) else {
        return Err(LookupError::NoEntry);
    };

    let mut html: *mut c_char = std::ptr::null_mut();
    // SAFETY: both strings are valid and NUL-terminated for the whole call, and
    // `html` is only read when the bridge reports success, then freed by it.
    let status = unsafe { mavoca_dcs_lookup(dictionary.as_ptr(), word.as_ptr(), &mut html) };
    match status {
        STATUS_OK => {}
        STATUS_UNAVAILABLE => return Err(LookupError::Unavailable),
        STATUS_NO_DICTIONARY => return Err(LookupError::NoDictionary),
        _ => return Err(LookupError::NoEntry),
    }

    // SAFETY: on STATUS_OK the bridge returns a NUL-terminated UTF-8 string
    // that we own until it is passed back to `mavoca_dcs_free`.
    let joined = unsafe { CStr::from_ptr(html) }
        .to_string_lossy()
        .into_owned();
    unsafe { mavoca_dcs_free(html) };

    Ok(joined.split(RECORD_SEPARATOR).map(str::to_string).collect())
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Dumps raw entries, for inspecting markup when the parser needs updating:
    /// `cargo test dump_macos_entries -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn dump_macos_entries() {
        let dictionary =
            std::env::var("MAVOCA_DICT").unwrap_or("New Oxford American Dictionary".into());
        let word = std::env::var("MAVOCA_WORD").unwrap_or("run".into());
        match lookup_html(&dictionary, &word) {
            Ok(entries) => {
                for entry in entries {
                    println!("{entry}\n----");
                }
            }
            Err(err) => println!("{dictionary}: {err}"),
        }
    }
}
