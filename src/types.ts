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
}

/** The popup's Vietnamese, from the `translate_card` command. */
export interface CardTranslation {
  /** The word's meaning: from Lạc Việt when it has the word, else translated. */
  word: string;
  /** The definition, translated; empty when the translator is unavailable. */
  definition: string;
  fromDictionary: boolean;
}
