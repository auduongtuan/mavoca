import React, { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  ArrowLeft,
  ArrowRight,
  Books,
  CaretLeft,
  CaretRight,
  Shuffle,
  SpeakerHigh,
  Translate,
} from '@phosphor-icons/react';
import { CardTranslation, VocabCardData } from '../types';
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
 * Speaks the word with a built-in macOS voice. The card shows the UK and US
 * pronunciations of the entry the definition belongs to, so each one is read
 * in its own accent.
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

/**
 * One accent the card can show. Cambridge entry rows carry both; the macOS
 * dictionary only one, which is why the accent label appears only when the two
 * differ.
 */
interface Pronunciation {
  accent: 'UK' | 'US';
  ipa: string;
  lang: string;
}

const pronunciationsOf = (card: VocabCardData): Pronunciation[] => {
  const uk: Pronunciation | null = card.pronUk
    ? { accent: 'UK', ipa: card.pronUk, lang: UK_VOICE_LANG }
    : null;
  const us: Pronunciation | null = card.pronUs
    ? { accent: 'US', ipa: card.pronUs, lang: US_VOICE_LANG }
    : null;

  if (uk && us && uk.ipa === us.ipa) {
    return [uk];
  }

  return [uk, us].filter((pronunciation): pronunciation is Pronunciation => pronunciation !== null);
};

/**
 * Keeps the Vietnamese of the definitions stepped through in this session.
 * Stepping remounts the card, and without it the meaning would blank out while
 * Rust answered again; that happens on every definition of a word.
 */
const translationCache = new Map<string, CardTranslation>();
const MAX_CACHED_TRANSLATIONS = 200;

const translationKey = (card: VocabCardData) =>
  `${card.word}\u0000${card.partOfSpeech}\u0000${card.definition}`;

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
  const pronunciations = pronunciationsOf(card);
  const cardKey = translationKey(card);
  const cardRef = useRef<HTMLDivElement>(null);
  usePopupAutoResize(cardRef);
  // The parent keys this component by card, so a new card starts untranslated —
  // unless this session already translated that definition.
  const [translation, setTranslation] = useState<CardTranslation | null>(
    () => translationCache.get(cardKey) ?? null,
  );
  const [showTranslation, setShowTranslation] = useState(false);
  const [translating, setTranslating] = useState(false);
  const [translateError, setTranslateError] = useState<string | null>(null);
  const requestedTranslation = useRef(false);

  // One call gives both the Lạc Việt meaning, shown on the card by default,
  // and the translated definition behind VI. Picking the Lạc Việt sense needs
  // the translated definition, which takes about a second, longer after the
  // translator's model was unloaded; Rust caches the result too.
  const loadTranslation = useCallback(async () => {
    if (requestedTranslation.current) {
      return;
    }
    requestedTranslation.current = true;

    const cached = translationCache.get(cardKey);
    if (cached) {
      setTranslation(cached);
      return;
    }

    setTranslating(true);
    setTranslateError(null);
    try {
      const result = await invoke<CardTranslation>('translate_card', {
        word: card.word,
        partOfSpeech: card.partOfSpeech,
        definition: card.definition,
      });
      setTranslation(result);
      if (translationCache.size >= MAX_CACHED_TRANSLATIONS) {
        translationCache.clear();
      }
      translationCache.set(cardKey, result);
    } catch (err) {
      requestedTranslation.current = false;
      setTranslateError(err instanceof Error ? err.message : String(err));
    } finally {
      setTranslating(false);
    }
  }, [cardKey, card.word, card.partOfSpeech, card.definition]);

  // Load while the popup is open; the hidden rotation every few seconds
  // mounts cards nobody sees, and those are skipped.
  useEffect(() => {
    const appWindow = getCurrentWindow();
    appWindow.isVisible().then((visible) => {
      if (visible) {
        void loadTranslation();
      }
    }).catch(() => {});
    const unlisten = appWindow.onFocusChanged(({ payload: focused }) => {
      if (focused) {
        void loadTranslation();
      }
    });
    return () => {
      unlisten.then((stop) => stop());
    };
  }, [loadTranslation]);

  const toggleTranslation = () => {
    setShowTranslation((shown) => !shown);
    void loadTranslation();
  };

  const meaning = translation?.meaning;
  const visibleTranslation = showTranslation ? translation : null;

  return (
    <div className="vocab-card" ref={cardRef}>
      <div className="word-header">
        <div className="word-header-top">
          <div className="word-line">
            <h1 className="word">{card.word}</h1>
            {canSpeak && pronunciations.length === 0 && (
              <button
                className="speak-btn"
                onClick={() => speakWord(card.word, US_VOICE_LANG)}
                title="Pronounce"
                aria-label={`Pronounce ${card.word}`}
              >
                <SpeakerHigh size={15} weight="bold" aria-hidden />
              </button>
            )}
          </div>
          {/* Above the part of speech and the transcription, which it changes:
              stepping the definitions steps the entry, not only the text below
              the divider. */}
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
        </div>
        <div className="word-entry">
          <span
            className="category"
            data-pos={normalizePartOfSpeech(card.partOfSpeech).replace(/\s+/g, '-')}
            title={card.partOfSpeech}
          >
            {abbreviatePartOfSpeech(card.partOfSpeech)}
          </span>
          {pronunciations.length > 0 && (
            <p className="word-pronunciation">
              {pronunciations.map(({ accent, ipa, lang }) => (
                <span className="pronunciation" key={accent}>
                  {pronunciations.length > 1 && (
                    <span className="pronunciation-accent">{accent}</span>
                  )}
                  <span className="pronunciation-ipa">{ipa}</span>
                  {canSpeak && (
                    <button
                      className="pronunciation-speak"
                      onClick={() => speakWord(card.word, lang)}
                      title={`Pronounce in ${accent} English`}
                      aria-label={`Pronounce ${card.word} in ${accent} English`}
                    >
                      <SpeakerHigh size={12} weight="bold" aria-hidden />
                    </button>
                  )}
                </span>
              ))}
            </p>
          )}
        </div>
      </div>

      <div className="card-meta">
        <button
          onClick={toggleTranslation}
          className={showTranslation ? 'translate-btn active' : 'translate-btn'}
          title={showTranslation ? 'Hide the Vietnamese definition' : 'Translate the definition to Vietnamese on this Mac'}
          aria-label={showTranslation ? 'Hide the Vietnamese definition' : 'Translate the definition to Vietnamese'}
          aria-pressed={showTranslation}
        >
          <Translate size={12} weight="bold" aria-hidden />
          VI
        </button>
        <div className="card-meta-end">
          {card.guideWord && <span className="guide-word">{card.guideWord}</span>}
        </div>
      </div>

      <div className="meaning">
        <p>{card.definition}</p>
        {/* Under the definition rather than in the header, so the header and
            its divider never move when the Vietnamese appears. */}
        {meaning && (
          <p className="dictionary-meaning" title="Từ điển Lạc Việt">
            <strong>{meaning.headline}</strong>
            {meaning.explanation && ` ${meaning.explanation}`}
          </p>
        )}
        {visibleTranslation?.word && (
          <p className="translation word-translation" title="Machine translation">
            {visibleTranslation.word}
          </p>
        )}
        {visibleTranslation?.definition && (
          <p className="translation">{visibleTranslation.definition}</p>
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
        <div className="control-group">
          <button onClick={onPrevious} className="control-btn" title="Previous" aria-label="Previous">
            <ArrowLeft size={15} weight="bold" aria-hidden />
          </button>
          <button onClick={onNext} className="control-btn" title="Next" aria-label="Next">
            <ArrowRight size={15} weight="bold" aria-hidden />
          </button>
        </div>
        <button onClick={onRandom} className="control-btn random-btn" title="Random" aria-label="Random">
          <Shuffle size={15} weight="bold" aria-hidden />
        </button>
        <button
          onClick={onManage}
          className="control-btn manage-btn"
          title="Manage vocabulary"
          aria-label="Manage vocabulary"
        >
          <Books size={15} weight="bold" aria-hidden />
        </button>
      </div>
    </div>
  );
};
