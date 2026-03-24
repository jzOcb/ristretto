import { Suspense, lazy, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

import type { AgentInfo } from '../lib/types';
import { parseOutput } from '../lib/output-parser';
import { agentTypeLabel } from '../lib/types';
import { useAgentStore } from '../stores/agent-store';
import { CommandCard } from './CommandCard';
import { DiffViewer } from './DiffViewer';
import { ErrorCard } from './ErrorCard';
import { TestResults } from './TestResults';

const RawTerminal = lazy(() => import('./RawTerminal'));

const SPAWN_RE = /^rist\s+spawn\s+(\w+)\s+"(.+)"$/;

export const AgentView = () => {
  const agents = useAgentStore((state) => state.agents);
  const outputs = useAgentStore((state) => state.outputs);
  const selectedAgentId = useAgentStore((state) => state.selectedAgentId);
  const rawMode = useAgentStore((state) => state.rawMode);
  const toggleRawMode = useAgentStore((state) => state.toggleRawMode);
  const commandInput = useAgentStore((state) => state.commandInput);
  const setCommandInput = useAgentStore((state) => state.setCommandInput);
  const setAgents = useAgentStore((state) => state.setAgents);
  const replaceOutput = useAgentStore((state) => state.replaceOutput);
  const selectAgent = useAgentStore((state) => state.selectAgent);

  const selectedAgent = agents.find((agent) => agent.id === selectedAgentId) ?? null;
  const output = selectedAgentId ? outputs[selectedAgentId] ?? '' : '';
  const blocks = useMemo(() => parseOutput(output), [output]);
  const listRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
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

  const submitCommand = async () => {
    const value = commandInput.trim();
    if (!value) return;

    const spawnMatch = value.match(SPAWN_RE);
    if (spawnMatch) {
      const agentType = { kind: spawnMatch[1] as 'claude' | 'codex' | 'gemini' };
      const task = spawnMatch[2];
      const id = await invoke<string>('spawn_agent', { agentType, task });
      const nextAgents = await invoke<AgentInfo[]>('list_agents');
      setAgents(nextAgents);
      await Promise.all(
        nextAgents.map(async (agent) => {
          const buffer = await invoke<string>('get_agent_buffer', { agentId: agent.id });
          replaceOutput(agent.id, buffer);
        }),
      );
      selectAgent(id);
    } else if (selectedAgentId) {
      const bytes = Array.from(new TextEncoder().encode(value + '\n'));
      await invoke('write_to_pty', { agentId: selectedAgentId, data: bytes });
    }

    setCommandInput('');
  };

  const placeholder = selectedAgent
    ? `Send to ${agentTypeLabel(selectedAgent.agent_type)} or rist spawn claude "task"`
    : 'rist spawn claude "Build auth module"';

  return (
    <section className="panel-shell flex min-h-[420px] flex-col">
      <div className="panel-header">
        <div>
          <p className="panel-eyebrow">Agent Stream</p>
          <h2 className="panel-title">{selectedAgent?.task ?? 'No agent selected'}</h2>
          <p className="mt-0.5 text-xs text-zinc-500">
            {selectedAgent
              ? `${agentTypeLabel(selectedAgent.agent_type)} · ${selectedAgent.workdir}`
              : 'Select an agent or spawn one to inspect structured output.'}
          </p>
        </div>
        <button
          className="rounded-md border border-zinc-700/60 px-2.5 py-1.5 text-[10px] font-medium uppercase tracking-wider text-zinc-400 transition hover:border-zinc-500 hover:text-zinc-200"
          onClick={toggleRawMode}
          type="button"
        >
          {rawMode ? 'Structured' : 'Raw'}
        </button>
      </div>

      <div className="relative min-h-0 flex-1">
        {!selectedAgent ? (
          <div className="flex h-full items-center justify-center px-8 py-12 text-center text-sm text-zinc-600">
            The agent viewport is waiting for a session. Use <kbd className="mx-1 rounded border border-zinc-700 bg-zinc-800 px-1.5 py-0.5 font-mono text-[10px] text-zinc-400">⌘T</kbd> to spawn one.
          </div>
        ) : rawMode ? (
          <div className="h-full px-4 pb-4">
            <Suspense fallback={<div className="rounded-lg bg-zinc-950 p-4 text-xs text-zinc-600">Loading terminal...</div>}>
              <RawTerminal agentId={selectedAgent.id} output={output} />
            </Suspense>
          </div>
        ) : (
          <>
            <div className="h-full space-y-3 overflow-auto px-4 pb-4" ref={listRef}>
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
                      <article className="rounded-lg border border-zinc-800/60 bg-zinc-900/60 px-3 py-2.5" key={block.id}>
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
                className="absolute bottom-6 right-6 rounded-md border border-zinc-700 bg-zinc-900/90 px-3 py-1.5 text-[10px] font-medium uppercase tracking-wider text-zinc-300 shadow-lg backdrop-blur-sm"
                onClick={() => {
                  listRef.current?.scrollTo({ top: listRef.current.scrollHeight, behavior: 'smooth' });
                }}
                type="button"
              >
                Jump to bottom
              </button>
            ) : null}
          </>
        )}
      </div>

      {/* Persistent Command Input Bar */}
      <div className="border-t border-zinc-800/40 bg-zinc-950/80 px-3 py-2.5">
        <div className="flex items-center gap-2 rounded-lg border border-zinc-700/40 bg-zinc-900/60 px-3 py-2 transition-colors focus-within:border-violet-500/50 focus-within:shadow-[0_0_0_1px_rgba(139,92,246,0.15)]">
          <span className="select-none text-xs text-violet-400/70">$</span>
          <input
            ref={inputRef}
            className="min-w-0 flex-1 bg-transparent font-mono text-sm text-zinc-200 outline-none placeholder:text-zinc-600"
            onChange={(event) => setCommandInput(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter' && !event.shiftKey) {
                event.preventDefault();
                void submitCommand();
              }
            }}
            placeholder={placeholder}
            value={commandInput}
          />
          <button
            className="rounded-md px-2 py-1 text-[10px] font-medium text-zinc-500 transition hover:text-violet-400 disabled:opacity-30"
            disabled={!commandInput.trim()}
            onClick={() => void submitCommand()}
            type="button"
          >
            <kbd className="rounded border border-zinc-700 bg-zinc-800 px-1.5 py-0.5 font-mono">↵</kbd>
          </button>
        </div>
      </div>
    </section>
  );
};
