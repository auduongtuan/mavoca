import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useVocab } from './hooks/useVocab';
import { VocabCard } from './components/VocabCard';
import { LoadingState } from './components/LoadingState';
import { ErrorState } from './components/ErrorState';
import { VocabManager } from './components/VocabManager';
import { SettingsPanel } from './components/SettingsPanel';
import { QuickAdd } from './components/QuickAdd';
import "./App.css";

function LearningApp() {
  const {
    card,
    loading,
    error,
    getRandomWord,
    getNextWord,
    getPreviousWord,
    getNextMeaning,
    getPreviousMeaning,
    reload
  } = useVocab();

  if (loading) {
    return <LoadingState />;
  }

  if (error) {
    return <ErrorState message={error} onRetry={reload} />;
  }

  if (!card) {
    return <ErrorState message="No vocabulary words found" onRetry={reload} />;
  }

  return (
    <main className="app">
      <VocabCard
        key={`${card.vocabId}-${card.position}`}
        card={card}
        onNext={getNextWord}
        onPrevious={getPreviousWord}
        onNextMeaning={getNextMeaning}
        onPreviousMeaning={getPreviousMeaning}
        onRandom={getRandomWord}
        onManage={() => invoke('show_manager_window')}
      />
    </main>
  );
}

function App() {
  if (getCurrentWindow().label === 'manager') {
    return <VocabManager />;
  }

  if (getCurrentWindow().label === 'settings') {
    return <SettingsPanel />;
  }

  if (getCurrentWindow().label === 'quick-add') {
    return <QuickAdd />;
  }

  return <LearningApp />;
}

export default App;
