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
    reload: loadCard,
  };
};
