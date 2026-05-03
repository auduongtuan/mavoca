import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useVocab } from './hooks/useVocab';
import { VocabCard } from './components/VocabCard';
import { LoadingState } from './components/LoadingState';
import { ErrorState } from './components/ErrorState';
import { VocabManager } from './components/VocabManager';
import { SettingsPanel } from './components/SettingsPanel';
import "./App.css";

function LearningApp() {
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

  return <LearningApp />;
}

export default App;
