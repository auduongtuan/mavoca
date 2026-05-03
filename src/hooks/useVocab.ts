import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { VocabWord } from '../types';

export const useVocab = () => {
  const [vocabList, setVocabList] = useState<VocabWord[]>([]);
  const [currentWord, setCurrentWord] = useState<VocabWord | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Load vocab data on mount
  useEffect(() => {
    loadVocab();
  }, []);

  // Set up event listener for vocab changes
  useEffect(() => {
    const unlisten = listen<VocabWord>('vocab-changed', (event) => {
      setCurrentWord(event.payload);
    });

    return () => {
      unlisten.then(fn => fn());
    };
  }, []);

  useEffect(() => {
    const unlisten = listen<VocabWord[]>('vocab-list-changed', (event) => {
      setVocabList(event.payload);
      setCurrentWord((current) => {
        if (!current) {
          return event.payload[0] ?? null;
        }
        return event.payload.find((word) => word.id === current.id) ?? event.payload[0] ?? null;
      });
    });

    return () => {
      unlisten.then(fn => fn());
    };
  }, []);

  useEffect(() => {
    const unlisten = listen('vocab-empty', () => {
      setVocabList([]);
      setCurrentWord(null);
    });

    return () => {
      unlisten.then(fn => fn());
    };
  }, []);

  const syncWithBackend = async (word: VocabWord) => {
    try {
      await invoke('set_current_vocab_id', { id: word.id });
    } catch (err) {
      console.error('Failed to sync with backend:', err);
    }
  };

  const loadVocab = async () => {
    try {
      setLoading(true);
      const data = await invoke<VocabWord[]>('list_vocab_words');
      setVocabList(data);

      const word = await invoke<VocabWord | null>('get_current_vocab_word');
      if (word) {
        setCurrentWord(word);
      } else if (data.length > 0) {
        setCurrentWord(data[0]);
      } else {
        setCurrentWord(null);
      }

      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Unknown error');
    } finally {
      setLoading(false);
    }
  };

  const getRandomWord = () => {
    if (vocabList.length > 0) {
      const randomIndex = Math.floor(Math.random() * vocabList.length);
      const nextWord = vocabList[randomIndex];
      setCurrentWord(nextWord);
      syncWithBackend(nextWord);
    }
  };

  const getNextWord = () => {
    if (vocabList.length > 0) {
      const currentIndex = currentWord
        ? vocabList.findIndex((word) => word.id === currentWord.id)
        : -1;
      const nextIndex = (currentIndex + 1) % vocabList.length;
      const nextWord = vocabList[nextIndex];
      setCurrentWord(nextWord);
      syncWithBackend(nextWord);
    }
  };

  const getPreviousWord = () => {
    if (vocabList.length > 0) {
      const currentIndex = currentWord
        ? vocabList.findIndex((word) => word.id === currentWord.id)
        : 0;
      const prevIndex = currentIndex <= 0 ? vocabList.length - 1 : currentIndex - 1;
      const previousWord = vocabList[prevIndex];
      setCurrentWord(previousWord);
      syncWithBackend(previousWord);
    }
  };

  return {
    vocabList,
    currentWord,
    loading,
    error,
    getRandomWord,
    getNextWord,
    getPreviousWord,
    reload: loadVocab
  };
};
