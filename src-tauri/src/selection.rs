//! The word the user has selected in whatever application is in front of
//! Mavoca.
//!
//! The add-word shortcut is handled while the other application still has the
//! keyboard — Mavoca is a menu bar agent, so showing a window does not activate
//! it until the click that asked for it — and that is the only moment the
//! selection can be read. Once the quick-add window takes focus the frontmost
//! application is Mavoca itself, and there is nothing to select from.
//!
//! Reading it goes through the Accessibility API, bridged in
//! `native/selection.c`, so the app needs the Accessibility permission. Without
//! it the read fails and the shortcut opens an empty field, exactly as it did
//! before.

use std::ffi::{c_char, CStr};

/// A word longer than this is a paragraph or a URL, not something to look up.
const MAX_WORD_CHARS: usize = 48;

/// What an application wraps a selected word in: quotes of both kinds, brackets,
/// and the punctuation around it. Trimmed from both ends only, so the hyphen and
/// the apostrophe inside `well-being` and `don't` survive.
const WRAPPING: &[char] = &[
    '"', '\'', '“', '”', '‘', '’', ',', '.', ';', ':', '!', '?', '(', ')', '[', ']', '{', '}', '«',
    '»', '<', '>', '…', '–', '—', '/', '\\', '|', '*', '_', '#', '~', '`', '=', '+',
];

/// The selected word, or `None` when nothing is selected, more than one word is,
/// or the selection is not a word at all.
#[cfg(target_os = "macos")]
pub fn selected_word() -> Option<String> {
    extern "C" {
        fn mavoca_selected_text() -> *mut c_char;
        fn mavoca_selection_free(text: *mut c_char);
    }

    let raw = unsafe { mavoca_selected_text() };
    if raw.is_null() {
        return None;
    }
    let selection = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { mavoca_selection_free(raw) };
    single_word(&selection)
}

#[cfg(not(target_os = "macos"))]
pub fn selected_word() -> Option<String> {
    None
}

/// Reduces a selection to the word in it, or nothing.
///
/// A triple click selects a paragraph and a double click in some editors takes
/// the trailing space with the word, so the selection cannot be trusted as it
/// arrives: it has to be exactly one word once the wrapping is off.
fn single_word(selection: &str) -> Option<String> {
    let word = selection.trim().trim_matches(WRAPPING);
    if word.is_empty() || word.chars().count() > MAX_WORD_CHARS {
        return None;
    }
    if word.split_whitespace().count() != 1 {
        return None;
    }
    if !word.chars().any(|letter| letter.is_alphabetic()) {
        return None;
    }
    Some(word.to_string())
}

#[cfg(test)]
mod tests {
    use super::single_word;

    #[test]
    fn takes_the_word_out_of_its_punctuation() {
        assert_eq!(single_word("word"), Some("word".into()));
        assert_eq!(single_word("  run\n"), Some("run".into()));
        assert_eq!(single_word("“ubiquitous”"), Some("ubiquitous".into()));
        assert_eq!(single_word("(mellifluous),"), Some("mellifluous".into()));
        assert_eq!(single_word("ephemeral."), Some("ephemeral".into()));
    }

    #[test]
    fn keeps_what_belongs_to_the_word() {
        assert_eq!(single_word("well-being"), Some("well-being".into()));
        assert_eq!(single_word("don't"), Some("don't".into()));
        assert_eq!(single_word("O'Brien"), Some("O'Brien".into()));
    }

    #[test]
    fn refuses_anything_that_is_not_one_word() {
        assert_eq!(single_word(""), None);
        assert_eq!(single_word("   "), None);
        assert_eq!(single_word("two words"), None);
        assert_eq!(single_word("the quick brown fox"), None);
        assert_eq!(single_word("12345"), None);
        assert_eq!(single_word("..."), None);
        assert_eq!(single_word(&"a".repeat(49)), None);
    }
}
