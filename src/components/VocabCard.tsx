import React from 'react';
import { VocabCardData } from '../types';

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

  return (
    <div className="vocab-card">
      <div className="word-header">
        <div className="word-title">
          <h1 className="word">{card.word}</h1>
          {pronunciation && (
            <p className="word-pronunciation">{pronunciation}</p>
          )}
        </div>
        <span className="category">{card.partOfSpeech}</span>
      </div>

      <div className="card-meta">
        {card.guideWord && <span className="guide-word">{card.guideWord}</span>}
        {card.total > 1 && (
          <span className="card-position">
            {card.position} of {card.total}
          </span>
        )}
      </div>

      <div className="meaning">
        <p>{card.definition}</p>
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
