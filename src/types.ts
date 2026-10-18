export type WordSource = 'cambridge' | 'dictionaryapi' | 'manual';

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
}
