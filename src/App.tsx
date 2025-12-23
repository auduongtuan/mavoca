import { useVocab } from './hooks/useVocab';
import { VocabCard } from './components/VocabCard';
import { LoadingState } from './components/LoadingState';
import { ErrorState } from './components/ErrorState';
import "./App.css";

function App() {
  const { 
    currentWord, 
    loading, 
    error, 
    getRandomWord, 
    getNextWord, 
    getPreviousWord, 
    reload 
  } = useVocab();

  if (loading) {
    return <LoadingState />;
  }

  if (error) {
    return <ErrorState message={error} onRetry={reload} />;
  }

  if (!currentWord) {
    return <ErrorState message="No vocabulary words found" onRetry={reload} />;
  }

  return (
    <main className="app">
      <VocabCard
        word={currentWord}
        onNext={getNextWord}
        onPrevious={getPreviousWord}
        onRandom={getRandomWord}
      />
    </main>
  );
}

export default App;
