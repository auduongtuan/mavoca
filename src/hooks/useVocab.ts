import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { VocabWord } from '../types';

export const useVocab = () => {
  const [vocabList, setVocabList] = useState<VocabWord[]>([]);
  const [currentWord, setCurrentWord] = useState<VocabWord | null>(null);
  const [currentIndex, setCurrentIndex] = useState<number>(0);
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
      // Update current index based on the word received
      const index = vocabList.findIndex(w => w.word === event.payload.word);
      if (index !== -1) {
        setCurrentIndex(index);
      }
    });

    return () => {
      unlisten.then(fn => fn());
    };
  }, [vocabList]);

  const syncWithBackend = async (index: number) => {
    try {
      await invoke('update_current_word_index', { index });
    } catch (err) {
      console.error('Failed to sync with backend:', err);
    }
  };

  const loadVocab = async () => {
    try {
      setLoading(true);
      const response = await fetch('/vocab.json');
      if (!response.ok) {
        throw new Error('Failed to load vocabulary');
      }
      const data: VocabWord[] = await response.json();
      setVocabList(data);

      // Get current word from Rust backend
      try {
        const word = await invoke<VocabWord>('get_current_vocab_word');
        if (word) {
          setCurrentWord(word);
          const index = data.findIndex(w => w.word === word.word);
          if (index !== -1) {
            setCurrentIndex(index);
          }
        }
      } catch {
        // Fallback to first word if backend call fails
        if (data.length > 0) {
          setCurrentWord(data[0]);
          setCurrentIndex(0);
        }
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
      setCurrentWord(vocabList[randomIndex]);
      setCurrentIndex(randomIndex);
      syncWithBackend(randomIndex);
    }
  };

  const getNextWord = () => {
    if (vocabList.length > 0) {
      const nextIndex = (currentIndex + 1) % vocabList.length;
      setCurrentWord(vocabList[nextIndex]);
      setCurrentIndex(nextIndex);
      syncWithBackend(nextIndex);
    }
  };

  const getPreviousWord = () => {
    if (vocabList.length > 0) {
      const prevIndex = currentIndex === 0 ? vocabList.length - 1 : currentIndex - 1;
      setCurrentWord(vocabList[prevIndex]);
      setCurrentIndex(prevIndex);
      syncWithBackend(prevIndex);
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