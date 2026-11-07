import React, { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  ArrowLeft,
  ArrowRight,
  CaretLeft,
  CaretRight,
  Check,
  Shuffle,
  SpeakerHigh,
  Translate,
} from '@phosphor-icons/react';
import { LacVietMeaning, VocabCardData } from '../types';
import { usePopupAutoResize } from '../hooks/usePopupAutoResize';
import { IconButton } from './ui/IconButton';

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

/**
 * The same, for the card's examples. Keyed on the examples themselves rather
 * than on the card, because the translations line up with them positionally:
 * an edited list must not read back the previous one's translations.
 */
const exampleTranslationCache = new Map<string, string[]>();

const exampleKey = (card: VocabCardData) => card.examples.join('\u0000');

/** Senses shown before the section has to be expanded. */
const SENSES_SHOWN = 3;

interface VocabCardProps {
  card: VocabCardData;
  onNext: () => void;
  onPrevious: () => void;
  onNextMeaning: () => void;
  onPreviousMeaning: () => void;
  onRandom: () => void;
  /** Scores the word as remembered and moves on to the next one. */
  onRemembered: () => void;
}

export const VocabCard: React.FC<VocabCardProps> = ({
  card,
  onNext,
  onPrevious,
  onNextMeaning,
  onPreviousMeaning,
  onRandom,
  onRemembered
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
  const [examplesVi, setExamplesVi] = useState<string[]>(
    () => exampleTranslationCache.get(exampleKey(card)) ?? [],
  );
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

  // Lazy, unlike the definition: the examples order nothing, so they are
  // translated when VI is pressed rather than when the card appears. Rust
  // stores them with the definition, so this costs the translator once ever.
  const loadExampleTranslations = useCallback(async () => {
    if (card.examples.length === 0) {
      return;
    }

    const key = exampleKey(card);
    const cached = exampleTranslationCache.get(key);
    if (cached) {
      setExamplesVi(cached);
      return;
    }

    try {
      const result = await invoke<string[]>('translate_examples', {
        examples: card.examples,
        definitionId: card.definitionId,
      });
      setExamplesVi(result);
      if (result.some((text) => text)) {
        if (exampleTranslationCache.size >= MAX_CACHED_TRANSLATIONS) {
          exampleTranslationCache.clear();
        }
        exampleTranslationCache.set(key, result);
      }
    } catch (err) {
      // The definition's own status line already carries the translator's
      // complaint; a second copy of it under the examples says nothing new.
      setTranslateError(err instanceof Error ? err.message : String(err));
    }
  }, [card]);

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
    if (shown) {
      void loadExampleTranslations();
    }
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
            <span
              className="category"
              data-pos={normalizePartOfSpeech(card.partOfSpeech).replace(/\s+/g, '-')}
              title={card.partOfSpeech}
            >
              {abbreviatePartOfSpeech(card.partOfSpeech)}
            </span>
            {canSpeak && pronunciations.length === 0 && (
              <IconButton
                size="small"
                onClick={() => speakWord(card.word, US_VOICE_LANG)}
                label={`Pronounce ${card.word}`}
              >
                <SpeakerHigh size={15} weight="bold" aria-hidden />
              </IconButton>
            )}
          </div>
          {/* Beside the word, the part of speech, and the transcription below,
              all of which it changes: stepping the definitions steps the whole
              entry, not only the text below the divider. */}
          {card.total > 1 && (
            <div className="meaning-stepper">
              <IconButton size="small" onClick={onPreviousMeaning} label="Previous meaning">
                <CaretLeft size={12} weight="bold" aria-hidden />
              </IconButton>
              <span className="card-position">
                {card.position} of {card.total}
              </span>
              <IconButton size="small" onClick={onNextMeaning} label="Next meaning">
                <CaretRight size={12} weight="bold" aria-hidden />
              </IconButton>
            </div>
          )}
        </div>
        {pronunciations.length > 0 && (
          <p className="word-pronunciation">
            {pronunciations.map(({ accent, ipa, lang }) => (
              <span className="pronunciation" key={accent}>
                {pronunciations.length > 1 && (
                  <span className="pronunciation-accent">{accent}</span>
                )}
                <span className="pronunciation-ipa">{ipa}</span>
                {canSpeak && (
                  <IconButton
                    size="small"
                    onClick={() => speakWord(card.word, lang)}
                    label={`Pronounce ${card.word} in ${accent} English`}
                  >
                    <SpeakerHigh size={12} weight="bold" aria-hidden />
                  </IconButton>
                )}
              </span>
            ))}
          </p>
        )}
      </div>

      {/* Only the guide word is left here, and not every card has one: an
          empty row would still spend the card's gap. */}
      {card.guideWord && (
        <div className="card-meta">
          <span className="guide-word">{card.guideWord}</span>
        </div>
      )}

      <div className="meaning">
        {/* Two sections rather than one block: the definition follows the
            stepper, the Vietnamese entry does not. Lạc Việt numbers its senses
            its own way and merges homographs under one part of speech, so a
            sense belongs to the word's entry, not to the definition on screen. */}
        <section className="card-section">
          {/* VI sits on this heading because this is the section it changes:
              it adds the Vietnamese under the definition and its examples. */}
          <div className="section-header">
            <h2 className="section-label">English</h2>
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
          </div>
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
          {/* The examples belong to the definition above them, so they live
              inside the English section and are replaced along with it when
              the stepper moves. */}
          {card.examples.length > 0 && (
            <div className="example">
              <h3 className="section-sublabel">Example</h3>
              {card.examples.map((example, index) => (
                <React.Fragment key={index}>
                  <p className="example-text">"{example}"</p>
                  {showTranslation && examplesVi[index] && (
                    <p className="example-translation">{examplesVi[index]}</p>
                  )}
                </React.Fragment>
              ))}
            </div>
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

      {/* Moving around the vocabulary is on the left and stays quiet; the one
          thing that changes something is on the right. The way out to the
          manager lives in the tray menu rather than here. */}
      <div className="controls">
        <div className="control-group">
          <IconButton onClick={onPrevious} label="Previous word">
            <ArrowLeft size={15} weight="bold" aria-hidden />
          </IconButton>
          <IconButton onClick={onNext} label="Next word">
            <ArrowRight size={15} weight="bold" aria-hidden />
          </IconButton>
          <IconButton onClick={onRandom} label="Random word">
            <Shuffle size={15} weight="bold" aria-hidden />
          </IconButton>
        </div>
        {/* The score itself lives in the manager: pressing this moves to the
            next word, which is the feedback, and the card stays a card. */}
        <button
          onClick={onRemembered}
          className="got-it-btn"
          title="Remembered this one — show the next word"
          aria-label="Remembered this word"
        >
          <Check size={14} weight="bold" aria-hidden />
          Got it
        </button>
      </div>
    </div>
  );
};
