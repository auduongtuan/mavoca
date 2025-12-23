import React from 'react';

export const LoadingState: React.FC = () => {
  return (
    <div className="loading-state">
      <div className="loading-spinner"></div>
      <p>Loading vocabulary...</p>
    </div>
  );
};