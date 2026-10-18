use serde::{Deserialize, Serialize};

/// Where a word's definitions came from. Stored in `vocabs.source` so the
/// manager can show a badge and find words that fell back off Cambridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WordSource {
    Cambridge,
    Dictionaryapi,
    Manual,
}

impl WordSource {
    pub fn as_str(self) -> &'static str {
        match self {
            WordSource::Cambridge => "cambridge",
            WordSource::Dictionaryapi => "dictionaryapi",
            WordSource::Manual => "manual",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "cambridge" => WordSource::Cambridge,
            "dictionaryapi" => WordSource::Dictionaryapi,
            _ => WordSource::Manual,
        }
    }
}

/// A single definition plus its example sentences. `id` is the
/// `vocab_definitions` row id; it is 0 for definitions that have been fetched
/// but not yet written, and the frontend only uses it as a list key.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VocabDefinition {
    #[serde(default)]
    pub id: i64,
    pub definition: String,
    #[serde(default)]
    pub examples: Vec<String>,
}

/// Cambridge groups definitions under a guide word (`MOVE QUICKLY`, `MANAGE`).
/// Sources without guide words produce a single group with an empty one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VocabSenseGroup {
    #[serde(default)]
    pub guide_word: String,
    pub definitions: Vec<VocabDefinition>,
}

/// One part-of-speech entry, with the pronunciations that belong to it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VocabEntry {
    pub part_of_speech: String,
    #[serde(default)]
    pub pron_uk: String,
    #[serde(default)]
    pub pron_us: String,
    pub sense_groups: Vec<VocabSenseGroup>,
}

/// A word and its full three-level tree, as sent to the manager UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VocabWord {
    pub id: i64,
    pub word: String,
    pub source: WordSource,
    pub entries: Vec<VocabEntry>,
    pub definition_count: usize,
    pub created_at: String,
    pub updated_at: String,
}

/// What the manager sends back when saving a hand-edited tree.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VocabTreeInput {
    pub word: String,
    pub entries: Vec<VocabEntry>,
}

/// One rotation unit: a single definition flattened together with the word it
/// belongs to. This is what the menu bar popup renders and what prev/next
/// steps through.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VocabCard {
    pub vocab_id: i64,
    pub word: String,
    pub source: WordSource,
    pub part_of_speech: String,
    pub pron_uk: String,
    pub pron_us: String,
    pub guide_word: String,
    pub definition: String,
    pub examples: Vec<String>,
    /// 1-based position of this definition within its word.
    pub position: usize,
    /// Total definitions belonging to this word.
    pub total: usize,
}

/// Flattens every word's tree into the rotation list, in list order.
pub fn build_cards(words: &[VocabWord]) -> Vec<VocabCard> {
    let mut cards = Vec::new();

    for word in words {
        let total = word.definition_count;
        let mut position = 0;

        for entry in &word.entries {
            for group in &entry.sense_groups {
                for definition in &group.definitions {
                    position += 1;
                    cards.push(VocabCard {
                        vocab_id: word.id,
                        word: word.word.clone(),
                        source: word.source,
                        part_of_speech: entry.part_of_speech.clone(),
                        pron_uk: entry.pron_uk.clone(),
                        pron_us: entry.pron_us.clone(),
                        guide_word: group.guide_word.clone(),
                        definition: definition.definition.clone(),
                        examples: definition.examples.clone(),
                        position,
                        total,
                    });
                }
            }
        }
    }

    cards
}

/// Flat form of one definition row, matching the `vocab_definitions` columns.
/// The tree is rebuilt from an ordered list of these.
#[derive(Debug, Clone)]
pub struct DefinitionRow {
    pub id: i64,
    pub entry_index: i64,
    pub part_of_speech: String,
    pub pron_uk: String,
    pub pron_us: String,
    pub guide_word: String,
    pub definition: String,
    pub examples: Vec<String>,
}

/// Rebuilds the three-level tree by grouping ordered rows on `entry_index`,
/// then on consecutive `guide_word`.
pub fn rows_to_entries(rows: Vec<DefinitionRow>) -> Vec<VocabEntry> {
    let mut entries: Vec<VocabEntry> = Vec::new();
    let mut current_entry_index: Option<i64> = None;

    for row in rows {
        if current_entry_index != Some(row.entry_index) {
            current_entry_index = Some(row.entry_index);
            entries.push(VocabEntry {
                part_of_speech: row.part_of_speech.clone(),
                pron_uk: row.pron_uk.clone(),
                pron_us: row.pron_us.clone(),
                sense_groups: Vec::new(),
            });
        }

        let entry = entries
            .last_mut()
            .expect("an entry is pushed before this point");

        let needs_group = entry
            .sense_groups
            .last()
            .map(|group| group.guide_word != row.guide_word)
            .unwrap_or(true);

        if needs_group {
            entry.sense_groups.push(VocabSenseGroup {
                guide_word: row.guide_word.clone(),
                definitions: Vec::new(),
            });
        }

        entry
            .sense_groups
            .last_mut()
            .expect("a group is pushed before this point")
            .definitions
            .push(VocabDefinition {
                id: row.id,
                definition: row.definition,
                examples: row.examples,
            });
    }

    entries
}

/// Flattens a tree back into ordered rows ready for insert. This is the single
/// place `entry_index` and ordering are assigned, which is what keeps the tree
/// well-formed given the schema does not enforce it.
pub fn entries_to_rows(entries: &[VocabEntry]) -> Vec<DefinitionRow> {
    let mut rows = Vec::new();

    for (entry_index, entry) in entries.iter().enumerate() {
        for group in &entry.sense_groups {
            for definition in &group.definitions {
                rows.push(DefinitionRow {
                    id: 0,
                    entry_index: entry_index as i64,
                    part_of_speech: entry.part_of_speech.clone(),
                    pron_uk: entry.pron_uk.clone(),
                    pron_us: entry.pron_us.clone(),
                    guide_word: group.guide_word.clone(),
                    definition: definition.definition.clone(),
                    examples: definition.examples.clone(),
                });
            }
        }
    }

    rows
}

pub fn count_definitions(entries: &[VocabEntry]) -> usize {
    entries
        .iter()
        .flat_map(|entry| &entry.sense_groups)
        .map(|group| group.definitions.len())
        .sum()
}
