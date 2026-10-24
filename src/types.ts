/** `macos` is the Oxford dictionary installed with macOS. */
export type WordSource = 'macos' | 'cambridge' | 'dictionaryapi' | 'manual';

export interface VocabDefinition {
  id: number;
  definition: string;
  examples: string[];
}

/** Cambridge groups definitions under a guide word; other sources leave it empty. */
export interface VocabSenseGroup {
  guideWord: string;
  definitions: VocabDefinition[];
}

/** One part-of-speech entry with the pronunciations belonging to it. */
export interface VocabEntry {
  partOfSpeech: string;
  pronUk: string;
  pronUs: string;
  senseGroups: VocabSenseGroup[];
}

export interface VocabWord {
  id: number;
  word: string;
  source: WordSource;
  entries: VocabEntry[];
  definitionCount: number;
  createdAt: string;
  updatedAt: string;
}

export interface VocabTreeInput {
  word: string;
  entries: VocabEntry[];
}

/** One rotation unit: a single definition, flattened with its word. */
export interface VocabCardData {
  vocabId: number;
  /** The definition row this card shows, used to store its translation. */
  definitionId: number;
  word: string;
  source: WordSource;
  partOfSpeech: string;
  pronUk: string;
  pronUs: string;
  guideWord: string;
  definition: string;
  examples: string[];
  position: number;
  total: number;
}

export interface AppSettings {
  backupPath: string;
  /** Accelerator such as `Cmd+Ctrl+Shift+N`; empty when disabled. */
  addWordShortcut: string;
  /** Seconds between words while the popup is hidden; 0 turns rotation off. */
  rotationIntervalSecs: number;
}

/**
 * The popup's Vietnamese section, from the `lac_viet_meaning` command: Lạc
 * Việt's own entry for the card's part of speech, or null when it does not have
 * the word.
 */
export interface LacVietMeaning {
  /** The labels the senses came from, such as `danh từ`; may be empty. */
  partOfSpeechLabel: string;
  /** Every sense for this part of speech, in display order. */
  senses: DictionaryMeaning[];
  /**
   * True when the translated definition chose the order. The first sense is
   * marked only then: otherwise the order is Lạc Việt's own, most common
   * first, and says nothing about the definition on screen.
   */
  ranked: boolean;
}

/** "bờ; dải (dải đất dọc theo hai bên bờ sông)" split at the parenthesis. */
export interface DictionaryMeaning {
  headline: string;
  explanation: string;
}

/** One candidate word under the add-word field. */
export interface WordSuggestion {
  word: string;
  /** Already in the vocabulary, so `add_vocab_word` would refuse it. */
  saved: boolean;
}

/** One definition shown beside the add-word suggestions. */
export interface PreviewDefinition {
  partOfSpeech: string;
  pronUk: string;
  pronUs: string;
  guideWord: string;
  definition: string;
}

/**
 * What the add-word window shows beside the suggestion it has highlighted.
 *
 * `saved` was read from the vocabulary, `macos` from the dictionary installed
 * with macOS, and `none` means that dictionary has no entry — the word can still
 * be added, because `add_vocab_word` also tries Cambridge.
 */
export interface WordPreview {
  word: string;
  saved: boolean;
  source: 'saved' | 'macos' | 'none';
  definitions: PreviewDefinition[];
  /** Definitions the entry has, which can be more than are shown. */
  total: number;
  /** The stored Vietnamese translation of the first definition, if any. */
  definitionVi: string | null;
}
