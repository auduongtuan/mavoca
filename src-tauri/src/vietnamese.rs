//! Vietnamese for a popup card: the word's meanings from the Lạc Việt
//! English–Vietnamese dictionary installed with macOS, and English text through
//! Apple's translator. Like `dictionary`, it has no database or Tauri
//! dependencies.
//!
//! The two are separate on purpose. Lạc Việt numbers its senses differently from
//! the English dictionaries and merges homographs under one part of speech
//! (`lead` the metal and `lead` the guide are both "danh từ"), so there is no
//! sense-to-sense mapping to make: `lac_viet_meaning` returns the word's whole
//! entry for the card's part of speech, and `translate_text` translates exactly
//! the definition on screen. The translated definition is only a hint about
//! which sense is closest, never a filter — "the land alongside a river" moves
//! "bờ" to the front for "bank", but "ngân hàng" stays visible.

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

/// Shared syllables a sense needs with the translated definition to be moved to
/// the front. One shared syllable is usually coincidence: "move faster than
/// walking" shares "nhanh" with "lan nhanh" (spread quickly) but means "chạy".
const MIN_SENSE_OVERLAP: usize = 2;

/// Translated sentences kept so a definition is translated once per run.
/// Cleared rather than evicted when full: it only has to hold one session.
const MAX_CACHED_TRANSLATIONS: usize = 256;

/// Holds the cache, and is held for a whole translation: a second call for a
/// sentence that is still being translated waits for the first and then reads
/// the cache, instead of translating it again.
fn translation_cache() -> MutexGuard<'static, HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Lạc Việt's entry for one card: the senses it lists for the card's part of
/// speech, in the order the popup shows them.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LacVietMeaning {
    /// The part-of-speech labels the senses came from, such as "danh từ".
    /// Empty when the entry labels none of them.
    pub part_of_speech_label: String,
    pub senses: Vec<DictionaryMeaning>,
    /// True when the translated definition moved a sense to the front. The
    /// popup marks the first sense only then: otherwise the order is Lạc
    /// Việt's own, most common first, and says nothing about this definition.
    pub ranked: bool,
}

/// A Lạc Việt sense reads "bờ; dải (dải đất dọc theo hai bên bờ sông)": the
/// meanings, then an explanation in parentheses.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryMeaning {
    pub headline: String,
    /// Empty when the sense has no explanation.
    pub explanation: String,
}

/// One part of speech in a Lạc Việt entry.
#[derive(Debug, PartialEq)]
struct LacVietGroup {
    label: String,
    /// The Vietnamese meaning of each numbered sense, in order.
    senses: Vec<String>,
}

/// Translates English text to Vietnamese, from the cache when it has been done
/// before.
///
/// Only ever called with the text on screen: a definition, or a word Lạc Việt
/// does not have. The definition is also stored with its row by the caller, so
/// a card is translated once rather than once per view.
pub fn translate_text(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(String::new());
    }

    let mut cache = translation_cache();
    if let Some(cached) = cache.get(text) {
        return Ok(cached.clone());
    }

    let translated = translate_to_vietnamese(&[text.to_string()])?
        .pop()
        .ok_or_else(|| "Translation returned an unexpected result".to_string())?;
    // An empty result is a failure, not a translation: leave it out so the next
    // call tries again.
    if !translated.is_empty() {
        if cache.len() >= MAX_CACHED_TRANSLATIONS {
            cache.clear();
        }
        cache.insert(text.to_string(), translated.clone());
    }
    Ok(translated)
}

/// Lạc Việt's entry for a card, or `None` when it does not have the word.
///
/// `translated_definition` is the definition on screen in Vietnamese, or `None`
/// when it has not been translated: it only reorders the senses, so the entry
/// can be shown before the translator has answered.
pub fn lac_viet_meaning(
    word: &str,
    part_of_speech: &str,
    translated_definition: Option<&str>,
) -> Option<LacVietMeaning> {
    let records = lookup_lac_viet(word)?;
    let selected = select_senses(&parse_lac_viet(&records), part_of_speech);
    if selected.senses.is_empty() {
        return None;
    }

    let (order, ranked) = rank_senses(&selected.senses, translated_definition);
    Some(LacVietMeaning {
        part_of_speech_label: selected.label,
        senses: order
            .into_iter()
            .map(|index| split_sense(&selected.senses[index]))
            .collect(),
        ranked,
    })
}

#[cfg(target_os = "macos")]
fn lookup_lac_viet(word: &str) -> Option<Vec<String>> {
    crate::macos_dictionary::lookup_html(LAC_VIET_DICTIONARY_NAME, word).ok()
}

#[cfg(not(target_os = "macos"))]
fn lookup_lac_viet(_word: &str) -> Option<Vec<String>> {
    None
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
                .map(|node| normalize_label(&node.text().collect::<String>()))
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
struct SelectedSenses {
    /// The distinct labels the senses came from, joined for display.
    label: String,
    senses: Vec<String>,
}

fn select_senses(groups: &[LacVietGroup], part_of_speech: &str) -> SelectedSenses {
    let part_of_speech = part_of_speech.to_lowercase();
    // Longest key first, so "adverb" is not caught by "verb".
    let mut mappings: Vec<_> = PART_OF_SPEECH_LABELS.iter().collect();
    mappings.sort_by_key(|(english, _)| std::cmp::Reverse(english.len()));
    let labels = mappings
        .into_iter()
        .find(|(english, _)| part_of_speech.contains(english))
        .map(|(_, labels)| *labels)
        .unwrap_or(&[]);

    let matching: Vec<&LacVietGroup> = groups
        .iter()
        .filter(|group| labels.contains(&group.label.as_str()))
        .collect();
    // Homographs keep separate groups with the same label (`lead` is "danh từ"
    // twice), so two groups can contribute; the label is only shown once.
    let groups: Vec<&LacVietGroup> = if matching.is_empty() {
        groups.iter().collect()
    } else {
        matching
    };

    SelectedSenses {
        label: join_labels(&groups),
        senses: groups
            .iter()
            .flat_map(|group| group.senses.iter().cloned())
            .collect(),
    }
}

fn join_labels(groups: &[&LacVietGroup]) -> String {
    let mut labels: Vec<&str> = Vec::new();
    for group in groups {
        if !group.label.is_empty() && !labels.contains(&group.label.as_str()) {
            labels.push(&group.label);
        }
    }
    labels.join(", ")
}

/// Sense indices in display order, and whether the translated definition is
/// what chose that order.
///
/// Dictionaries list their most common sense first, so the original order wins
/// ties and any match weaker than `MIN_SENSE_OVERLAP`.
fn rank_senses(senses: &[String], translated_definition: Option<&str>) -> (Vec<usize>, bool) {
    let native: Vec<usize> = (0..senses.len()).collect();
    let Some(translated) = translated_definition
        .map(str::trim)
        .filter(|translated| !translated.is_empty())
    else {
        return (native, false);
    };

    let definition = syllables(translated);
    let overlaps: Vec<usize> = senses
        .iter()
        .map(|sense| syllables(sense).intersection(&definition).count())
        .collect();
    let best = overlaps.iter().copied().max().unwrap_or(0);
    if best < MIN_SENSE_OVERLAP {
        return (native, false);
    }

    let mut order = native;
    // Stable, so senses scoring the same keep Lạc Việt's order.
    order.sort_by_key(|index| std::cmp::Reverse(overlaps[*index]));
    (order, true)
}

fn syllables(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|syllable| !syllable.is_empty() && !STOP_SYLLABLES.contains(syllable))
        .map(str::to_string)
        .collect()
}

/// Splits a sense into its meanings and the explanation in parentheses after
/// them. A sense that opens with parentheses, such as a usage label
/// ("(đánh bài) vốn của nhà cái"), is all headline.
fn split_sense(sense: &str) -> DictionaryMeaning {
    match sense.find(" (") {
        Some(index) if index > 0 => DictionaryMeaning {
            headline: sense[..index].trim().to_string(),
            explanation: sense[index..].trim().to_string(),
        },
        _ => DictionaryMeaning {
            headline: sense.trim().to_string(),
            explanation: String::new(),
        },
    }
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Lạc Việt writes a label as "ngoại động từ," in some entries, and the comma
/// would otherwise be shown in the popup.
fn normalize_label(text: &str) -> String {
    normalize(text)
        .trim_end_matches([',', ';', '.'])
        .trim()
        .to_string()
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
        let verb = select_senses(&groups(), "verb");
        assert_eq!(verb.senses, vec!["gửi tiền"]);
        assert_eq!(verb.label, "ngoại động từ");

        let noun = select_senses(&groups(), "plural noun");
        assert_eq!(noun.senses.len(), 2);
        assert_eq!(noun.label, "danh từ");

        // Nothing labelled as an adverb: fall back to every sense.
        let adverb = select_senses(&groups(), "adverb");
        assert_eq!(adverb.senses.len(), 3);
        assert_eq!(adverb.label, "danh từ, ngoại động từ");
    }

    /// Homographs come as separate groups with the same label (`lead` the metal
    /// and `lead` the guide are both "danh từ"); both contribute, and the label
    /// is still shown once.
    #[test]
    fn joins_homograph_groups_under_one_label() {
        let groups = vec![
            LacVietGroup {
                label: "danh từ".into(),
                senses: vec!["chì".into()],
            },
            LacVietGroup {
                label: "danh từ".into(),
                senses: vec!["sự lãnh đạo".into()],
            },
        ];
        let selected = select_senses(&groups, "noun");
        assert_eq!(selected.senses, vec!["chì", "sự lãnh đạo"]);
        assert_eq!(selected.label, "danh từ");
    }

    #[test]
    fn ranks_the_sense_matching_the_definition_first() {
        let senses = select_senses(&groups(), "noun").senses;

        let (order, ranked) = rank_senses(&senses, Some("vùng đất dọc theo bờ sông hoặc hồ"));
        assert!(ranked);
        assert_eq!(order, vec![1, 0]);

        // No translation yet: Lạc Việt's own order, most common first.
        assert_eq!(rank_senses(&senses, None), (vec![0, 1], false));
        assert_eq!(rank_senses(&senses, Some("  ")), (vec![0, 1], false));

        // No overlap, or a single shared syllable: still Lạc Việt's order.
        assert_eq!(
            rank_senses(&senses, Some("một điều hoàn toàn khác")),
            (vec![0, 1], false)
        );
        assert_eq!(
            rank_senses(&senses, Some("chạy dọc con đường")),
            (vec![0, 1], false)
        );

        // Everything is still returned, the ranking only reorders.
        assert_eq!(rank_senses(&senses, Some("bờ sông")).0.len(), senses.len());
    }

    #[test]
    fn resolves_the_meaning_for_a_card() {
        let selected = select_senses(&groups(), "noun");
        let (order, ranked) = rank_senses(&selected.senses, Some("dải đất bên bờ sông"));
        let senses: Vec<DictionaryMeaning> = order
            .into_iter()
            .map(|index| split_sense(&selected.senses[index]))
            .collect();

        assert!(ranked);
        assert_eq!(
            senses,
            vec![
                DictionaryMeaning {
                    headline: "bờ; dải".into(),
                    explanation: "(dải đất dọc theo hai bên bờ sông)".into(),
                },
                DictionaryMeaning {
                    headline: "quỹ; tiền gửi".into(),
                    explanation: "(nơi giữ tiền của khách)".into(),
                },
            ]
        );
    }

    #[test]
    fn splits_the_explanation_from_the_meanings() {
        assert_eq!(
            split_sense("khả năng cầu may (khả năng tình cờ)"),
            DictionaryMeaning {
                headline: "khả năng cầu may".into(),
                explanation: "(khả năng tình cờ)".into(),
            }
        );
        assert_eq!(
            split_sense("(đánh bài) vốn của nhà cái"),
            DictionaryMeaning {
                headline: "(đánh bài) vốn của nhà cái".into(),
                explanation: String::new(),
            }
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
        // Stand-ins for the translator's output, which needs the app's run loop:
        // the first sense is what the ranking should move to the front.
        let cases = [
            (
                "bank",
                "noun",
                "bờ",
                "vùng đất dọc theo hoặc dốc xuống sông hoặc hồ",
            ),
            (
                "bank",
                "noun",
                "ngân hàng",
                "một tổ chức tài chính nhận tiền gửi của khách hàng",
            ),
            (
                "run",
                "verb",
                "chạy",
                "di chuyển với tốc độ nhanh hơn đi bộ",
            ),
            (
                "serendipity",
                "noun",
                "cầu may",
                "sự xảy ra tình cờ một cách may mắn",
            ),
        ];
        for (word, part_of_speech, expected, definition_vi) in cases {
            let meaning = lac_viet_meaning(word, part_of_speech, Some(definition_vi))
                .unwrap_or_else(|| panic!("no Lạc Việt entry for {word}"));
            assert!(!meaning.senses.is_empty(), "no senses for {word}");
            // The ranking is a hint, not a guarantee: a short definition can
            // share fewer than MIN_SENSE_OVERLAP syllables with every sense
            // ("run"), and Lạc Việt's most common sense is already the right
            // one there.
            println!(
                "{word} ({part_of_speech}) [{}] {} senses, ranked: {}, first: {}",
                meaning.part_of_speech_label,
                meaning.senses.len(),
                meaning.ranked,
                meaning.senses[0].headline
            );
            assert!(
                meaning.senses[0].headline.contains(expected),
                "{word}: expected a sense containing {expected}, got {}",
                meaning.senses[0].headline
            );
        }
    }

    /// The popup shows the entry before the translator has answered, so the
    /// unranked path has to work on its own.
    #[test]
    #[ignore = "reads the dictionaries installed on this Mac"]
    fn lac_viet_entry_is_available_without_a_translation() {
        for (word, part_of_speech) in [("lead", "verb"), ("run", "verb"), ("bank", "noun")] {
            let meaning = lac_viet_meaning(word, part_of_speech, None)
                .unwrap_or_else(|| panic!("no Lạc Việt entry for {word}"));
            assert!(!meaning.ranked);
            assert!(!meaning.part_of_speech_label.is_empty());
            assert!(!meaning.senses[0].headline.is_empty());
            println!(
                "{word} ({part_of_speech}) [{}]: {} senses, first: {}",
                meaning.part_of_speech_label,
                meaning.senses.len(),
                meaning.senses[0].headline
            );
        }
    }
}
