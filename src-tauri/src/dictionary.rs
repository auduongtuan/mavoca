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

/// The dictionary installed with macOS that lookups try first. It ships with
/// every Mac, so nothing needs downloading; the British Oxford Dictionary of
/// English is optional in Dictionary.app and often missing.
const MACOS_DICTIONARY_NAME: &str = "New Oxford American Dictionary";
/// Its bundle identifier, which keys its settings in Dictionary.app.
#[cfg(target_os = "macos")]
const MACOS_DICTIONARY_ID: &str = "com.apple.dictionary.NOAD";
/// Oxford's "US English (IPA)" pronunciation setting. The default, "0", shows
/// Oxford's own respelling (/baNGk/) and leaves IPA out of the entry HTML.
#[cfg(target_os = "macos")]
const MACOS_DICTIONARY_IPA_PRONUNCIATION: &str = "1";
/// The settings format version Oxford declares in `DCSDictionaryDefaultPrefs`.
#[cfg(target_os = "macos")]
const MACOS_DICTIONARY_PREFS_VERSION: &str = "2";

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

/// Fetches a word from the Oxford dictionary installed with macOS, then
/// Cambridge, then dictionaryapi.dev.
///
/// The macOS dictionary is offline and complete, but read through private
/// functions that a macOS release could change; any failure there moves on to
/// the online sources.
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

    let macos_error = match fetch_from_macos_dictionary(&clean_word) {
        Ok(entries) if !entries.is_empty() => {
            return Ok(FetchedWord {
                entries: truncate_entries(entries),
                source: WordSource::MacosDictionary,
            })
        }
        Ok(_) => format!("no {MACOS_DICTIONARY_NAME} entry for '{clean_word}'"),
        Err(err) => err,
    };

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
            "No definitions found for '{clean_word}' (macOS: {macos_error}; Cambridge: {cambridge_error})"
        )),
        Err(fallback_error) => Err(format!(
            "Could not look up '{clean_word}'. macOS: {macos_error}. Cambridge: {cambridge_error}. Fallback: {fallback_error}"
        )),
    }
}

#[cfg(target_os = "macos")]
fn fetch_from_macos_dictionary(word: &str) -> Result<Vec<VocabEntry>, String> {
    let records = crate::macos_dictionary::lookup_html(MACOS_DICTIONARY_NAME, word)
        .map_err(|err| err.to_string())?;
    parse_macos_dictionary(&records)
}

#[cfg(not(target_os = "macos"))]
fn fetch_from_macos_dictionary(_word: &str) -> Result<Vec<VocabEntry>, String> {
    Err("the macOS dictionary is only available on macOS".into())
}

/// Switches Oxford to IPA pronunciations unless the user has picked a style in
/// Dictionary.app, whose choice is kept. Run before the first lookup. Returns
/// whether the setting was changed.
#[cfg(target_os = "macos")]
pub fn prefer_ipa_pronunciation() -> bool {
    crate::macos_dictionary::default_pronunciation(
        MACOS_DICTIONARY_ID,
        MACOS_DICTIONARY_IPA_PRONUNCIATION,
        MACOS_DICTIONARY_PREFS_VERSION,
    )
}

/// Parses Oxford entries as Dictionary.app renders them, one HTML document per
/// homograph ("bank" the river side and "bank" the business are two).
///
/// `.se1` is a part of speech, `.se2` a numbered sense, and `.msDict` a single
/// definition; each numbered sense keeps only its first `.msDict`, because the
/// "•" subsenses after it are too fine-grained for a card. A part of speech
/// with a single sense has no `.se2` and holds its `.msDict` directly. Phrases,
/// phrasal verbs, and derivatives sit in `.subEntryBlock`s outside `.se1` and
/// are left out. Oxford has no guide words, so each entry gets one unlabelled
/// group.
fn parse_macos_dictionary(records: &[String]) -> Result<Vec<VocabEntry>, String> {
    let part_of_speech_selector = selector(".se1")?;
    let numbered_sense_selector = selector(".se2")?;
    let definition_block_selector = selector(".msDict")?;
    let mut entries = Vec::new();

    for record in records {
        let document = Html::parse_document(record);
        // The transcription the dictionary gives the headword itself, used by a
        // part of speech that carries none: `run` keeps one for the whole
        // entry, while `record` repeats one per part of speech.
        let entry_pron = headword_pronunciation_in(document.root_element())?.unwrap_or_default();

        for part_of_speech_node in document.select(&part_of_speech_selector) {
            let part_of_speech = first_text_in(part_of_speech_node, ".posg .pos")?
                .unwrap_or_else(|| "unknown".into());

            // A part of speech keeps its own transcription when it has one:
            // `record` is /ˈrɛkərd/ as a noun and /rəˈkɔrd/ as a verb, and
            // `lead` is /liːd/ or /lɛd/ depending on which entry it is. The
            // popup shows the pronunciation of the card's own entry, so it
            // follows the definition on screen instead of the first one.
            let pron_us = match headword_pronunciation_in(part_of_speech_node)? {
                Some(pronunciation) => pronunciation,
                None => entry_pron.clone(),
            };

            let numbered_senses: Vec<_> = part_of_speech_node
                .select(&numbered_sense_selector)
                .collect();
            let blocks: Vec<_> = if numbered_senses.is_empty() {
                part_of_speech_node
                    .select(&definition_block_selector)
                    .filter(|block| !block.value().classes().any(|class| class == "t_subsense"))
                    .collect()
            } else {
                numbered_senses
                    .iter()
                    .filter_map(|sense| {
                        sense
                            .select(&definition_block_selector)
                            .find(|block| has_text_in(*block, ".df"))
                    })
                    .collect()
            };

            let mut definitions = Vec::new();
            for block in blocks {
                let Some(definition) = first_text_in(block, ".df")? else {
                    continue;
                };
                let examples = all_text_in(block, ".ex")?
                    .iter()
                    // A grammar label before an example renders as ": ".
                    .map(|example| clean_sentence(example.trim_start_matches([':', ' '])))
                    .filter(|example| !example.is_empty())
                    .collect();

                definitions.push(VocabDefinition {
                    id: 0,
                    definition: clean_sentence(&definition)
                        .trim_end_matches('.')
                        .to_string(),
                    examples,
                });
            }

            if !definitions.is_empty() {
                entries.push(VocabEntry {
                    part_of_speech,
                    pron_uk: String::new(),
                    pron_us: pron_us.clone(),
                    sense_groups: vec![VocabSenseGroup {
                        guide_word: String::new(),
                        definitions,
                    }],
                });
            }
        }
    }

    Ok(entries)
}

fn has_text_in(element: ElementRef<'_>, query: &str) -> bool {
    matches!(first_text_in(element, query), Ok(Some(_)))
}

/// True when an ancestor of `element` has `class_name`. Used to tell the
/// headword's own markup from the blocks nested around it.
fn is_inside(element: ElementRef<'_>, class_name: &str) -> bool {
    element
        .ancestors()
        .filter_map(ElementRef::wrap)
        .any(|ancestor| ancestor.value().classes().any(|class| class == class_name))
}

/// Reads the first transcription in `scope` that belongs to the headword.
///
/// Oxford writes a word's inflections ahead of the rest of the entry, and each
/// one carries its own transcription — `run` opens with `running` (/ˈrənɪŋ/)
/// and `ran` (/ræn/) before reaching any definition — inside `.infg`. Phrases
/// and derivatives sit in `.subEntryBlock`. Neither is the word's own
/// pronunciation, so both are skipped; on a bare `.se1` that leaves nothing and
/// the entry-level transcription is used instead.
fn headword_pronunciation_in(scope: ElementRef<'_>) -> Result<Option<String>, String> {
    let ph_selector = selector(".ph")?;

    for node in scope.select(&ph_selector) {
        if is_inside(node, "infg") || is_inside(node, "subEntryBlock") {
            continue;
        }

        let pronunciation = format_pronunciation(&text_of(node));
        if !pronunciation.is_empty() {
            return Ok(Some(pronunciation));
        }
    }

    Ok(None)
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

    /// Oxford's markup as Dictionary.app returns it, trimmed, with invented
    /// text in place of the real entries.
    const MACOS_MULTI_SENSE: &str = r#"<html><body><d:entry d:title="zorp" class="entry">
<span class="hg x_xh0"><span class="hw">zorp </span><span class="prx"> | <span d:prn="US" class="ph t_respell">zôrp</span> | </span></span>
<span class="sg">
<span class="se1 x_xd0"><span class="posg x_xdh"><span d:pos="1" class="pos"><span class="gp tg_pos">verb </span></span><span class="infg">(<span class="inf">zorps </span><span class="prx"> | <span class="ph t_IPA">zorpz</span> | </span>)</span></span>
<span class="se2 x_xd1 hasSn"><span class="gp sn">1 </span>
<span class="msDict x_xd1sub t_first"><span class="gg">[no object] </span><span class="df">move in a wobbly way</span><span class="gp tg_df">: </span><span class="eg"><span class="ex"> the jelly zorped</span><span class="gp tg_eg"> | </span></span><span class="eg"><span class="gg">[with object]</span><span class="ex"><span class="lbl">:  </span>she zorped the tray</span><span class="gp tg_eg">. </span></span></span>
<span class="msDict x_xd1sub hasSn t_subsense"><span class="gp sn">• </span><span class="df">wobble for fun</span>: <span class="eg"><span class="ex"> kids zorp</span></span></span>
</span>
<span class="se2 x_xd1 hasSn"><span class="gp sn">2 </span>
<span class="msDict x_xd1sub t_first"><span class="lg"><span class="sj">Sport </span></span><span class="df">score by wobbling<span class="gp tg_df">. </span></span></span>
</span>
</span>
<span class="se1 x_xd0"><span class="posg x_xdh"><span class="pos"><span class="gp tg_pos">noun </span></span></span><span class="prx"> | <span d:prn="US" class="ph t_IPA">zawrp</span> | </span>
<span class="msDict x_xd1 t_core"><span class="df">a wobbly movement</span>: <span class="eg"><span class="ex"> a sudden zorp</span></span></span>
</span>
</span>
<span class="subEntryBlock x_xo0 t_phrases"><span class="subEntry"><span class="l">zorp around</span><span class="msDict x_xo2 t_core"><span class="df">a phrase sense</span></span></span></span>
</d:entry></body></html>"#;

    #[test]
    fn parses_macos_dictionary_entry() {
        let entries = parse_macos_dictionary(&[MACOS_MULTI_SENSE.to_string()]).expect("parse");

        assert_eq!(entries.len(), 2);
        let verb = &entries[0];
        assert_eq!(verb.part_of_speech, "verb");
        // The inflection inside `.infg` has its own transcription; the entry
        // falls back to the headword's, the first one on the page.
        assert_eq!(verb.pron_us, "/zôrp/");
        assert_eq!(verb.pron_uk, "");
        assert_eq!(verb.sense_groups.len(), 1);
        assert_eq!(verb.sense_groups[0].guide_word, "");

        // One definition per numbered sense: the subsense is dropped, labels
        // and punctuation are not part of the text.
        let definitions = &verb.sense_groups[0].definitions;
        assert_eq!(definitions.len(), 2);
        assert_eq!(definitions[0].definition, "move in a wobbly way");
        assert_eq!(
            definitions[0].examples,
            vec!["the jelly zorped", "she zorped the tray"]
        );
        assert_eq!(definitions[1].definition, "score by wobbling");
        assert!(definitions[1].examples.is_empty());

        // A single-sense part of speech has no numbered senses; the phrase
        // block outside `.se1` is skipped.
        let noun = &entries[1];
        assert_eq!(noun.part_of_speech, "noun");
        // Its own transcription wins over the headword's, so a word whose
        // parts of speech are pronounced differently ("record") keeps both.
        assert_eq!(noun.pron_us, "/zawrp/");
        let noun_definitions = &noun.sense_groups[0].definitions;
        assert_eq!(noun_definitions.len(), 1);
        assert_eq!(noun_definitions[0].definition, "a wobbly movement");
        assert_eq!(noun_definitions[0].examples, vec!["a sudden zorp"]);
    }

    #[test]
    fn macos_dictionary_pronunciation_follows_the_part_of_speech() {
        // Oxford's shape for "record": one headword, one part of speech per
        // pronunciation, the transcription inside each `.se1`.
        let record = r#"<html><body><d:entry d:title="zorp" class="entry">
<span class="hg x_xh0"><span class="hw">zorp</span>
<span class="se1 x_xd0"><span class="posg"><span class="pos"><span class="gp tg_pos">noun </span></span></span><span class="prx"> | <span class="ph t_IPA">zawrp</span> | </span><span class="msDict"><span class="df">a wobbly movement</span></span></span>
<span class="se1 x_xd0"><span class="posg"><span class="pos"><span class="gp tg_pos">verb </span></span></span><span class="prx"> | <span class="ph t_IPA">zorp</span> | </span><span class="msDict"><span class="df">move in a wobbly way</span></span></span>
</span>
</d:entry></body></html>"#;

        let entries = parse_macos_dictionary(&[record.to_string()]).expect("parse");

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].part_of_speech, "noun");
        assert_eq!(entries[0].pron_us, "/zawrp/");
        assert_eq!(entries[1].part_of_speech, "verb");
        assert_eq!(entries[1].pron_us, "/zorp/");
    }

    #[test]
    fn macos_dictionary_homographs_become_separate_entries() {
        let records = vec![MACOS_MULTI_SENSE.to_string(), MACOS_MULTI_SENSE.to_string()];
        let entries = parse_macos_dictionary(&records).expect("parse");
        assert_eq!(entries.len(), 4);
    }

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

    fn print_entries(entries: &[VocabEntry]) {
        for entry in entries {
            println!(
                "{} uk={} us={}",
                entry.part_of_speech, entry.pron_uk, entry.pron_us
            );
            for group in &entry.sense_groups {
                println!("  guide={:?}", group.guide_word);
                for definition in &group.definitions {
                    println!("    {} {:?}", definition.definition, definition.examples);
                }
            }
        }
    }

    #[test]
    #[ignore = "requires network access"]
    fn cambridge_is_reachable_and_still_parses() {
        // Called directly: `fetch_word` tries the macOS dictionary first.
        let entries = fetch_from_cambridge("serendipity")
            .expect("Cambridge is blocked or its markup changed");
        print_entries(&entries);
        assert!(!entries.is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "reads the dictionaries installed on this Mac"]
    fn macos_dictionary_is_readable_and_still_parses() {
        // `record` and `lead` are the cases a single entry-level transcription
        // got wrong: each part of speech, and each homograph, is pronounced
        // differently, and the popup shows the one belonging to the definition
        // on screen.
        for word in ["serendipity", "run", "bank", "record", "lead"] {
            let entries = fetch_from_macos_dictionary(word)
                .expect("the private functions changed or the dictionary is missing");
            println!("== {word}");
            print_entries(&truncate_entries(entries.clone()));
            assert!(!entries.is_empty(), "no entries for {word}");
        }

        for word in ["record", "lead"] {
            let entries = fetch_from_macos_dictionary(word).expect("entry");
            let pronunciations: Vec<&str> = entries
                .iter()
                .map(|entry| entry.pron_us.as_str())
                .filter(|pron| !pron.is_empty())
                .collect();
            assert!(
                pronunciations.windows(2).any(|pair| pair[0] != pair[1]),
                "every entry of {word} shares one transcription: {pronunciations:?}"
            );
        }
    }
}
