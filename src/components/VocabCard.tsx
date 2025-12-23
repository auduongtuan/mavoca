import React from 'react';
import { VocabWord } from '../types';

interface VocabCardProps {
  word: VocabWord;
  onNext: () => void;
  onPrevious: () => void;
  onRandom: () => void;
}

export const VocabCard: React.FC<VocabCardProps> = ({ 
  word, 
  onNext, 
  onPrevious, 
  onRandom 
}) => {
  return (
    <div className="vocab-card">
      <div className="word-header">
        <h1 className="word">{word.word}</h1>
        <span className="category">{word.category}</span>
      </div>
      
      <div className="meaning">
        <p>{word.meaning}</p>
      </div>
      
      <div className="example">
        <h3>Example:</h3>
        <p className="example-text">"{word.example}"</p>
      </div>
      
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
    </div>
  );
};