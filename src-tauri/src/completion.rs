//! Word completion for the add-word field.
//!
//! The vocabulary alone cannot complete anything useful — it only holds what is
//! already saved — so the candidates come from the word list macOS installs for
//! spell checking and Look Up. No network, no per-keystroke parsing: the list is
//! read once, folded to lowercase, and binary searched.

use std::sync::OnceLock;

/// The system word list, the same one `look` and the spell checker use. It is
/// about 236k entries, one per line.
const SYSTEM_WORD_LIST: &str = "/usr/share/dict/words";

/// One letter matches too much to be worth a list.
const MIN_PREFIX_LEN: usize = 2;
/// How many prefix matches are ordered before `limit` of them are shown, so a
/// short word just past the prefix is not crowded out by a long one that happens
/// to sort first.
const SCAN_LIMIT: usize = 64;

static WORDS: OnceLock<Vec<String>> = OnceLock::new();

/// Reads the word list now, so the first typed letter does not pay for it.
/// Called from a background thread at launch; failure is not an error, the list
/// is simply empty and the add-word field offers only saved words.
pub fn warm() {
    let _ = words();
}

/// Lowercase, alphabetic, sorted, deduplicated: the file mixes `A` and `a`, odd
/// punctuation, and proper nouns, none of which belong in a completion list.
fn parse_word_list(lines: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut words: Vec<String> = lines
        .into_iter()
        .map(|line| line.trim().to_lowercase())
        .filter(|word| word.len() >= MIN_PREFIX_LEN && word.chars().all(|c| c.is_ascii_lowercase()))
        .collect();
    words.sort_unstable();
    words.dedup();
    words
}

fn words() -> &'static [String] {
    WORDS.get_or_init(|| match std::fs::read_to_string(SYSTEM_WORD_LIST) {
        Ok(contents) => parse_word_list(contents.lines().map(str::to_string)),
        Err(err) => {
            eprintln!("Could not read {SYSTEM_WORD_LIST}: {err}");
            Vec::new()
        }
    })
}

/// Up to `limit` words beginning with `prefix`, the one closest to what was
/// typed first.
pub fn suggest(prefix: &str, limit: usize) -> Vec<String> {
    suggest_from(words(), prefix, limit)
}

fn suggest_from(words: &[String], prefix: &str, limit: usize) -> Vec<String> {
    let prefix = prefix.trim().to_lowercase();
    if limit == 0 || prefix.len() < MIN_PREFIX_LEN {
        return Vec::new();
    }

    // The list is sorted, so the matches are one contiguous run.
    let start = words.partition_point(|word| word.as_str() < prefix.as_str());
    let mut matches: Vec<&String> = words[start..]
        .iter()
        .take_while(|word| word.starts_with(&prefix))
        .take(SCAN_LIMIT)
        .collect();

    // A word one letter past the prefix is likelier than one ten letters past
    // it, so short words come first and the alphabet breaks ties.
    matches.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
    matches.into_iter().take(limit).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_list_is_folded_and_cleaned() {
        let words = parse_word_list(
            [
                "Serendipity",
                "serendipity",
                "a",
                "tear",
                "e-mail",
                "o'clock",
                "Zebra",
            ]
            .map(str::to_string),
        );
        assert_eq!(words, vec!["serendipity", "tear", "zebra"]);
    }

    #[test]
    fn suggestions_are_prefix_matches_shortest_first() {
        let words = parse_word_list(
            [
                "amend",
                "serendipity",
                "sere",
                "amen",
                "amenities",
                "serendipitous",
            ]
            .map(str::to_string),
        );
        assert_eq!(
            suggest_from(&words, "amen", 10),
            vec!["amen", "amend", "amenities"]
        );
        // Shortest first, and the alphabet breaks ties.
        assert_eq!(suggest_from(&words, "SER", 2), vec!["sere", "serendipity"]);
        // One letter, no match, and no room for any of them.
        assert!(suggest_from(&words, "s", 10).is_empty());
        assert!(suggest_from(&words, "nothing", 10).is_empty());
        assert!(suggest_from(&words, "amen", 0).is_empty());
    }

    /// Reads the real word list, so it is ignored: the file is part of macOS and
    /// a machine without it simply offers no completions.
    #[test]
    #[ignore]
    fn the_system_word_list_answers_a_prefix() {
        let suggestions = suggest("seren", 6);
        println!("{suggestions:?}");
        assert!(!suggestions.is_empty());
    }
}
