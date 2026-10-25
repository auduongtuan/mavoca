import { invoke } from '@tauri-apps/api/core';
import { ScrollArea } from '@base-ui/react/scroll-area';
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

  // The card is taller than the window only when it has more to show than the
  // screen allows. The scroll area draws its own overlay bar, so that case costs
  // the card no width and the text cannot shift when the bar comes and goes.
  return (
    <main className="app">
      <ScrollArea.Root className="app-scroll-area">
        <ScrollArea.Viewport className="app-viewport">
          <div className="app-body">
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
          </div>
        </ScrollArea.Viewport>
        <ScrollArea.Scrollbar className="app-scrollbar" orientation="vertical">
          <ScrollArea.Thumb className="app-scrollbar-thumb" />
        </ScrollArea.Scrollbar>
      </ScrollArea.Root>
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
