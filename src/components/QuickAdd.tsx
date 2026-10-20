import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { MagnifyingGlass } from '@phosphor-icons/react';
import { VocabWord } from '../types';

/**
 * The window the add-word shortcut opens. Only the word is typed; Rust fetches
 * every meaning on save, then the meaning popup shows the new word.
 */
export const QuickAdd: React.FC = () => {
  const inputRef = useRef<HTMLInputElement>(null);
  const [word, setWord] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // The window is hidden, not closed, between uses, so each opening starts fresh.
  useEffect(() => {
    inputRef.current?.focus();
    const unlisten = getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (focused) {
        setWord('');
        setError(null);
        inputRef.current?.focus();
      }
    });
    return () => {
      unlisten.then((stop) => stop());
    };
  }, []);

  const close = () => {
    invoke('hide_quick_add_window').catch((err) => {
      console.error('Failed to hide quick add', err);
    });
  };

  // The input is disabled during lookup, which drops focus; take it back so a
  // typo can be fixed straight away.
  useEffect(() => {
    if (!busy) {
      inputRef.current?.focus();
    }
  }, [busy]);

  const add = async () => {
    if (!word.trim() || busy) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const saved = await invoke<VocabWord>('add_vocab_word', { word });
      await invoke('set_current_vocab_id', { id: saved.id });
      close();
      await invoke('show_main_window');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      event.preventDefault();
      void add();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      close();
    }
  };

  return (
    <main className="quick-add">
      <div className="quick-add-field">
        <MagnifyingGlass size={18} weight="bold" aria-hidden />
        <input
          ref={inputRef}
          value={word}
          onChange={(event) => setWord(event.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Add a word"
          aria-label="Word to add"
          disabled={busy}
          spellCheck={false}
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="off"
        />
      </div>
      <p className={error ? 'quick-add-status error' : 'quick-add-status'}>
        {error ?? (busy ? 'Looking up meanings…' : 'Enter to add · Esc to close')}
      </p>
    </main>
  );
};
