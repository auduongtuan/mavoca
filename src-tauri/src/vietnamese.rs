//! Vietnamese for a popup card: the word's meaning from the Lạc Việt
//! English–Vietnamese dictionary installed with macOS, and the definition
//! through Apple's translator. Like `dictionary`, it has no database or Tauri
//! dependencies.
//!
//! Lạc Việt numbers its senses differently from the English dictionaries, so a
//! card's definition cannot be matched to a sense by position. The definition
//! is translated first, and the Lạc Việt sense sharing the most syllables with
//! that translation wins: "the land alongside a river" picks "bờ" for "bank",
//! not "ngân hàng".

use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::translator::translate_to_vietnamese;

#[cfg(target_os = "macos")]
const LAC_VIET_DICTIONARY_NAME: &str = "Từ điển Lạc Việt";

/// Lạc Việt's part-of-speech labels for each English one, matched against a
/// substring of the card's part of speech so "plural noun" finds "danh từ".
/// Oxford says "exclamation" where Cambridge says "interjection".
const PART_OF_SPEECH_LABELS: &[(&str, &[&str])] = &[
    ("noun", &["danh từ"]),
    ("verb", &["động từ", "ngoại động từ", "nội động từ"]),
    ("adjective", &["tính từ"]),
    ("adverb", &["phó từ", "trạng từ"]),
    ("pronoun", &["đại từ"]),
    ("preposition", &["giới từ"]),
    ("conjunction", &["liên từ"]),
    ("interjection", &["thán từ"]),
    ("exclamation", &["thán từ"]),
    ("determiner", &["mạo từ", "từ hạn định"]),
];

/// Syllables too common in Vietnamese glosses to say anything about which
/// sense a definition means.
const STOP_SYLLABLES: &[&str] = &[
    "của", "và", "là", "một", "các", "những", "có", "được", "trong", "cho", "để", "với", "người",
    "cái", "sự", "việc", "khi", "từ", "này", "đó", "hoặc", "hay", "ở", "theo", "như", "không",
    "đi", "ra", "vào", "lên", "xuống",
];

/// Shared syllables a later sense needs to beat the first one. One shared
/// syllable is usually coincidence: "move faster than walking" shares "nhanh"
/// with "lan nhanh" (spread quickly) but means "chạy", the first sense.
const MIN_SENSE_OVERLAP: usize = 2;

/// Translated cards kept so a second press of VI, or a press after the popup
/// prefetched the card, is instant. Cleared rather than evicted when full: it
/// only has to hold the cards of one session.
const MAX_CACHED_TRANSLATIONS: usize = 64;

type CacheKey = (String, String, String);

/// Holds the cache, and is held for a whole translation: a VI press that
/// arrives while the popup's prefetch of the same card is running waits for it
/// and then reads the cache, instead of translating the card a second time.
fn translation_cache() -> MutexGuard<'static, HashMap<CacheKey, CardTranslation>> {
    static CACHE: OnceLock<Mutex<HashMap<CacheKey, CardTranslation>>> = OnceLock::new();
    CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardTranslation {
    /// The word's Vietnamese meaning.
    pub word: String,
    /// The definition in Vietnamese; empty when the translator is unavailable
    /// but the dictionary still had the word.
    pub definition: String,
    /// True when `word` came from Lạc Việt rather than the translator.
    pub from_dictionary: bool,
}

/// One part of speech in a Lạc Việt entry.
#[derive(Debug, PartialEq)]
struct LacVietGroup {
    label: String,
    /// The Vietnamese meaning of each numbered sense, in order.
    senses: Vec<String>,
}

/// Translates a card, from the cache when it has been done before.
///
/// The translator costs about a second per sentence, and several seconds for
/// the first one after its model was unloaded, so the popup calls this as soon
/// as it opens and the VI button usually finds the result waiting.
pub fn translate_card(
    word: &str,
    part_of_speech: &str,
    definition: &str,
) -> Result<CardTranslation, String> {
    let key = (
        word.to_string(),
        part_of_speech.to_string(),
        definition.to_string(),
    );
    let mut cache = translation_cache();
    if let Some(cached) = cache.get(&key) {
        return Ok(cached.clone());
    }

    let translation = translate_uncached(word, part_of_speech, definition)?;
    // A result without the definition means the translator failed; leave it
    // out so the next press tries again.
    if !translation.definition.is_empty() {
        if cache.len() >= MAX_CACHED_TRANSLATIONS {
            cache.clear();
        }
        cache.insert(key, translation.clone());
    }
    Ok(translation)
}

fn translate_uncached(
    word: &str,
    part_of_speech: &str,
    definition: &str,
) -> Result<CardTranslation, String> {
    let senses = lac_viet_senses(word, part_of_speech);

    let definition_vi = match translate_one(definition) {
        Ok(definition_vi) => definition_vi,
        // The dictionary works without the translator's language packs; show
        // its most common sense rather than nothing.
        Err(_) if !senses.is_empty() => {
            return Ok(CardTranslation {
                word: headline(&senses[0]),
                definition: String::new(),
                from_dictionary: true,
            })
        }
        Err(err) => return Err(err),
    };

    if let Some(sense) = pick_sense(&senses, &definition_vi) {
        return Ok(CardTranslation {
            word: headline(sense),
            definition: definition_vi,
            from_dictionary: true,
        });
    }

    // Only translate the word itself when Lạc Việt does not have it: it is a
    // second trip through the translator.
    Ok(CardTranslation {
        word: translate_one(word)?,
        definition: definition_vi,
        from_dictionary: false,
    })
}

fn translate_one(text: &str) -> Result<String, String> {
    translate_to_vietnamese(&[text.to_string()])?
        .pop()
        .ok_or_else(|| "Translation returned an unexpected result".to_string())
}

#[cfg(target_os = "macos")]
fn lac_viet_senses(word: &str, part_of_speech: &str) -> Vec<String> {
    crate::macos_dictionary::lookup_html(LAC_VIET_DICTIONARY_NAME, word)
        .map(|records| senses_for(&parse_lac_viet(&records), part_of_speech))
        .unwrap_or_default()
}

#[cfg(not(target_os = "macos"))]
fn lac_viet_senses(_word: &str, _part_of_speech: &str) -> Vec<String> {
    Vec::new()
}

fn parse_lac_viet(records: &[String]) -> Vec<LacVietGroup> {
    let group_selector = Selector::parse(".gramb").unwrap();
    let label_selector = Selector::parse(".ps").unwrap();
    let sense_selector = Selector::parse(".semb").unwrap();

    let mut groups = Vec::new();
    for record in records {
        let document = Html::parse_document(record);
        for group in document.select(&group_selector) {
            let label = group
                .select(&label_selector)
                .next()
                .map(|node| normalize(&node.text().collect::<String>()))
                .unwrap_or_default();
            let senses = group
                .select(&sense_selector)
                .filter_map(|sense| {
                    // The sense's meaning is its own `.trg`; each example's
                    // translation sits in another `.trg` inside `.exg`.
                    sense
                        .children()
                        .filter_map(ElementRef::wrap)
                        .find(|child| child.value().classes().any(|class| class == "trg"))
                        .map(|meaning| normalize(&meaning.text().collect::<String>()))
                })
                .filter(|meaning| !meaning.is_empty())
                .collect();
            groups.push(LacVietGroup { label, senses });
        }
    }
    groups
}

/// The senses whose label matches the card's part of speech, or every sense
/// when nothing matches (an entry Lạc Việt labels differently).
fn senses_for(groups: &[LacVietGroup], part_of_speech: &str) -> Vec<String> {
    let part_of_speech = part_of_speech.to_lowercase();
    // Longest key first, so "adverb" is not caught by "verb".
    let mut mappings: Vec<_> = PART_OF_SPEECH_LABELS.iter().collect();
    mappings.sort_by_key(|(english, _)| std::cmp::Reverse(english.len()));
    let labels = mappings
        .into_iter()
        .find(|(english, _)| part_of_speech.contains(english))
        .map(|(_, labels)| *labels)
        .unwrap_or(&[]);

    let matching: Vec<String> = groups
        .iter()
        .filter(|group| labels.contains(&group.label.as_str()))
        .flat_map(|group| group.senses.iter().cloned())
        .collect();
    if !matching.is_empty() {
        return matching;
    }
    groups
        .iter()
        .flat_map(|group| group.senses.iter().cloned())
        .collect()
}

/// The sense sharing the most meaningful syllables with the translated
/// definition. Dictionaries list the most common sense first, so it wins ties
/// and any match weaker than `MIN_SENSE_OVERLAP`.
fn pick_sense<'a>(senses: &'a [String], translated_definition: &str) -> Option<&'a String> {
    let definition = syllables(translated_definition);
    let (index, overlap) = senses
        .iter()
        .enumerate()
        .map(|(index, sense)| (index, syllables(sense).intersection(&definition).count()))
        .max_by_key(|(index, overlap)| (*overlap, std::cmp::Reverse(*index)))?;
    let index = if overlap >= MIN_SENSE_OVERLAP {
        index
    } else {
        0
    };
    senses.get(index)
}

fn syllables(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|syllable| !syllable.is_empty() && !STOP_SYLLABLES.contains(syllable))
        .map(str::to_string)
        .collect()
}

/// A sense reads "bờ; dải (dải đất dọc theo hai bên bờ sông)": the meanings,
/// then an explanation in parentheses. The popup shows only the meanings.
/// A sense that is all explanation keeps it.
fn headline(sense: &str) -> String {
    let trimmed = match sense.find(" (") {
        Some(index) if index > 0 => &sense[..index],
        _ => sense,
    };
    trimmed.trim().to_string()
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lạc Việt's markup as Dictionary.app returns it, trimmed, with invented
    /// text in place of the real entry.
    const ENTRY: &str = r#"<html><body><d:entry d:title="zorp" class="entry" lang="vi">
<span class="hwg x_xh0"><span class="hw">zorp</span></span>
<span class="gramb x_xd0"><span d:pos="1" class="ps x_xdh">danh từ </span>
<span ord="1" class="semb x_xd1 hasSn"><span class="sn x_xdh">1 </span><span class="trg x_xd2"><span d:def="1" class="trans">quỹ; tiền gửi (nơi giữ tiền của khách) </span></span>
<span class="exg x_xd2 hasSn"><span class="x_xdh"><span class="ex">a zorp account </span></span><span class="trg x_xd3"><span class="trans">tài khoản quỹ </span></span></span></span>
<span ord="2" class="semb x_xd1 hasSn"><span class="sn x_xdh">2 </span><span class="trg x_xd2"><span d:def="1" class="trans">bờ; dải (dải đất dọc theo hai bên bờ sông) </span></span></span>
</span>
<span class="gramb x_xd0"><span class="ps x_xdh">ngoại động từ </span>
<span ord="1" class="semb x_xd1"><span class="trg x_xd2"><span d:def="1" class="trans">gửi tiền </span></span></span>
</span>
</d:entry></body></html>"#;

    fn groups() -> Vec<LacVietGroup> {
        parse_lac_viet(&[ENTRY.to_string()])
    }

    #[test]
    fn parses_senses_without_example_translations() {
        assert_eq!(
            groups(),
            vec![
                LacVietGroup {
                    label: "danh từ".into(),
                    senses: vec![
                        "quỹ; tiền gửi (nơi giữ tiền của khách)".into(),
                        "bờ; dải (dải đất dọc theo hai bên bờ sông)".into(),
                    ],
                },
                LacVietGroup {
                    label: "ngoại động từ".into(),
                    senses: vec!["gửi tiền".into()],
                },
            ]
        );
    }

    #[test]
    fn filters_senses_by_part_of_speech() {
        assert_eq!(senses_for(&groups(), "verb"), vec!["gửi tiền"]);
        assert_eq!(senses_for(&groups(), "plural noun").len(), 2);
        // Nothing labelled as an adverb: fall back to every sense.
        assert_eq!(senses_for(&groups(), "adverb").len(), 3);
    }

    #[test]
    fn picks_the_sense_that_matches_the_definition() {
        let senses = senses_for(&groups(), "noun");
        let river = pick_sense(&senses, "vùng đất dọc theo bờ sông hoặc hồ").unwrap();
        assert_eq!(headline(river), "bờ; dải");

        // No overlap, or a single shared syllable: the first, most common sense.
        let unrelated = pick_sense(&senses, "một điều hoàn toàn khác").unwrap();
        assert_eq!(headline(unrelated), "quỹ; tiền gửi");
        let weak = pick_sense(&senses, "chạy dọc con đường").unwrap();
        assert_eq!(headline(weak), "quỹ; tiền gửi");
        assert!(pick_sense(&[], "bất kỳ").is_none());
    }

    #[test]
    fn headline_drops_the_explanation() {
        assert_eq!(
            headline("khả năng cầu may (khả năng tình cờ)"),
            "khả năng cầu may"
        );
        assert_eq!(
            headline("(đánh bài) vốn của nhà cái"),
            "(đánh bài) vốn của nhà cái"
        );
    }
}

#[cfg(all(test, target_os = "macos"))]
mod live_tests {
    use super::*;

    /// `cargo test lac_viet_is_readable -- --ignored --nocapture`
    #[test]
    #[ignore = "reads the dictionaries installed on this Mac"]
    fn lac_viet_is_readable_and_still_parses() {
        // Stand-ins for the translator's output, which needs the app's run loop.
        let cases = [
            (
                "bank",
                "noun",
                "vùng đất dọc theo hoặc dốc xuống sông hoặc hồ",
            ),
            (
                "bank",
                "noun",
                "một tổ chức tài chính nhận tiền gửi của khách hàng",
            ),
            ("run", "verb", "di chuyển với tốc độ nhanh hơn đi bộ"),
            ("serendipity", "noun", "sự xảy ra tình cờ một cách may mắn"),
        ];
        for (word, part_of_speech, definition_vi) in cases {
            let senses = lac_viet_senses(word, part_of_speech);
            assert!(!senses.is_empty(), "no Lạc Việt senses for {word}");
            let picked = pick_sense(&senses, definition_vi).map(|sense| headline(sense));
            println!(
                "{word} ({part_of_speech}), {} senses: {picked:?}",
                senses.len()
            );
        }
    }
}
