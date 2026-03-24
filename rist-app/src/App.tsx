import { useCallback, useState, type CSSProperties } from 'react';

import { ActivityFeed } from './components/ActivityFeed';
import { AgentBar } from './components/AgentBar';
import { AgentCards } from './components/AgentCards';
import { AgentView } from './components/AgentView';
import { CommandPalette } from './components/CommandPalette';
import { DagPanel } from './components/DagPanel';
import { StatusBar } from './components/StatusBar';
import { useDaemon } from './hooks/use-daemon';
import { useKeyboard } from './hooks/use-keyboard';
import { useAgentStore } from './stores/agent-store';

export default function App() {
  useDaemon();
  useKeyboard();

  const showDag = useAgentStore((state) => state.showDag);
  const viewMode = useAgentStore((state) => state.viewMode);
  const showActivityFeed = useAgentStore((state) => state.showActivityFeed);
  const [split, setSplit] = useState(40);

  const onDividerMouseDown = useCallback(() => {
    const onMove = (event: MouseEvent) => {
      const nextSplit = (event.clientX / window.innerWidth) * 100;
      setSplit(Math.min(58, Math.max(24, nextSplit)));
    };

    const onUp = () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };

    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }, []);

  return (
    <main className="flex h-screen flex-col bg-zinc-950 text-zinc-100">
      <StatusBar />
      <div className="relative min-h-0 flex-1 overflow-hidden px-4 py-4">
        <div className="flex h-full gap-3">
          <div className="min-w-0 flex-1 rounded-[2rem] border border-zinc-800 bg-[radial-gradient(circle_at_top_left,rgba(245,158,11,0.08),transparent_38%),linear-gradient(180deg,rgba(24,24,27,0.96),rgba(9,9,11,0.98))] p-3 shadow-[0_24px_80px_rgba(0,0,0,0.35)]">
            {viewMode === 'cards' ? (
              <AgentCards />
            ) : (
              <div
                className={`grid h-full min-h-0 gap-3 ${
                  showDag ? 'grid-cols-[minmax(320px,var(--dag-width))_12px_minmax(480px,1fr)]' : 'grid-cols-1'
                }`}
                style={{ '--dag-width': `${split}%` } as CSSProperties}
              >
                {showDag ? <DagPanel /> : null}
                {showDag ? (
                  <button
                    aria-label="Resize panels"
                    className="group relative hidden cursor-col-resize rounded-full bg-zinc-900/50 md:block"
                    onMouseDown={onDividerMouseDown}
                    type="button"
                  >
                    <span className="absolute inset-y-8 left-1/2 w-px -translate-x-1/2 bg-zinc-700 transition group-hover:bg-zinc-500" />
                  </button>
                ) : null}
                <AgentView />
              </div>
            )}
          </div>
          {showActivityFeed ? (
            <div className="shrink-0 overflow-hidden rounded-[2rem] border border-zinc-800 bg-[linear-gradient(180deg,rgba(24,24,27,0.96),rgba(9,9,11,0.98))] shadow-[0_24px_80px_rgba(0,0,0,0.35)]">
              <ActivityFeed />
            </div>
          ) : null}
        </div>
        <CommandPalette />
      </div>
      <AgentBar />
    </main>
  );
}
