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
import { LacVietMeaning, VocabCardData } from '../types';
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
 * Definitions translated in this session, keyed by the English text: stepping
 * remounts the card, and without it the VI panel would empty until Rust
 * answered again. Keyed on text rather than the row id, because SQLite reuses
 * the id of a deleted and re-inserted definition.
 */
const translationCache = new Map<string, string>();
const MAX_CACHED_TRANSLATIONS = 200;

const translationKey = (card: VocabCardData) =>
  `${card.word}\u0000${card.partOfSpeech}\u0000${card.definition}`;

/** Senses shown before the section has to be expanded. */
const SENSES_SHOWN = 3;

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

  // Lạc Việt's entry for this card. It is a local lookup, so it shows as soon
  // as the card does; `undefined` until it arrives, `null` when Lạc Việt has no
  // entry for the word.
  const [lacViet, setLacViet] = useState<LacVietMeaning | null | undefined>(undefined);
  const [showAllSenses, setShowAllSenses] = useState(false);

  // Behind the VI button: the definition translated, and the word itself when
  // Lạc Việt has no entry to show instead.
  const [definitionVi, setDefinitionVi] = useState<string>(
    () => translationCache.get(cardKey) ?? '',
  );
  const [wordVi, setWordVi] = useState('');
  const [showTranslation, setShowTranslation] = useState(false);
  const [translating, setTranslating] = useState(false);
  const [translateError, setTranslateError] = useState<string | null>(null);
  const requestedTranslation = useRef(false);

  // Fetch the entry on its own, so the Vietnamese section never waits on the
  // translator. The definition on screen is only a hint about which sense is
  // closest: Rust uses the translation already stored with the row, if any, and
  // leaves Lạc Việt's own order alone otherwise.
  const loadLacViet = useCallback(async () => {
    try {
      const result = await invoke<LacVietMeaning | null>('lac_viet_meaning', {
        word: card.word,
        partOfSpeech: card.partOfSpeech,
        definitionId: card.definitionId,
      });
      setLacViet(result);
    } catch {
      // A Mac without the dictionary leaves the section out; the English card
      // is unaffected, so there is nothing to report.
      setLacViet(null);
    }
  }, [card.word, card.partOfSpeech, card.definitionId]);

  // Eager, so the translation is stored with the definition and the VI button
  // is instant afterwards. Takes about a second the first time.
  const loadTranslation = useCallback(async () => {
    if (requestedTranslation.current) {
      return;
    }
    requestedTranslation.current = true;

    const cached = translationCache.get(cardKey);
    if (cached) {
      setDefinitionVi(cached);
      return;
    }

    setTranslating(true);
    setTranslateError(null);
    try {
      const result = await invoke<string>('translate_definition', {
        definition: card.definition,
        definitionId: card.definitionId,
      });
      setDefinitionVi(result);
      if (result) {
        if (translationCache.size >= MAX_CACHED_TRANSLATIONS) {
          translationCache.clear();
        }
        translationCache.set(cardKey, result);
        // Rust stored it with the row, so asking again now ranks the senses
        // against it. The first sight of a definition shows Lạc Việt's own
        // order, which is the honest fallback while there is nothing to
        // compare against.
        void loadLacViet();
      }
    } catch (err) {
      requestedTranslation.current = false;
      setTranslateError(err instanceof Error ? err.message : String(err));
    } finally {
      setTranslating(false);
    }
  }, [cardKey, card.definition, card.definitionId, loadLacViet]);

  // Takes the word through the translator, for the case where Lạc Việt has no
  // entry and the VI button is all there is to show.
  const loadWordTranslation = useCallback(async () => {
    setTranslating(true);
    setTranslateError(null);
    try {
      setWordVi(await invoke<string>('translate_word', { word: card.word }));
    } catch (err) {
      setTranslateError(err instanceof Error ? err.message : String(err));
    } finally {
      setTranslating(false);
    }
  }, [card.word]);

  // Load while the popup is open; the hidden rotation every few seconds
  // mounts cards nobody sees, and those are skipped.
  useEffect(() => {
    const appWindow = getCurrentWindow();
    appWindow.isVisible().then((visible) => {
      if (visible) {
        void loadLacViet();
        void loadTranslation();
      }
    }).catch(() => {});
    const unlisten = appWindow.onFocusChanged(({ payload: focused }) => {
      if (focused) {
        void loadLacViet();
        void loadTranslation();
      }
    });
    return () => {
      unlisten.then((stop) => stop());
    };
  }, [loadLacViet, loadTranslation]);

  const toggleTranslation = () => {
    const shown = !showTranslation;
    setShowTranslation(shown);
    void loadTranslation();
    // Nothing from Lạc Việt to show: the word itself is the only Vietnamese
    // there is, and it costs a second translation, so only on request.
    if (shown && lacViet === null && !wordVi) {
      void loadWordTranslation();
    }
  };

  const senses = lacViet?.senses ?? [];
  const visibleSenses = showAllSenses ? senses : senses.slice(0, SENSES_SHOWN);
  const hiddenSenses = senses.length - SENSES_SHOWN;

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
        {/* Two sections rather than one block: the definition follows the
            stepper, the Vietnamese entry does not. Lạc Việt numbers its senses
            its own way and merges homographs under one part of speech, so a
            sense belongs to the word's entry, not to the definition on screen. */}
        <section className="card-section">
          <h2 className="section-label">English</h2>
          <p>{card.definition}</p>
          {showTranslation && definitionVi && (
            <p className="translation">{definitionVi}</p>
          )}
          {showTranslation && translating && (
            <p className="translation-status">Translating…</p>
          )}
          {showTranslation && translateError && (
            <p className="translation-status error">{translateError}</p>
          )}
        </section>

        {senses.length > 0 ? (
          <section className="card-section dictionary-section">
            <h2 className="section-label">
              Tiếng Việt
              <span className="dictionary-source">
                Lạc Việt{lacViet?.partOfSpeechLabel ? ` · ${lacViet.partOfSpeechLabel}` : ''}
              </span>
            </h2>
            <ul className="sense-list">
              {visibleSenses.map((sense, index) => (
                <li
                  className={index === 0 && lacViet?.ranked ? 'sense closest' : 'sense'}
                  key={`${index}-${sense.headline}`}
                >
                  <strong>{sense.headline}</strong>
                  {sense.explanation && ` ${sense.explanation}`}
                </li>
              ))}
            </ul>
            {hiddenSenses > 0 && (
              <button
                className="sense-toggle"
                onClick={() => setShowAllSenses((shown) => !shown)}
                aria-expanded={showAllSenses}
              >
                {showAllSenses ? 'Show fewer' : `+${hiddenSenses} more`}
              </button>
            )}
          </section>
        ) : (
          showTranslation &&
          wordVi && (
            <section className="card-section dictionary-section">
              <h2 className="section-label">
                Tiếng Việt
                <span className="dictionary-source">máy dịch</span>
              </h2>
              <p className="word-translation">{wordVi}</p>
            </section>
          )
        )}
      </div>

      {card.examples.length > 0 && (
        <div className="example">
          <h2 className="section-label">Example</h2>
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
