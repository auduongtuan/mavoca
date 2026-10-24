import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { MagnifyingGlass } from '@phosphor-icons/react';
import { VocabWord, WordSuggestion } from '../types';

/** One lookup per pause in typing, not one per keystroke. */
const SUGGEST_DEBOUNCE_MS = 180;
/** Below two letters every list is noise. */
const MIN_PREFIX_LEN = 2;

/**
 * The window the add-word shortcut opens. Only the word is typed; Rust fetches
 * every meaning on save, then the meaning popup shows the new word.
 *
 * Typing offers completions from the saved words and from the system word list.
 * Each lookup is debounced and its answer dropped if the word has moved on, so
 * typing never waits on a query it has already made obsolete.
 */
export const QuickAdd: React.FC = () => {
  const inputRef = useRef<HTMLInputElement>(null);
  const mainRef = useRef<HTMLElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const [word, setWord] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [suggestions, setSuggestions] = useState<WordSuggestion[]>([]);
  const [activeIndex, setActiveIndex] = useState(-1);
  /** The word whose list was dismissed, so it does not reopen until the next edit. */
  const [dismissedFor, setDismissedFor] = useState<string | null>(null);
  /** Answers to superseded lookups are ignored rather than shown late. */
  const latestLookup = useRef(0);

  const clearSuggestions = () => {
    setSuggestions([]);
    setActiveIndex(-1);
  };

  // The window is hidden, not closed, between uses, so each opening starts fresh.
  useEffect(() => {
    inputRef.current?.focus();
    // The window is hidden on blur, so it is emptied on either transition and
    // the next opening does not start with the word that was left in it.
    const unlisten = getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      setWord('');
      setError(null);
      clearSuggestions();
      setDismissedFor(null);
      if (focused) {
        inputRef.current?.focus();
      }
    });
    return () => {
      unlisten.then((stop) => stop());
    };
  }, []);

  // The input is disabled during lookup, which drops focus; take it back so a
  // typo can be fixed straight away.
  useEffect(() => {
    if (!busy) {
      inputRef.current?.focus();
    }
  }, [busy]);

  // The window is exactly as tall as the content inside it, so a suggestion
  // takes a row of window and nothing is left over when the list closes. The
  // field keeps its place at the top while the list opens below it.
  useEffect(() => {
    const content = contentRef.current;
    const main = mainRef.current;
    if (!content || !main) {
      return;
    }
    const styles = getComputedStyle(main);
    const padding = parseFloat(styles.paddingTop) + parseFloat(styles.paddingBottom);
    const height = padding + content.getBoundingClientRect().height;
    invoke('resize_quick_add_window', { height }).catch((err) => {
      console.error('Failed to resize the add word window', err);
    });
  }, [suggestions.length]);

  useEffect(() => {
    const prefix = word.trim();
    const lookup = ++latestLookup.current;
    if (busy || prefix.length < MIN_PREFIX_LEN || prefix === dismissedFor) {
      clearSuggestions();
      return;
    }

    const timer = setTimeout(() => {
      invoke<WordSuggestion[]>('suggest_words', { prefix })
        .then((found) => {
          if (lookup !== latestLookup.current) {
            return;
          }
          setSuggestions(found);
          setActiveIndex(-1);
        })
        .catch((err) => {
          console.error('Failed to load word suggestions', err);
        });
    }, SUGGEST_DEBOUNCE_MS);

    return () => clearTimeout(timer);
  }, [word, busy, dismissedFor]);

  const close = () => {
    invoke('hide_quick_add_window').catch((err) => {
      console.error('Failed to hide quick add', err);
    });
  };

  const add = async () => {
    if (!word.trim() || busy) {
      return;
    }
    setBusy(true);
    setError(null);
    clearSuggestions();
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

  // Completing the word fills the field and leaves the list closed: Enter then
  // adds it, and only a second press is needed after picking a different word.
  const accept = (suggestion: WordSuggestion) => {
    setWord(suggestion.word);
    setDismissedFor(suggestion.word);
    clearSuggestions();
    inputRef.current?.focus();
  };

  const move = (step: number) => {
    if (!suggestions.length) {
      return;
    }
    setActiveIndex((current) => {
      const next = current + step;
      if (next < 0) {
        return suggestions.length - 1;
      }
      return next >= suggestions.length ? 0 : next;
    });
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    const active = suggestions[activeIndex];

    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      move(event.key === 'ArrowDown' ? 1 : -1);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      if (active) {
        accept(active);
      } else {
        void add();
      }
    } else if (event.key === 'Tab' && active) {
      event.preventDefault();
      accept(active);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      if (suggestions.length) {
        setDismissedFor(word);
        clearSuggestions();
      } else {
        close();
      }
    }
  };

  const listOpen = suggestions.length > 0;

  return (
    <main className="quick-add" ref={mainRef}>
      <div className="quick-add-content" ref={contentRef}>
        <div className="quick-add-field">
          <MagnifyingGlass size={18} weight="bold" aria-hidden />
          <input
            ref={inputRef}
            value={word}
            onChange={(event) => setWord(event.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Add a word"
            aria-label="Word to add"
            role="combobox"
            aria-autocomplete="list"
            aria-expanded={listOpen}
            aria-controls={listOpen ? 'word-suggestions' : undefined}
            aria-activedescendant={listOpen && activeIndex >= 0 ? `suggestion-${activeIndex}` : undefined}
            disabled={busy}
            spellCheck={false}
            autoComplete="off"
            autoCorrect="off"
            autoCapitalize="off"
          />
        </div>
        {listOpen && (
          <ul className="suggestions" id="word-suggestions" role="listbox" aria-label="Word suggestions">
            {suggestions.map((suggestion, index) => (
              <li
                key={suggestion.word}
                id={`suggestion-${index}`}
                role="option"
                aria-selected={index === activeIndex}
                className="suggestion"
                // Keep the keystrokes: a click that takes focus out of the field
                // would close the window through the blur handler.
                onMouseDown={(event) => {
                  event.preventDefault();
                  accept(suggestion);
                }}
                onMouseEnter={() => setActiveIndex(index)}
              >
                <span className="suggestion-word">{suggestion.word}</span>
                {suggestion.saved && <span className="suggestion-tag">Saved</span>}
              </li>
            ))}
          </ul>
        )}
        <p className={error ? 'quick-add-status error' : 'quick-add-status'}>
          {error ?? (busy ? 'Looking up meanings…' : 'Enter to add · Esc to close')}
        </p>
      </div>
    </main>
  );
};
