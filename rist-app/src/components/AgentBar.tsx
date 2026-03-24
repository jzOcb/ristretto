import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

import type { AgentInfo, AgentType } from '../lib/types';
import { agentTypeLabel, statusDot } from '../lib/types';
import { useAgentStore } from '../stores/agent-store';

const defaultAgentType: AgentType = { kind: 'codex' };

export const AgentBar = () => {
  const agents = useAgentStore((state) => state.agents);
  const selectedAgentId = useAgentStore((state) => state.selectedAgentId);
  const selectAgent = useAgentStore((state) => state.selectAgent);
  const setAgents = useAgentStore((state) => state.setAgents);
  const replaceOutput = useAgentStore((state) => state.replaceOutput);
  const setSpawnOpen = useAgentStore((state) => state.setSpawnOpen);
  const spawnOpen = useAgentStore((state) => state.spawnOpen);

  const [agentType, setAgentType] = useState<AgentType>(defaultAgentType);
  const [task, setTask] = useState('');
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!spawnOpen) {
      setTask('');
      setAgentType(defaultAgentType);
    }
  }, [spawnOpen]);

  const refreshAgents = async (selectedId?: string) => {
    const nextAgents = await invoke<AgentInfo[]>('list_agents');
    setAgents(nextAgents);
    await Promise.all(
      nextAgents.map(async (agent) => {
        const buffer = await invoke<string>('get_agent_buffer', { agentId: agent.id });
        replaceOutput(agent.id, buffer);
      }),
    );
    if (selectedId) {
      selectAgent(selectedId);
    }
  };

  return (
    <>
      {spawnOpen ? (
        <section className="border-t border-zinc-800 bg-zinc-900/95 px-5 py-4">
          <div className="grid gap-4 lg:grid-cols-[180px_1fr_auto]">
            <label className="space-y-2">
              <span className="text-[10px] uppercase tracking-[0.2em] text-zinc-500">Model</span>
              <select
                className="w-full rounded-2xl border border-zinc-800 bg-zinc-950 px-3 py-3 text-sm text-zinc-200 outline-none"
                onChange={(event) => setAgentType({ kind: event.target.value as 'claude' | 'codex' | 'gemini' })}
                value={agentType.kind}
              >
                <option value="codex">Codex</option>
                <option value="claude">Claude</option>
                <option value="gemini">Gemini</option>
              </select>
            </label>
            <label className="space-y-2">
              <span className="text-[10px] uppercase tracking-[0.2em] text-zinc-500">Task</span>
              <textarea
                className="min-h-[92px] w-full rounded-2xl border border-zinc-800 bg-zinc-950 px-3 py-3 text-sm text-zinc-200 outline-none placeholder:text-zinc-500"
                onChange={(event) => setTask(event.target.value)}
                placeholder="Describe the agent task..."
                value={task}
              />
            </label>
            <div className="flex items-end gap-3">
              <button
                className="rounded-full border border-zinc-700 px-4 py-2 text-sm text-zinc-300 transition hover:border-zinc-500"
                onClick={() => setSpawnOpen(false)}
                type="button"
              >
                Cancel
              </button>
              <button
                className="rounded-full bg-zinc-100 px-4 py-2 text-sm font-medium text-zinc-950 transition hover:bg-white disabled:opacity-50"
                disabled={!task.trim() || busy}
                onClick={async () => {
                  setBusy(true);
                  try {
                    const id = await invoke<string>('spawn_agent', {
                      agentType,
                      task: task.trim(),
                    });
                    await refreshAgents(id);
                    setSpawnOpen(false);
                  } finally {
                    setBusy(false);
                  }
                }}
                type="button"
              >
                Spawn
              </button>
            </div>
          </div>
        </section>
      ) : null}
      <footer className="border-t border-zinc-800 bg-zinc-900 px-4 py-3">
        <div className="flex items-center gap-3 overflow-x-auto">
          {agents.map((agent) => (
            <button
              className={`flex shrink-0 items-center gap-3 rounded-full border px-4 py-2 text-left transition ${
                selectedAgentId === agent.id
                  ? 'border-zinc-500 bg-zinc-800 text-zinc-100'
                  : 'border-zinc-800 bg-zinc-950 text-zinc-400 hover:border-zinc-700'
              }`}
              key={agent.id}
              onClick={() => selectAgent(agent.id)}
              type="button"
            >
              <span className={`h-2.5 w-2.5 rounded-full ${statusDot(agent.status)}`} />
              <span className="max-w-[14rem] truncate text-sm">{agent.task}</span>
              <span className="rounded-full border border-zinc-700 px-2 py-0.5 text-[10px] uppercase tracking-[0.2em]">
                {agentTypeLabel(agent.agent_type)}
              </span>
            </button>
          ))}
          <button
            className="flex h-11 w-11 shrink-0 items-center justify-center rounded-full border border-dashed border-zinc-700 bg-zinc-950 text-xl text-zinc-300 transition hover:border-zinc-500"
            onClick={() => setSpawnOpen(true)}
            type="button"
          >
            +
          </button>
        </div>
      </footer>
    </>
  );
};
