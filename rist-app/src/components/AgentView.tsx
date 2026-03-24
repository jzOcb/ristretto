import { Suspense, lazy, useEffect, useMemo, useRef, useState } from 'react';

import { parseOutput } from '../lib/output-parser';
import { agentTypeLabel } from '../lib/types';
import { useAgentStore } from '../stores/agent-store';
import { CommandCard } from './CommandCard';
import { DiffViewer } from './DiffViewer';
import { ErrorCard } from './ErrorCard';
import { TestResults } from './TestResults';

const RawTerminal = lazy(() => import('./RawTerminal'));

export const AgentView = () => {
  const agents = useAgentStore((state) => state.agents);
  const outputs = useAgentStore((state) => state.outputs);
  const selectedAgentId = useAgentStore((state) => state.selectedAgentId);
  const rawMode = useAgentStore((state) => state.rawMode);
  const toggleRawMode = useAgentStore((state) => state.toggleRawMode);
  const selectedAgent = agents.find((agent) => agent.id === selectedAgentId) ?? null;
  const output = selectedAgentId ? outputs[selectedAgentId] ?? '' : '';
  const blocks = useMemo(() => parseOutput(output), [output]);
  const listRef = useRef<HTMLDivElement | null>(null);
  const [showJump, setShowJump] = useState(false);

  useEffect(() => {
    const element = listRef.current;
    if (!element || rawMode) {
      return;
    }
    element.scrollTop = element.scrollHeight;
  }, [blocks, rawMode]);

  useEffect(() => {
    const element = listRef.current;
    if (!element) {
      return;
    }

    const onScroll = () => {
      const threshold = element.scrollHeight - element.scrollTop - element.clientHeight;
      setShowJump(threshold > 160);
    };

    onScroll();
    element.addEventListener('scroll', onScroll);
    return () => element.removeEventListener('scroll', onScroll);
  }, [selectedAgentId]);

  return (
    <section className="panel-shell min-h-[420px]">
      <div className="panel-header">
        <div>
          <p className="panel-eyebrow">Agent Stream</p>
          <h2 className="panel-title">{selectedAgent?.task ?? 'No agent selected'}</h2>
          <p className="mt-1 text-sm text-zinc-400">
            {selectedAgent
              ? `${agentTypeLabel(selectedAgent.agent_type)} · ${selectedAgent.workdir}`
              : 'Select an agent or spawn one to inspect structured output.'}
          </p>
        </div>
        <button
          className="rounded-full border border-zinc-700 px-3 py-2 text-xs uppercase tracking-[0.18em] text-zinc-200 transition hover:border-zinc-500"
          onClick={toggleRawMode}
          type="button"
        >
          {rawMode ? 'Structured View' : 'Raw Terminal'}
        </button>
      </div>

      {!selectedAgent ? (
        <div className="flex min-h-[420px] items-center justify-center px-8 py-12 text-center text-zinc-500">
          The agent viewport is waiting for a session. Use the bottom bar or `Cmd+T` to spawn one.
        </div>
      ) : rawMode ? (
        <div className="h-[calc(100%-5rem)] px-5 pb-5">
          <Suspense fallback={<div className="rounded-2xl bg-zinc-950 p-4 text-sm text-zinc-500">Loading terminal...</div>}>
            <RawTerminal agentId={selectedAgent.id} output={output} />
          </Suspense>
        </div>
      ) : (
        <div className="relative h-[calc(100%-5rem)]">
          <div className="h-full space-y-4 overflow-auto px-5 pb-5" ref={listRef}>
            {blocks.map((block) => {
              switch (block.type) {
                case 'command':
                  return <CommandCard block={block} key={block.id} />;
                case 'diff':
                  return <DiffViewer block={block} key={block.id} />;
                case 'test':
                  return <TestResults block={block} key={block.id} />;
                case 'error':
                  return <ErrorCard block={block} key={block.id} />;
                case 'text':
                  return (
                    <article className="rounded-2xl border border-zinc-800 bg-zinc-900/70 px-4 py-3" key={block.id}>
                      <pre className="whitespace-pre-wrap font-mono text-xs leading-6 text-zinc-300">{block.text}</pre>
                    </article>
                  );
                default:
                  return null;
              }
            })}
          </div>

          {showJump ? (
            <button
              className="absolute bottom-8 right-8 rounded-full border border-zinc-700 bg-zinc-900/90 px-4 py-2 text-xs uppercase tracking-[0.18em] text-zinc-100 shadow-lg backdrop-blur"
              onClick={() => {
                listRef.current?.scrollTo({ top: listRef.current.scrollHeight, behavior: 'smooth' });
              }}
              type="button"
            >
              Jump to bottom
            </button>
          ) : null}
        </div>
      )}
    </section>
  );
};
