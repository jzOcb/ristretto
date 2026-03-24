import { useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

import type { AgentInfo } from '../lib/types';
import { agentTypeLabel } from '../lib/types';
import { useAgentStore } from '../stores/agent-store';

export const CommandPalette = () => {
  const [query, setQuery] = useState('');
  const agents = useAgentStore((state) => state.agents);
  const paletteOpen = useAgentStore((state) => state.paletteOpen);
  const selectedAgentId = useAgentStore((state) => state.selectedAgentId);
  const setPaletteOpen = useAgentStore((state) => state.setPaletteOpen);
  const setSpawnOpen = useAgentStore((state) => state.setSpawnOpen);
  const setAgents = useAgentStore((state) => state.setAgents);
  const selectAgent = useAgentStore((state) => state.selectAgent);
  const toggleRawMode = useAgentStore((state) => state.toggleRawMode);
  const toggleDag = useAgentStore((state) => state.toggleDag);

  const actions = useMemo(() => {
    const baseActions = [
      {
        id: 'spawn',
        label: 'Spawn agent',
        keywords: 'new create task',
        run: () => setSpawnOpen(true),
      },
      {
        id: 'toggle-raw',
        label: 'Toggle raw terminal',
        keywords: 'terminal output structured',
        run: () => toggleRawMode(),
      },
      {
        id: 'toggle-dag',
        label: 'Toggle DAG panel',
        keywords: 'graph tasks layout',
        run: () => toggleDag(),
      },
    ];

    const agentActions = agents.flatMap((agent) => [
      {
        id: `switch-${agent.id}`,
        label: `Switch to ${agent.task}`,
        keywords: `${agent.task} ${agentTypeLabel(agent.agent_type)} ${agent.status}`,
        run: () => selectAgent(agent.id),
      },
      {
        id: `kill-${agent.id}`,
        label: `Kill ${agent.task}`,
        keywords: `${agent.task} stop close terminate`,
        run: async () => {
          await invoke('kill_agent', { agentId: agent.id });
          const nextAgents = await invoke<AgentInfo[]>('list_agents');
          setAgents(nextAgents);
        },
      },
    ]);

    return [...baseActions, ...agentActions];
  }, [agents, selectAgent, setAgents, setSpawnOpen, toggleDag, toggleRawMode]);

  const filteredActions = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return actions.filter((action) =>
      !normalized
        ? true
        : `${action.label} ${action.keywords}`.toLowerCase().includes(normalized),
    );
  }, [actions, query]);

  if (!paletteOpen) {
    return null;
  }

  return (
    <div className="absolute inset-0 z-40 flex items-start justify-center bg-zinc-950/60 px-4 pt-[12vh] backdrop-blur-sm">
      <div className="w-full max-w-xl overflow-hidden rounded-xl border border-zinc-700/50 bg-zinc-900 shadow-[0_32px_80px_rgba(0,0,0,0.5)]">
        <div className="border-b border-zinc-800/60 px-4 py-3">
          <input
            autoFocus
            className="w-full bg-transparent text-base text-zinc-100 outline-none placeholder:text-zinc-500"
            onChange={(event) => setQuery(event.target.value)}
            placeholder={selectedAgentId ? 'Switch, spawn, or control agents' : 'Spawn or search commands'}
            value={query}
          />
        </div>
        <div className="max-h-[24rem] overflow-auto p-2">
          {filteredActions.map((action) => (
            <button
              className="flex w-full items-center justify-between rounded-lg px-3 py-2.5 text-left transition hover:bg-violet-500/10"
              key={action.id}
              onClick={() => {
                void Promise.resolve(action.run());
                setPaletteOpen(false);
                setQuery('');
              }}
              type="button"
            >
              <span className="text-sm text-zinc-200">{action.label}</span>
              <span className="text-[10px] text-zinc-600">↵</span>
            </button>
          ))}
          {!filteredActions.length ? (
            <div className="rounded-lg border border-dashed border-zinc-800 px-3 py-5 text-center text-xs text-zinc-600">
              No matching command.
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
};
