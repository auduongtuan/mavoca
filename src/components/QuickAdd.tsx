import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Autocomplete } from '@base-ui/react/autocomplete';
import { MagnifyingGlass } from '@phosphor-icons/react';
import { PreviewDefinition, VocabWord, WordPreview, WordSuggestion } from '../types';

/** One lookup per pause in typing, not one per keystroke. */
const SUGGEST_DEBOUNCE_MS = 180;
/** The pane follows the arrow keys, so its lookup waits for a moment's rest. */
const PREVIEW_DEBOUNCE_MS = 120;
/** Senses shown before the count takes over; the pane resizes with the window. */
const PREVIEW_SENSES = 3;
/** Below two letters every list is noise. */
const MIN_PREFIX_LEN = 2;

/** The accent-labelled transcriptions of one entry, without repeating a shared one. */
const pronunciationsOf = (definition: PreviewDefinition): string[] => {
  const parts: string[] = [];
  if (definition.pronUk) {
    parts.push(`UK ${definition.pronUk}`);
  }
  if (definition.pronUs && definition.pronUs !== definition.pronUk) {
    parts.push(`US ${definition.pronUs}`);
  }
  return parts;
};

/** Where a previewed meaning came from, when saying so is worth the room. */
const SOURCE_LABELS: Record<string, string> = {
  saved: 'Saved',
  cambridge: 'Cambridge',
  dictionaryapi: 'Dictionary API',
};

/**
 * The window the add-word shortcut opens. Only the word is typed; Rust fetches
 * every meaning on save, then the meaning popup shows the new word.
 *
 * Typing offers completions from the saved words and from the system word list,
 * and the first completion is highlighted as soon as the list opens, so its
 * meaning shows beside the list without anything else being pressed. The pane
 * follows the highlight with one debounced lookup per word, answered from the
 * vocabulary, the dictionary installed with macOS, or the online sources behind
 * it, and dropped if the highlight has moved on.
 *
 * The list is a Base UI autocomplete in `inline` mode: the window is sized from
 * the content it measures, so a list that floated above the window could not be
 * measured at all.
 */
export const QuickAdd: React.FC = () => {
  const inputRef = useRef<HTMLInputElement>(null);
  const mainRef = useRef<HTMLElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const [word, setWord] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [suggestions, setSuggestions] = useState<WordSuggestion[]>([]);
  /** The suggestion the arrow keys are on, which the pane describes. */
  const [highlighted, setHighlighted] = useState<string | null>(null);
  const [preview, setPreview] = useState<WordPreview | null>(null);
  /** The word whose list was dismissed, so it does not reopen until the next edit. */
  const [dismissedFor, setDismissedFor] = useState<string | null>(null);
  /** Answers to superseded lookups are ignored rather than shown late. */
  const latestLookup = useRef(0);
  /** A meaning is looked up once per word per opening, not once per arrow key. */
  const previewCache = useRef(new Map<string, WordPreview>());
  const requestedSize = useRef({ width: 0, height: 0 });

  const clearSuggestions = () => {
    setSuggestions([]);
    setHighlighted(null);
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
      setPreview(null);
      setDismissedFor(null);
      previewCache.current.clear();
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

  // The window is exactly as large as the content inside it, so a suggestion or
  // a meaning takes its own room and nothing is left over when it closes. The
  // field keeps its place at the top while the window grows down and right.
  useEffect(() => {
    const content = contentRef.current;
    const main = mainRef.current;
    if (!content || !main) {
      return;
    }
    const styles = getComputedStyle(main);
    const width =
      parseFloat(styles.paddingLeft) + parseFloat(styles.paddingRight) + content.offsetWidth;
    const height =
      parseFloat(styles.paddingTop) + parseFloat(styles.paddingBottom) + content.offsetHeight;
    const last = requestedSize.current;
    if (Math.abs(last.width - width) < 0.5 && Math.abs(last.height - height) < 0.5) {
      return;
    }
    requestedSize.current = { width, height };
    invoke('resize_quick_add_window', { width, height }).catch((err) => {
      console.error('Failed to resize the add word window', err);
    });
  });

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
        })
        .catch((err) => {
          console.error('Failed to load word suggestions', err);
        });
    }, SUGGEST_DEBOUNCE_MS);

    return () => clearTimeout(timer);
  }, [word, busy, dismissedFor]);

  // The pane keeps the meaning it already has while the next one loads, so an
  // arrow key does not empty the window and resize it twice.
  useEffect(() => {
    if (!highlighted) {
      setPreview(null);
      return;
    }

    const cached = previewCache.current.get(highlighted);
    if (cached) {
      setPreview(cached);
      return;
    }

    let cancelled = false;
    const timer = setTimeout(() => {
      invoke<WordPreview>('preview_word', { word: highlighted })
        .then((found) => {
          previewCache.current.set(highlighted, found);
          if (!cancelled) {
            setPreview(found);
          }
        })
        .catch((err) => {
          console.error('Failed to preview the highlighted word', err);
        });
    }, PREVIEW_DEBOUNCE_MS);

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [highlighted]);

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
    setPreview(null);
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
    setPreview(null);
    inputRef.current?.focus();
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      // Enter always adds what the field holds: the list carries a highlight from
      // the moment it opens, so letting the list take Enter would add a word that
      // was never typed.
      event.preventDefault();
      event.stopPropagation();
      void add();
    } else if (event.key === 'Tab' && !event.shiftKey) {
      // Tab is the keyboard's way to take the highlighted suggestion.
      const suggestion = suggestions.find((candidate) => candidate.word === highlighted);
      if (suggestion) {
        event.preventDefault();
        event.stopPropagation();
        accept(suggestion);
      }
    } else if (event.key === 'Escape') {
      event.preventDefault();
      if (suggestions.length) {
        setDismissedFor(word);
        clearSuggestions();
        setPreview(null);
      } else {
        close();
      }
    }
  };

  const listOpen = suggestions.length > 0;
  // The pane describes the highlighted word; while its meaning is still on the
  // way it holds the word alone rather than emptying the window.
  const current = preview && highlighted && preview.word === highlighted ? preview : null;
  const first = current?.definitions[0];
  const samePartOfSpeech =
    !!current &&
    current.definitions.every(
      (definition) => definition.partOfSpeech === current.definitions[0].partOfSpeech,
    );
  // The pane is a quick lookup, so it answers in Vietnamese as well: Lạc Việt
  // has most words, and where it does not, a saved word still has the stored
  // translation of the definition above it.
  const lacViet = current?.lacViet ?? null;
  const senses = (lacViet?.senses ?? []).slice(0, PREVIEW_SENSES);
  const hiddenSenses = (lacViet?.senses.length ?? 0) - senses.length;

  return (
    <main className="quick-add" ref={mainRef}>
      <Autocomplete.Root
        items={suggestions}
        value={word}
        onValueChange={(value) => setWord(value)}
        mode="none"
        // Rust has already filtered, and it searches the whole system word list.
        filter={null}
        inline
        open={listOpen}
        // The pane is the point of the list, so the first completion is described
        // before anything is pressed and stays highlighted as the word changes.
        autoHighlight="always"
        keepHighlight
        itemToStringValue={(suggestion: WordSuggestion) => suggestion.word}
        onItemHighlighted={(suggestion) => setHighlighted(suggestion?.word ?? null)}
      >
        <div className="quick-add-content" ref={contentRef}>
          <div className="quick-add-main">
            <Autocomplete.InputGroup className="quick-add-field">
              <MagnifyingGlass size={18} weight="bold" aria-hidden />
              <Autocomplete.Input
                ref={inputRef}
                className="quick-add-input"
                onKeyDown={handleKeyDown}
                placeholder="Add a word"
                aria-label="Word to add"
                disabled={busy}
                spellCheck={false}
                autoComplete="off"
                autoCorrect="off"
                autoCapitalize="off"
              />
            </Autocomplete.InputGroup>
            {listOpen && (
              <Autocomplete.List
                className="suggestions"
                // Keep the keystrokes: a click that takes focus out of the field
                // would close the window through the blur handler.
                onMouseDown={(event) => event.preventDefault()}
              >
                {(suggestion: WordSuggestion) => (
                  <Autocomplete.Item
                    key={suggestion.word}
                    value={suggestion}
                    className="suggestion"
                    onClick={() => accept(suggestion)}
                  >
                    <span className="suggestion-word">{suggestion.word}</span>
                    {suggestion.saved && <span className="suggestion-tag">Saved</span>}
                  </Autocomplete.Item>
                )}
              </Autocomplete.List>
            )}
            <p className={error ? 'quick-add-status error' : 'quick-add-status'}>
              {error ?? (busy ? 'Looking up meanings…' : 'Enter to add · Tab to complete · Esc to close')}
            </p>
          </div>
          {highlighted && (
            <aside className="quick-add-preview">
              <header className="preview-head">
                <span className="preview-word">{highlighted}</span>
                {current && SOURCE_LABELS[current.source] && (
                  <span className="suggestion-tag">{SOURCE_LABELS[current.source]}</span>
                )}
              </header>
              {!current && <p className="preview-note">Looking up…</p>}
              {current && !first && (
                <p className="preview-note">
                  {current.source === 'none'
                    ? 'Neither the macOS dictionary nor Cambridge has an entry.'
                    : 'No definitions stored for this word.'}
                </p>
              )}
              {current && first && (
                <>
                  <p className="preview-meta">
                    {samePartOfSpeech && (
                      <span className="preview-pos">{first.partOfSpeech}</span>
                    )}
                    {pronunciationsOf(first).map((pronunciation) => (
                      <span className="preview-ipa" key={pronunciation}>
                        {pronunciation}
                      </span>
                    ))}
                  </p>
                  <ol className="preview-definitions">
                    {current.definitions.map((definition, index) => (
                      <li className="preview-definition" key={`${definition.definition}-${index}`}>
                        {definition.guideWord && (
                          <span className="guide-word">{definition.guideWord}</span>
                        )}
                        <span className="preview-text">
                          {!samePartOfSpeech && (
                            <span className="preview-pos">{definition.partOfSpeech}</span>
                          )}
                          {definition.definition}
                        </span>
                      </li>
                    ))}
                  </ol>
                  {current.total > current.definitions.length && (
                    <p className="preview-note">
                      {current.total - current.definitions.length} more definitions when added.
                    </p>
                  )}
                </>
              )}
              {current && (senses.length > 0 || current.definitionVi) && (
                <div className="preview-vi">
                  {senses.length > 0 ? (
                    <>
                      <span className="preview-vi-label">
                        Lạc Việt
                        {lacViet?.partOfSpeechLabel ? ` · ${lacViet.partOfSpeechLabel}` : ''}
                      </span>
                      {/* No sense is marked: the pane has no translation to rank
                          them against, so the order is Lạc Việt's own. */}
                      <ul className="sense-list">
                        {senses.map((sense, index) => (
                          <li className="sense" key={`${index}-${sense.headline}`}>
                            <strong>{sense.headline}</strong>
                            {sense.explanation && ` ${sense.explanation}`}
                          </li>
                        ))}
                      </ul>
                      {hiddenSenses > 0 && (
                        <p className="preview-note">+{hiddenSenses} more</p>
                      )}
                    </>
                  ) : (
                    <>
                      <span className="preview-vi-label">Tiếng Việt</span>
                      {current.definitionVi}
                    </>
                  )}
                </div>
              )}
            </aside>
          )}
        </div>
      </Autocomplete.Root>
    </main>
  );
};
