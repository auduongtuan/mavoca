export interface VocabWord {
  id: number;
  word: string;
  meaning: string;
  example: string;
  category: string;
  createdAt: string;
  updatedAt: string;
}

export interface VocabInput {
  word: string;
  meaning: string;
  example: string;
  category: string;
}
