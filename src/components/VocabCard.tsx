import React, { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { VocabCardData } from '../types';

const UK_VOICE_LANG = 'en-GB';
const US_VOICE_LANG = 'en-US';
// Slightly under normal speed so a single word is easier to catch.
const SPEECH_RATE = 0.9;

const canSpeak = typeof window !== 'undefined' && 'speechSynthesis' in window;

/**
 * Speaks the word with a built-in macOS voice. The card shows the UK
 * pronunciation when it has one, so the voice follows the same accent.
 */
const speakWord = (word: string, lang: string) => {
  const synth = window.speechSynthesis;
  synth.cancel();

  const utterance = new SpeechSynthesisUtterance(word);
  utterance.lang = lang;
  utterance.rate = SPEECH_RATE;
  const voice = synth.getVoices().find((candidate) => candidate.lang.replace('_', '-') === lang);
  if (voice) {
    utterance.voice = voice;
  }
  synth.speak(utterance);
};

interface VocabCardProps {
  card: VocabCardData;
  onNext: () => void;
  onPrevious: () => void;
  onRandom: () => void;
  onManage: () => void;
}

export const VocabCard: React.FC<VocabCardProps> = ({
  card,
  onNext,
  onPrevious,
  onRandom,
  onManage
}) => {
  const pronunciation = card.pronUk || card.pronUs;
  // Index 0 is the word, index 1 the definition. The parent keys this
  // component by card, so a new card starts untranslated.
  const [translations, setTranslations] = useState<string[] | null>(null);
  const [showTranslation, setShowTranslation] = useState(false);
  const [translating, setTranslating] = useState(false);
  const [translateError, setTranslateError] = useState<string | null>(null);

  const toggleTranslation = async () => {
    if (showTranslation) {
      setShowTranslation(false);
      return;
    }
    setShowTranslation(true);
    if (translations) {
      return;
    }

    setTranslating(true);
    setTranslateError(null);
    try {
      const result = await invoke<string[]>('translate_texts', {
        texts: [card.word, card.definition],
      });
      setTranslations(result);
    } catch (err) {
      setTranslateError(err instanceof Error ? err.message : String(err));
    } finally {
      setTranslating(false);
    }
  };

  const visibleTranslations = showTranslation ? translations : null;

  return (
    <div className="vocab-card">
      <div className="word-header">
        <div className="word-title">
          <div className="word-line">
            <h1 className="word">{card.word}</h1>
            {canSpeak && (
              <button
                className="speak-btn"
                onClick={() => speakWord(card.word, card.pronUk ? UK_VOICE_LANG : US_VOICE_LANG)}
                title="Pronounce"
                aria-label={`Pronounce ${card.word}`}
              >
                <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true">
                  <path
                    d="M4 9v6h4l5 4V5L8 9H4zm12.5 3a4.5 4.5 0 0 0-2.5-4v8a4.5 4.5 0 0 0 2.5-4zM14 3.2v2.1a7 7 0 0 1 0 13.4v2.1a9 9 0 0 0 0-17.6z"
                    fill="currentColor"
                  />
                </svg>
              </button>
            )}
          </div>
          {pronunciation && (
            <p className="word-pronunciation">{pronunciation}</p>
          )}
          {/* Always rendered so showing the translation never shifts the card. */}
          <p className="translation word-translation">{visibleTranslations?.[0]}</p>
        </div>
        <span className="category">{card.partOfSpeech}</span>
      </div>

      <div className="card-meta">
        {card.guideWord && <span className="guide-word">{card.guideWord}</span>}
        <div className="card-meta-end">
          {card.total > 1 && (
            <span className="card-position">
              {card.position} of {card.total}
            </span>
          )}
          <button
            onClick={toggleTranslation}
            className="translate-btn"
            disabled={translating}
            title="Translate to Vietnamese on this Mac"
          >
            {showTranslation ? 'Hide VI' : 'Translate VI'}
          </button>
        </div>
      </div>

      <div className="meaning">
        <p>{card.definition}</p>
        {visibleTranslations?.[1] && (
          <p className="translation">{visibleTranslations[1]}</p>
        )}
        {showTranslation && translating && (
          <p className="translation-status">Translating…</p>
        )}
        {showTranslation && translateError && (
          <p className="translation-status error">{translateError}</p>
        )}
      </div>

      {card.examples.length > 0 && (
        <div className="example">
          <h3>Example:</h3>
          {card.examples.map((example, index) => (
            <p className="example-text" key={index}>"{example}"</p>
          ))}
        </div>
      )}

      <div className="controls">
        <button onClick={onPrevious} className="control-btn">
          ← Previous
        </button>
        <button onClick={onRandom} className="control-btn random-btn">
          🎲 Random
        </button>
        <button onClick={onNext} className="control-btn">
          Next →
        </button>
      </div>

      <button onClick={onManage} className="manage-btn">
        Manage vocabulary
      </button>
    </div>
  );
};
