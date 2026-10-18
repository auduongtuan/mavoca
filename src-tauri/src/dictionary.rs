use reqwest::blocking::Client;
use scraper::{ElementRef, Html, Selector};
use serde::Deserialize;
use std::time::Duration;

use crate::model::{VocabDefinition, VocabEntry, VocabSenseGroup, WordSource};

const REQUEST_TIMEOUT_SECS: u64 = 12;
const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Cap on definitions kept for a single word, across all its entries. Both
/// sources return far more than this for common words — Cambridge yields 57 for
/// "run" and the fallback 62 — which makes definition-by-definition rotation
/// unusable. Both list their most common senses first, so truncating keeps the
/// ones worth learning.
const MAX_DEFINITIONS_PER_WORD: usize = 12;

const CAMBRIDGE_URL: &str = "https://dictionary.cambridge.org/us/dictionary/english";
const DICTIONARY_API_URL: &str = "https://api.dictionaryapi.dev/api/v2/entries/en";

pub struct FetchedWord {
    pub entries: Vec<VocabEntry>,
    pub source: WordSource,
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cambridge nests each word of a sentence in its own element, so joining the
/// text nodes leaves a space before punctuation ("five minutes ."). This closes
/// those gaps and drops a trailing colon left by definition markup.
fn clean_sentence(value: &str) -> String {
    let normalized = normalize_text(value);
    let mut cleaned = String::with_capacity(normalized.len());

    for character in normalized.chars() {
        if matches!(
            character,
            '.' | ',' | '!' | '?' | ';' | ':' | ')' | '’' | '\''
        ) && cleaned.ends_with(' ')
        {
            cleaned.pop();
        }

        cleaned.push(character);
    }

    cleaned = cleaned.replace("( ", "(");
    cleaned.trim_end_matches(':').trim().to_string()
}

/// Keeps at most `MAX_DEFINITIONS_PER_WORD` definitions, walking the tree in
/// order and dropping groups and entries that end up empty.
fn truncate_entries(entries: Vec<VocabEntry>) -> Vec<VocabEntry> {
    let mut remaining = MAX_DEFINITIONS_PER_WORD;
    let mut truncated = Vec::new();

    for entry in entries {
        if remaining == 0 {
            break;
        }

        let mut sense_groups = Vec::new();
        for group in entry.sense_groups {
            if remaining == 0 {
                break;
            }

            let take = group.definitions.len().min(remaining);
            let definitions: Vec<_> = group.definitions.into_iter().take(take).collect();
            remaining -= definitions.len();

            if !definitions.is_empty() {
                sense_groups.push(VocabSenseGroup {
                    guide_word: group.guide_word,
                    definitions,
                });
            }
        }

        if !sense_groups.is_empty() {
            truncated.push(VocabEntry {
                sense_groups,
                ..entry
            });
        }
    }

    truncated
}

/// Cambridge splits IPA across nested spans and omits the surrounding slashes,
/// so the joined text arrives as "ˌser. ə nˈdɪp.ə.ti". Whitespace is never
/// meaningful inside a transcription, so all of it is dropped.
fn format_pronunciation(value: &str) -> String {
    let compact: String = value.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() || compact.starts_with('/') {
        return compact;
    }

    format!("/{compact}/")
}

fn build_client() -> Result<Client, String> {
    Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|err| format!("Failed to create dictionary client: {err}"))
}

fn selector(query: &str) -> Result<Selector, String> {
    Selector::parse(query).map_err(|_| format!("Invalid selector: {query}"))
}

fn text_of(element: ElementRef<'_>) -> String {
    normalize_text(&element.text().collect::<Vec<_>>().join(" "))
}

fn first_text_in(element: ElementRef<'_>, query: &str) -> Result<Option<String>, String> {
    let selector = selector(query)?;
    Ok(element
        .select(&selector)
        .next()
        .map(text_of)
        .filter(|text| !text.is_empty()))
}

/// Reads one pronunciation, joining every IPA segment inside the first matching
/// block. Cambridge usually puts a whole transcription in a single `.ipa`, but
/// splits some across sibling spans; taking only the first would truncate those
/// to a single phoneme. Scoping to the first `.uk`/`.us` keeps later blocks on
/// the page from being concatenated in.
fn pronunciation_in(entry: ElementRef<'_>, scope_query: &str) -> Result<String, String> {
    let scope_selector = selector(scope_query)?;
    let ipa_selector = selector(".ipa")?;

    let Some(scope) = entry.select(&scope_selector).next() else {
        return Ok(String::new());
    };

    let joined = scope
        .select(&ipa_selector)
        .map(text_of)
        .collect::<Vec<_>>()
        .join("");

    Ok(format_pronunciation(&joined))
}

fn all_text_in(element: ElementRef<'_>, query: &str) -> Result<Vec<String>, String> {
    let selector = selector(query)?;
    let mut values = Vec::new();

    for node in element.select(&selector) {
        let text = text_of(node);
        if !text.is_empty() && !values.contains(&text) {
            values.push(text);
        }
    }

    Ok(values)
}

/// Fetches a word, preferring Cambridge and falling back to dictionaryapi.dev.
///
/// Cambridge sits behind a Cloudflare challenge that rejects unusual clients —
/// the app's previous `VocabLearning/0.1` user-agent got a blanket HTTP 403.
/// The browser user-agent above passes. The fallback covers a renewed block,
/// words Cambridge does not carry, and markup changes that break parsing.
pub fn fetch_word(word: &str) -> Result<FetchedWord, String> {
    let clean_word = normalize_text(word);
    if clean_word.is_empty() {
        return Err("Enter a word to look up".into());
    }

    let cambridge_error = match fetch_from_cambridge(&clean_word) {
        Ok(entries) if !entries.is_empty() => {
            return Ok(FetchedWord {
                entries: truncate_entries(entries),
                source: WordSource::Cambridge,
            })
        }
        Ok(_) => format!("no Cambridge entry for '{clean_word}'"),
        Err(err) => err,
    };

    match fetch_from_dictionary_api(&clean_word) {
        Ok(entries) if !entries.is_empty() => Ok(FetchedWord {
            entries: truncate_entries(entries),
            source: WordSource::Dictionaryapi,
        }),
        Ok(_) => Err(format!(
            "No definitions found for '{clean_word}' (Cambridge: {cambridge_error})"
        )),
        Err(fallback_error) => Err(format!(
            "Could not look up '{clean_word}'. Cambridge: {cambridge_error}. Fallback: {fallback_error}"
        )),
    }
}

fn fetch_from_cambridge(word: &str) -> Result<Vec<VocabEntry>, String> {
    let lookup_path = word.to_lowercase().replace(' ', "-");
    let encoded_word = urlencoding::encode(&lookup_path);
    let url = format!("{CAMBRIDGE_URL}/{encoded_word}");

    let response = build_client()?
        .get(url)
        .send()
        .map_err(|err| format!("request failed: {err}"))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let html = response
        .text()
        .map_err(|err| format!("unreadable response: {err}"))?;

    parse_cambridge(&html, word)
}

/// Parses the Cambridge entry page into part-of-speech entries, sense groups
/// keyed by guide word, and definitions with their examples.
///
/// Verified against the live pages for "run" and "serendipity". Scraped markup
/// is not a stable contract, so every selector has a looser fallback and an
/// entry that yields no definitions is skipped rather than failing the fetch.
fn parse_cambridge(html: &str, word: &str) -> Result<Vec<VocabEntry>, String> {
    let document = Html::parse_document(html);
    let entry_selector = selector(".pr.entry-body__el, .entry-body__el")?;
    let headword_selector = selector(".hw.dhw")?;
    let sense_selector = selector(".pr.dsense, .dsense")?;
    let def_block_selector = selector(".def-block.ddef_block, .def-block")?;
    let requested = word.to_lowercase();
    let mut entries = Vec::new();

    for entry_node in document.select(&entry_selector) {
        let headword = entry_node
            .select(&headword_selector)
            .next()
            .map(text_of)
            .unwrap_or_default();

        if headword.to_lowercase() != requested {
            continue;
        }

        let part_of_speech = first_text_in(entry_node, ".pos.dpos")?
            .or(first_text_in(entry_node, ".pos")?)
            .unwrap_or_else(|| "unknown".into());
        let pron_uk = pronunciation_in(entry_node, ".uk")?;
        let pron_us = pronunciation_in(entry_node, ".us")?;

        let mut sense_groups: Vec<VocabSenseGroup> = Vec::new();
        let sense_nodes: Vec<_> = entry_node.select(&sense_selector).collect();

        if sense_nodes.is_empty() {
            // No guide-word grouping on this entry: keep the definitions in a
            // single unlabelled group.
            let definitions = collect_definitions(entry_node, &def_block_selector)?;
            if !definitions.is_empty() {
                sense_groups.push(VocabSenseGroup {
                    guide_word: String::new(),
                    definitions,
                });
            }
        } else {
            for sense_node in sense_nodes {
                let guide_word = first_text_in(sense_node, ".dsense_gw .dw")?
                    .or(first_text_in(sense_node, ".dsense_gw span")?)
                    .or(first_text_in(sense_node, ".dsense_gw")?)
                    .unwrap_or_default();
                let definitions = collect_definitions(sense_node, &def_block_selector)?;

                if !definitions.is_empty() {
                    sense_groups.push(VocabSenseGroup {
                        guide_word,
                        definitions,
                    });
                }
            }
        }

        if !sense_groups.is_empty() {
            entries.push(VocabEntry {
                part_of_speech,
                pron_uk,
                pron_us,
                sense_groups,
            });
        }
    }

    Ok(entries)
}

fn collect_definitions(
    scope: ElementRef<'_>,
    def_block_selector: &Selector,
) -> Result<Vec<VocabDefinition>, String> {
    let mut definitions = Vec::new();

    for block in scope.select(def_block_selector) {
        let Some(definition) =
            first_text_in(block, ".def.ddef_d.db")?.or(first_text_in(block, ".def")?)
        else {
            continue;
        };

        // `.eg.deg` is the sentence on its own. Its parent `.examp` also wraps
        // the grammar label and collocation prefix ("[ T ]", "run away"), which
        // read as noise on a card.
        let mut examples = all_text_in(block, ".eg.deg")?;
        if examples.is_empty() {
            examples = all_text_in(block, ".examp.dexamp")?;
        }

        definitions.push(VocabDefinition {
            id: 0,
            definition: clean_sentence(&definition),
            examples: examples
                .iter()
                .map(|example| clean_sentence(example))
                .filter(|example| !example.is_empty())
                .collect(),
        });
    }

    Ok(definitions)
}

#[derive(Debug, Deserialize)]
struct ApiEntry {
    #[serde(default)]
    phonetics: Vec<ApiPhonetic>,
    #[serde(default)]
    meanings: Vec<ApiMeaning>,
}

#[derive(Debug, Deserialize)]
struct ApiPhonetic {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiMeaning {
    #[serde(rename = "partOfSpeech", default)]
    part_of_speech: String,
    #[serde(default)]
    definitions: Vec<ApiDefinition>,
}

#[derive(Debug, Deserialize)]
struct ApiDefinition {
    #[serde(default)]
    definition: String,
    #[serde(default)]
    example: Option<String>,
}

fn fetch_from_dictionary_api(word: &str) -> Result<Vec<VocabEntry>, String> {
    let encoded_word = urlencoding::encode(word);
    let url = format!("{DICTIONARY_API_URL}/{encoded_word}");

    let response = build_client()?
        .get(url)
        .send()
        .map_err(|err| format!("request failed: {err}"))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    // reqwest is built without default features, so there is no .json() here.
    let body = response
        .text()
        .map_err(|err| format!("unreadable response: {err}"))?;
    let api_entries: Vec<ApiEntry> =
        serde_json::from_str(&body).map_err(|err| format!("unexpected response shape: {err}"))?;

    Ok(parse_dictionary_api(api_entries))
}

/// Maps the fallback API onto the same tree. It has no guide words, so every
/// entry gets a single unlabelled sense group, and it exposes one pronunciation
/// list rather than a UK/US split.
fn parse_dictionary_api(api_entries: Vec<ApiEntry>) -> Vec<VocabEntry> {
    let mut entries = Vec::new();

    for api_entry in api_entries {
        let pronunciation = api_entry
            .phonetics
            .iter()
            .filter_map(|phonetic| phonetic.text.as_deref())
            .find(|text| !text.trim().is_empty())
            .map(format_pronunciation)
            .unwrap_or_default();

        for meaning in api_entry.meanings {
            let definitions: Vec<VocabDefinition> = meaning
                .definitions
                .into_iter()
                .filter_map(|api_definition| {
                    let definition = clean_sentence(&api_definition.definition);
                    if definition.is_empty() {
                        return None;
                    }

                    let examples = api_definition
                        .example
                        .map(|example| clean_sentence(&example))
                        .filter(|example| !example.is_empty())
                        .map(|example| vec![example])
                        .unwrap_or_default();

                    Some(VocabDefinition {
                        id: 0,
                        definition,
                        examples,
                    })
                })
                .collect();

            if definitions.is_empty() {
                continue;
            }

            let part_of_speech = normalize_text(&meaning.part_of_speech);
            entries.push(VocabEntry {
                part_of_speech: if part_of_speech.is_empty() {
                    "unknown".into()
                } else {
                    part_of_speech
                },
                pron_uk: String::new(),
                pron_us: pronunciation.clone(),
                sense_groups: vec![VocabSenseGroup {
                    guide_word: String::new(),
                    definitions,
                }],
            });
        }
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentences_lose_the_gaps_left_by_nested_markup() {
        assert_eq!(
            clean_sentence("I can run a mile in five minutes ."),
            "I can run a mile in five minutes."
        );
        assert_eq!(
            clean_sentence("A bus runs ( = goes on a route ) three times a day ."),
            "A bus runs (= goes on a route) three times a day."
        );
        // A definition's trailing colon introduces the examples and reads as a
        // typo once the examples are shown separately.
        assert_eq!(
            clean_sentence("to move using your legs :"),
            "to move using your legs"
        );
    }

    #[test]
    fn pronunciations_are_compacted_and_wrapped() {
        assert_eq!(
            format_pronunciation("ˌser. ə nˈdɪp.ə.ti"),
            "/ˌser.ənˈdɪp.ə.ti/"
        );
        assert_eq!(format_pronunciation("rʌn"), "/rʌn/");
        assert_eq!(format_pronunciation("/rʌn/"), "/rʌn/");
        assert_eq!(format_pronunciation("   "), "");
    }

    fn definition(text: &str) -> VocabDefinition {
        VocabDefinition {
            id: 0,
            definition: text.into(),
            examples: vec![],
        }
    }

    #[test]
    fn truncation_keeps_the_first_definitions_and_drops_empty_levels() {
        let entries = vec![
            VocabEntry {
                part_of_speech: "verb".into(),
                pron_uk: String::new(),
                pron_us: String::new(),
                sense_groups: vec![
                    VocabSenseGroup {
                        guide_word: "A".into(),
                        definitions: (0..10).map(|i| definition(&format!("a{i}"))).collect(),
                    },
                    VocabSenseGroup {
                        guide_word: "B".into(),
                        definitions: (0..10).map(|i| definition(&format!("b{i}"))).collect(),
                    },
                ],
            },
            VocabEntry {
                part_of_speech: "noun".into(),
                pron_uk: String::new(),
                pron_us: String::new(),
                sense_groups: vec![VocabSenseGroup {
                    guide_word: "C".into(),
                    definitions: vec![definition("c0")],
                }],
            },
        ];

        let truncated = truncate_entries(entries);
        let total: usize = truncated
            .iter()
            .flat_map(|entry| &entry.sense_groups)
            .map(|group| group.definitions.len())
            .sum();

        assert_eq!(total, MAX_DEFINITIONS_PER_WORD);
        // The noun entry falls entirely outside the budget and is dropped
        // rather than kept as an empty shell.
        assert_eq!(truncated.len(), 1);
        assert_eq!(truncated[0].sense_groups.len(), 2);
        assert_eq!(truncated[0].sense_groups[0].definitions.len(), 10);
        assert_eq!(truncated[0].sense_groups[1].definitions.len(), 2);
    }

    /// Mirrors the structure of a real Cambridge entry page.
    const CAMBRIDGE_FIXTURE: &str = r#"
    <div class="pr entry-body__el">
      <div class="pos-header dpos-h">
        <span class="hw dhw">run</span>
        <span class="pos dpos">verb</span>
        <span class="uk dpron-i"><span class="ipa dipa">r</span><span class="ipa dipa">ʌn</span></span>
        <span class="us dpron-i"><span class="ipa dipa">rʌn</span></span>
      </div>
      <div class="pr dsense">
        <div class="dsense_h"><span class="dsense_gw"><span class="dw">GO QUICKLY</span></span></div>
        <div class="def-block ddef_block">
          <div class="def ddef_d db">to move using your legs :</div>
          <div class="examp dexamp"><span class="gram">[ I ]</span><span class="eg deg">They ran for the bus .</span></div>
          <div class="examp dexamp"><span class="eg deg">Hugh runs five miles a day .</span></div>
        </div>
      </div>
      <div class="pr dsense">
        <div class="dsense_h"><span class="dsense_gw"><span class="dw">MANAGE</span></span></div>
        <div class="def-block ddef_block">
          <div class="def ddef_d db">to organize or control something</div>
        </div>
      </div>
    </div>
    <div class="pr entry-body__el">
      <div class="pos-header dpos-h">
        <span class="hw dhw">run</span>
        <span class="pos dpos">noun</span>
      </div>
      <div class="def-block ddef_block">
        <div class="def ddef_d db">an act of running</div>
      </div>
    </div>
    <div class="pr entry-body__el">
      <div class="pos-header dpos-h"><span class="hw dhw">runner</span><span class="pos dpos">noun</span></div>
      <div class="def-block ddef_block"><div class="def ddef_d db">someone who runs</div></div>
    </div>
    "#;

    #[test]
    fn cambridge_markup_becomes_a_three_level_tree() {
        let entries = parse_cambridge(CAMBRIDGE_FIXTURE, "run").expect("parse");

        // The "runner" entry has a different headword and must not be absorbed.
        assert_eq!(entries.len(), 2);

        let verb = &entries[0];
        assert_eq!(verb.part_of_speech, "verb");
        assert_eq!(verb.pron_uk, "/rʌn/");
        assert_eq!(verb.pron_us, "/rʌn/");
        assert_eq!(verb.sense_groups.len(), 2);

        assert_eq!(verb.sense_groups[0].guide_word, "GO QUICKLY");
        let first = &verb.sense_groups[0].definitions[0];
        assert_eq!(first.definition, "to move using your legs");
        // The grammar label "[ I ]" lives in .examp but not in .eg, so
        // preferring .eg keeps it out of the example text.
        assert_eq!(
            first.examples,
            vec!["They ran for the bus.", "Hugh runs five miles a day."]
        );

        assert_eq!(verb.sense_groups[1].guide_word, "MANAGE");

        // An entry with no .dsense still yields a single unlabelled group.
        let noun = &entries[1];
        assert_eq!(noun.part_of_speech, "noun");
        assert_eq!(noun.sense_groups.len(), 1);
        assert_eq!(noun.sense_groups[0].guide_word, "");
        assert_eq!(
            noun.sense_groups[0].definitions[0].definition,
            "an act of running"
        );
    }

    #[test]
    fn unrelated_markup_yields_no_entries_rather_than_an_error() {
        let entries =
            parse_cambridge("<html><body><p>nothing here</p></body></html>", "run").expect("parse");
        assert!(entries.is_empty());
    }

    #[test]
    fn fallback_json_becomes_entries_without_guide_words() {
        let api_entries: Vec<ApiEntry> = serde_json::from_str(
            r#"[{
                "phonetics": [{"text": ""}, {"text": "/ɹʌn/"}],
                "meanings": [
                    {"partOfSpeech": "noun", "definitions": [
                        {"definition": "act of running", "example": "my morning run ."},
                        {"definition": "", "example": "dropped"}
                    ]},
                    {"partOfSpeech": "verb", "definitions": [
                        {"definition": "to move swiftly"}
                    ]}
                ]
            }]"#,
        )
        .expect("fixture parses");

        let entries = parse_dictionary_api(api_entries);
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].part_of_speech, "noun");
        assert_eq!(entries[0].pron_us, "/ɹʌn/");
        assert_eq!(entries[0].sense_groups[0].guide_word, "");
        // The definition with empty text is dropped, not kept as a blank card.
        assert_eq!(entries[0].sense_groups[0].definitions.len(), 1);
        assert_eq!(
            entries[0].sense_groups[0].definitions[0].examples,
            vec!["my morning run."]
        );

        // A definition with no example produces an empty list, not a placeholder.
        assert!(entries[1].sense_groups[0].definitions[0]
            .examples
            .is_empty());
    }
}

/// Hits the network, so it is excluded from the default run. Use it to check
/// whether Cambridge is still reachable and whether its markup still parses:
/// `cargo test -- --ignored --nocapture`
#[cfg(test)]
mod live_tests {
    use super::*;

    #[test]
    #[ignore = "requires network access"]
    fn cambridge_is_reachable_and_still_parses() {
        let fetched = fetch_word("serendipity").expect("fetch");
        println!("source = {:?}", fetched.source);

        for entry in &fetched.entries {
            println!(
                "{} uk={} us={}",
                entry.part_of_speech, entry.pron_uk, entry.pron_us
            );
            for group in &entry.sense_groups {
                println!("  guide={:?}", group.guide_word);
                for definition in &group.definitions {
                    println!("    {}", definition.definition);
                }
            }
        }

        assert_eq!(
            fetched.source,
            WordSource::Cambridge,
            "fell back to dictionaryapi.dev — Cambridge is blocked or its markup changed"
        );
        assert!(!fetched.entries.is_empty());
    }
}
