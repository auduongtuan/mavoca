import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { VocabCardData } from '../types';

/**
 * The popup shows one definition at a time. Rust owns the flattened rotation
 * list and the current position, so stepping is a command rather than index
 * math here — that keeps the tray title, auto-rotation, and the popup in sync.
 */
export const useVocab = () => {
  const [card, setCard] = useState<VocabCardData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadCard = useCallback(async () => {
    try {
      setLoading(true);
      const current = await invoke<VocabCardData | null>('get_current_vocab_card');
      setCard(current);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadCard();
  }, [loadCard]);

  useEffect(() => {
    const unlisten = listen<VocabCardData>('vocab-changed', (event) => {
      setCard(event.payload);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const unlisten = listen('vocab-empty', () => {
      setCard(null);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const step = useCallback(async (delta: number) => {
    try {
      const next = await invoke<VocabCardData | null>('step_vocab_card', { delta });
      if (next) {
        setCard(next);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  const getNextWord = useCallback(() => void step(1), [step]);
  const getPreviousWord = useCallback(() => void step(-1), [step]);

  // A word's definitions sit next to each other in the rotation, so moving
  // between meanings is a step that wraps within the word instead of leaving it.
  const stepMeaning = useCallback((direction: 1 | -1) => {
    if (!card || card.total < 2) {
      return;
    }
    const atEdge = direction === 1 ? card.position === card.total : card.position === 1;
    void step(atEdge ? -direction * (card.total - 1) : direction);
  }, [card, step]);

  const getNextMeaning = useCallback(() => stepMeaning(1), [stepMeaning]);
  const getPreviousMeaning = useCallback(() => stepMeaning(-1), [stepMeaning]);

  const getRandomWord = useCallback(async () => {
    try {
      const next = await invoke<VocabCardData | null>('random_vocab_card');
      if (next) {
        setCard(next);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  return {
    card,
    loading,
    error,
    getRandomWord,
    getNextWord,
    getPreviousWord,
    getNextMeaning,
    getPreviousMeaning,
    reload: loadCard,
  };
};
