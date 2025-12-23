import React from 'react';

interface ErrorStateProps {
  message: string;
  onRetry: () => void;
}

export const ErrorState: React.FC<ErrorStateProps> = ({ message, onRetry }) => {
  return (
    <div className="error-state">
      <h2>Oops! Something went wrong</h2>
      <p className="error-message">{message}</p>
      <button onClick={onRetry} className="retry-btn">
        Try Again
      </button>
    </div>
  );
};