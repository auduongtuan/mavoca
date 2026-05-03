import React, { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { VocabInput, VocabWord } from '../types';

const emptyForm: VocabInput = {
  word: '',
  meaning: '',
  example: '',
  category: '',
};

const categoryOptions = [
  'noun',
  'verb',
  'adjective',
  'adverb',
  'pronoun',
  'preposition',
  'conjunction',
  'interjection',
  'phrase',
  'idiom',
  'phrasal verb',
  'unknown',
];

export const VocabManager: React.FC = () => {
  const [words, setWords] = useState<VocabWord[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [form, setForm] = useState<VocabInput>(emptyForm);
  const [query, setQuery] = useState('');
  const [lookupWord, setLookupWord] = useState('');
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const selectedWord = words.find((word) => word.id === selectedId) ?? null;
  const categories = useMemo(() => {
    const savedCategories = words
      .map((word) => word.category.trim())
      .filter(Boolean);
    const currentCategory = form.category.trim();

    return Array.from(new Set([
      ...categoryOptions,
      ...savedCategories,
      ...(currentCategory ? [currentCategory] : []),
    ]));
  }, [form.category, words]);

  const filteredWords = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) {
      return words;
    }

    return words.filter((word) => (
      word.word.toLowerCase().includes(needle)
      || word.meaning.toLowerCase().includes(needle)
      || word.category.toLowerCase().includes(needle)
    ));
  }, [query, words]);

  const loadWords = async () => {
    const list = await invoke<VocabWord[]>('list_vocab_words');
    setWords(list);
    setSelectedId((current) => {
      if (current && list.some((word) => word.id === current)) {
        return current;
      }
      return list[0]?.id ?? null;
    });
  };

  useEffect(() => {
    loadWords()
      .catch((err) => setError(err instanceof Error ? err.message : String(err)))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    const unlisten = listen<VocabWord[]>('vocab-list-changed', (event) => {
      setWords(event.payload);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (selectedWord) {
      setForm({
        word: selectedWord.word,
        meaning: selectedWord.meaning,
        example: selectedWord.example,
        category: selectedWord.category,
      });
    }
  }, [selectedWord]);

  const runAction = async (action: () => Promise<void>, success: string) => {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      await action();
      await loadWords();
      setMessage(success);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const lookup = async () => {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      const result = await invoke<VocabInput>('lookup_vocab_word', { word: lookupWord });
      setSelectedId(null);
      setForm(result);
      setMessage('Lookup complete. Review and save the entry.');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const save = () => runAction(async () => {
    if (selectedId) {
      const updated = await invoke<VocabWord>('update_vocab', { id: selectedId, input: form });
      setSelectedId(updated.id);
    } else {
      const created = await invoke<VocabWord>('create_vocab', { input: form });
      setSelectedId(created.id);
    }
  }, selectedId ? 'Vocabulary updated.' : 'Vocabulary added.');

  const remove = () => {
    if (!selectedId) {
      return;
    }

    const word = selectedWord?.word ?? 'this word';
    if (!window.confirm(`Delete "${word}"?`)) {
      return;
    }

    runAction(async () => {
      await invoke('delete_vocab', { id: selectedId });
      setSelectedId(null);
    }, 'Vocabulary deleted.');
  };

  const startNew = () => {
    setSelectedId(null);
    setForm(emptyForm);
    setLookupWord('');
    setError(null);
    setMessage(null);
  };

  return (
    <main className="manager-shell">
      <section className="manager-sidebar">
        <div className="manager-title">
          <span>Vocabulary</span>
          <strong>{words.length}</strong>
        </div>

        <input
          className="manager-search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search words"
        />

        <div className="word-list">
          {loading ? (
            <div className="manager-empty">Loading...</div>
          ) : filteredWords.length === 0 ? (
            <div className="manager-empty">No words found</div>
          ) : (
            filteredWords.map((word) => (
              <button
                key={word.id}
                className={`word-row ${word.id === selectedId ? 'active' : ''}`}
                onClick={() => setSelectedId(word.id)}
              >
                <span>{word.word}</span>
                <small>{word.category}</small>
              </button>
            ))
          )}
        </div>
      </section>

      <section className="manager-editor">
        <div className="editor-header">
          <div>
            <p className="eyebrow">SQLite-backed library</p>
            <h1>{selectedId ? 'Edit vocabulary' : 'Add vocabulary'}</h1>
          </div>
          <button className="secondary-btn" onClick={startNew} disabled={busy}>
            New
          </button>
        </div>

        <div className="lookup-bar">
          <input
            value={lookupWord}
            onChange={(event) => setLookupWord(event.target.value)}
            placeholder="Type a word for Cambridge lookup"
          />
          <button onClick={lookup} disabled={busy || !lookupWord.trim()}>
            Lookup
          </button>
        </div>

        <div className="form-grid">
          <label>
            Word
            <input
              value={form.word}
              onChange={(event) => setForm({ ...form, word: event.target.value })}
            />
          </label>

          <label>
            Category
            <select
              value={form.category}
              onChange={(event) => setForm({ ...form, category: event.target.value })}
            >
              <option value="" disabled>Select category</option>
              {categories.map((category) => (
                <option key={category} value={category}>
                  {category}
                </option>
              ))}
            </select>
          </label>

          <label className="wide">
            Meaning
            <textarea
              value={form.meaning}
              onChange={(event) => setForm({ ...form, meaning: event.target.value })}
              rows={4}
            />
          </label>

          <label className="wide">
            Example
            <textarea
              value={form.example}
              onChange={(event) => setForm({ ...form, example: event.target.value })}
              rows={3}
            />
          </label>
        </div>

        {error && <p className="manager-alert error">{error}</p>}
        {message && <p className="manager-alert success">{message}</p>}

        <div className="editor-actions">
          <button className="danger-btn" onClick={remove} disabled={busy || !selectedId}>
            Delete
          </button>
          <button className="primary-btn" onClick={save} disabled={busy}>
            {busy ? 'Saving...' : 'Save vocabulary'}
          </button>
        </div>
      </section>
    </main>
  );
};
