import React, { useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  ArrowLeft,
  ArrowRight,
  CaretLeft,
  CaretRight,
  Shuffle,
  SpeakerHigh,
} from '@phosphor-icons/react';
import { VocabCardData } from '../types';
import { usePopupAutoResize } from '../hooks/usePopupAutoResize';

const UK_VOICE_LANG = 'en-GB';
const US_VOICE_LANG = 'en-US';
// Slightly under normal speed so a single word is easier to catch.
const SPEECH_RATE = 0.9;

// Dictionary-style short forms; anything not listed is shown in full.
const PART_OF_SPEECH_ABBREVIATIONS: Record<string, string> = {
  noun: 'n.',
  verb: 'v.',
  adjective: 'adj.',
  adverb: 'adv.',
  pronoun: 'pron.',
  preposition: 'prep.',
  conjunction: 'conj.',
  interjection: 'interj.',
  determiner: 'det.',
  'phrasal verb': 'phr. v.',
};

const normalizePartOfSpeech = (partOfSpeech: string) => partOfSpeech.trim().toLowerCase();

const abbreviatePartOfSpeech = (partOfSpeech: string) =>
  PART_OF_SPEECH_ABBREVIATIONS[normalizePartOfSpeech(partOfSpeech)] ?? partOfSpeech;

const canSpeak =typeof window !== 'undefined' && 'speechSynthesis' in window;

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
  onNextMeaning: () => void;
  onPreviousMeaning: () => void;
  onRandom: () => void;
  onManage: () => void;
}

export const VocabCard: React.FC<VocabCardProps> = ({
  card,
  onNext,
  onPrevious,
  onNextMeaning,
  onPreviousMeaning,
  onRandom,
  onManage
}) => {
  const pronunciation = card.pronUk || card.pronUs;
  const cardRef = useRef<HTMLDivElement>(null);
  usePopupAutoResize(cardRef);
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
    <div className="vocab-card" ref={cardRef}>
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
                <SpeakerHigh size={15} weight="bold" aria-hidden />
              </button>
            )}
          </div>
          {pronunciation && (
            <p className="word-pronunciation">{pronunciation}</p>
          )}
          {/* Always rendered so showing the translation never shifts the card. */}
          <p className="translation word-translation">{visibleTranslations?.[0]}</p>
        </div>
        <span
          className="category"
          data-pos={normalizePartOfSpeech(card.partOfSpeech).replace(/\s+/g, '-')}
          title={card.partOfSpeech}
        >
          {abbreviatePartOfSpeech(card.partOfSpeech)}
        </span>
      </div>

      <div className="card-meta">
        {card.guideWord && <span className="guide-word">{card.guideWord}</span>}
        <div className="card-meta-end">
          {card.total > 1 && (
            <div className="meaning-stepper">
              <button
                onClick={onPreviousMeaning}
                className="meaning-step-btn"
                title="Previous meaning"
                aria-label="Previous meaning"
              >
                <CaretLeft size={12} weight="bold" aria-hidden />
              </button>
              <span className="card-position">
                {card.position} of {card.total}
              </span>
              <button
                onClick={onNextMeaning}
                className="meaning-step-btn"
                title="Next meaning"
                aria-label="Next meaning"
              >
                <CaretRight size={12} weight="bold" aria-hidden />
              </button>
            </div>
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
          <ArrowLeft size={13} weight="bold" aria-hidden />
          Previous
        </button>
        <button onClick={onRandom} className="control-btn random-btn">
          <Shuffle size={13} weight="bold" aria-hidden />
          Random
        </button>
        <button onClick={onNext} className="control-btn">
          Next
          <ArrowRight size={13} weight="bold" aria-hidden />
        </button>
      </div>

      <button onClick={onManage} className="manage-btn">
        Manage vocabulary
      </button>
    </div>
  );
};
