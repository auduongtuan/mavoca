import React, { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';
import { VocabEntry, VocabTreeInput, VocabWord, WordSource } from '../types';
import { VocabTreeEditor, createEmptyEntries } from './VocabTreeEditor';

const sourceLabels: Record<WordSource, string> = {
  cambridge: 'Cambridge',
  dictionaryapi: 'dictionaryapi.dev',
  manual: 'Manual',
};

const definitionsOf = (word: VocabWord): string[] =>
  word.entries
    .flatMap((entry) => entry.senseGroups)
    .flatMap((group) => group.definitions)
    .map((definition) => definition.definition);

export const VocabManager: React.FC = () => {
  const [words, setWords] = useState<VocabWord[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [wordName, setWordName] = useState('');
  const [entries, setEntries] = useState<VocabEntry[]>(createEmptyEntries);
  const [query, setQuery] = useState('');
  const [newWord, setNewWord] = useState('');
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const selectedWord = words.find((word) => word.id === selectedId) ?? null;
  const appWindow = getCurrentWindow();

  const handleTitlebarMouseDown = (event: React.MouseEvent<HTMLElement>) => {
    if (event.button !== 0) {
      return;
    }

    // Let controls in the title bar keep their own click behaviour.
    if ((event.target as HTMLElement).closest('button, input, select, textarea, a')) {
      return;
    }

    // Without this the webview starts its own drag session next to the native
    // window drag, which shows the macOS copy cursor and leaves the window put.
    event.preventDefault();

    // Both commands need an explicit capability. Report a rejection instead of
    // discarding it, or an ACL denial looks exactly like a dead title bar.
    const call = event.detail === 2 ? appWindow.toggleMaximize() : appWindow.startDragging();
    call.catch((err) => setError(err instanceof Error ? err.message : String(err)));
  };

  const filteredWords = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) {
      return words;
    }

    return words.filter((word) => (
      word.word.toLowerCase().includes(needle)
      || word.entries.some((entry) => entry.partOfSpeech.toLowerCase().includes(needle))
      || definitionsOf(word).some((definition) => definition.toLowerCase().includes(needle))
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
      setWordName(selectedWord.word);
      setEntries(selectedWord.entries.length > 0 ? selectedWord.entries : createEmptyEntries());
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

  // Fetches and imports every definition in one step: no result picker.
  const addWord = () => runAction(async () => {
    const created = await invoke<VocabWord>('add_vocab_word', { word: newWord });
    setSelectedId(created.id);
    setNewWord('');
  }, 'Word added from the dictionary.');

  const refresh = () => {
    if (!selectedId) {
      return;
    }

    runAction(async () => {
      await invoke<VocabWord>('refresh_vocab_word', { id: selectedId });
    }, 'Definitions refreshed from the dictionary.');
  };

  const save = () => runAction(async () => {
    const input: VocabTreeInput = { word: wordName, entries };
    if (selectedId) {
      const updated = await invoke<VocabWord>('update_vocab', { id: selectedId, input });
      setSelectedId(updated.id);
    } else {
      const created = await invoke<VocabWord>('create_vocab', { input });
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
    setWordName('');
    setEntries(createEmptyEntries());
    setNewWord('');
    setError(null);
    setMessage(null);
  };

  const totalDefinitions = words.reduce((sum, word) => sum + word.definitionCount, 0);

  return (
    <main className="manager-shell">
      <div className="manager-window">
        <header className="manager-toolbar" onMouseDown={handleTitlebarMouseDown}>
          <div className="manager-toolbar-spacer" />
          <div className="manager-toolbar-title">
            <span>Vocabulary Manager</span>
            <small>{words.length} words · {totalDefinitions} definitions</small>
          </div>
          <div className="manager-toolbar-spacer" />
        </header>

        <div className="manager-main">
          <section className="manager-sidebar">
            <div className="manager-title">
              <span>Vocabulary</span>
              <strong>{words.length}</strong>
            </div>

            <input
              className="manager-search"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search words and definitions"
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
                    <small>
                      {word.definitionCount} def{word.definitionCount === 1 ? '' : 's'}
                    </small>
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
                value={newWord}
                onChange={(event) => setNewWord(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter' && newWord.trim() && !busy) {
                    addWord();
                  }
                }}
                placeholder="Type a word to add with dictionary definitions"
              />
              <button onClick={addWord} disabled={busy || !newWord.trim()}>
                {busy ? 'Working...' : 'Add'}
              </button>
            </div>

            <div className="form-grid">
              <label>
                Word
                <input
                  value={wordName}
                  onChange={(event) => setWordName(event.target.value)}
                />
              </label>

              {selectedWord && (
                <div className="word-source">
                  <span className={`source-badge source-${selectedWord.source}`}>
                    {sourceLabels[selectedWord.source]}
                  </span>
                  <button className="secondary-btn" onClick={refresh} disabled={busy}>
                    Refresh from dictionary
                  </button>
                </div>
              )}
            </div>

            <VocabTreeEditor entries={entries} onChange={setEntries} disabled={busy} />

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
        </div>
      </div>
    </main>
  );
};
