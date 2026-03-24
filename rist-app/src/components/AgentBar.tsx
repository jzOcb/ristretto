import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

import type { AgentInfo, AgentType } from '../lib/types';
import { agentTypeLabel, statusDot } from '../lib/types';
import { useAgentStore } from '../stores/agent-store';

const formatUptime = (createdAt: string): string => {
  const elapsed = Math.max(0, Math.floor((Date.now() - new Date(createdAt).getTime()) / 1000));
  if (elapsed < 60) return `${elapsed}s`;
  if (elapsed < 3600) return `${Math.floor(elapsed / 60)}m ${elapsed % 60}s`;
  const hours = Math.floor(elapsed / 3600);
  const mins = Math.floor((elapsed % 3600) / 60);
  return `${hours}h ${mins}m`;
};

const TASK_TEMPLATES = [
  { label: 'None', value: '' },
  { label: 'Fix bug', value: 'Fix bug: [describe]' },
  { label: 'Add feature', value: 'Add feature: [describe]' },
  { label: 'Refactor', value: 'Refactor: [describe]' },
  { label: 'Review PR', value: 'Review PR #[number]' },
  { label: 'Write tests', value: 'Write tests for: [describe]' },
];

const defaultAgentType: AgentType = { kind: 'codex' };

export const AgentBar = () => {
  const agents = useAgentStore((state) => state.agents);
  const selectedAgentId = useAgentStore((state) => state.selectedAgentId);
  const selectAgent = useAgentStore((state) => state.selectAgent);
  const setAgents = useAgentStore((state) => state.setAgents);
  const replaceOutput = useAgentStore((state) => state.replaceOutput);
  const setSpawnOpen = useAgentStore((state) => state.setSpawnOpen);
  const spawnOpen = useAgentStore((state) => state.spawnOpen);
  const setContextMenu = useAgentStore((state) => state.setContextMenu);

  const [agentType, setAgentType] = useState<AgentType>(defaultAgentType);
  const [task, setTask] = useState('');
  const [repoPath, setRepoPath] = useState('');
  const [fileOwnership, setFileOwnership] = useState('');
  const [customCommand, setCustomCommand] = useState('');
  const [busy, setBusy] = useState(false);
  const [, setTick] = useState(0);

  useEffect(() => {
    const id = setInterval(() => setTick((t) => t + 1), 1000);
    return () => clearInterval(id);
  }, []);

  useEffect(() => {
    if (!spawnOpen) {
      setTask('');
      setAgentType(defaultAgentType);
      setRepoPath('');
      setFileOwnership('');
      setCustomCommand('');
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

  const handleModelChange = (value: string) => {
    if (value === 'custom') {
      setAgentType({ kind: 'custom', value: customCommand || 'custom-agent' });
    } else {
      setAgentType({ kind: value as 'claude' | 'codex' | 'gemini' });
    }
  };

  const modelSelectValue = agentType.kind === 'custom' ? 'custom' : agentType.kind;

  return (
    <>
      {spawnOpen ? (
        <section className="border-t border-zinc-800/60 bg-zinc-900/95 px-4 py-3 backdrop-blur-sm">
          <div className="grid gap-3 lg:grid-cols-[1fr_2fr_auto]">
            {/* Left column: Model, Repo, Template */}
            <div className="space-y-3">
              <label className="space-y-1.5">
                <span className="text-[10px] font-medium text-zinc-500">Model</span>
                <select
                  className="w-full rounded-lg border border-zinc-800 bg-zinc-950 px-3 py-2.5 text-sm text-zinc-200 outline-none focus:border-violet-500/50"
                  onChange={(e) => handleModelChange(e.target.value)}
                  value={modelSelectValue}
                >
                  <option value="codex">Codex</option>
                  <option value="claude">Claude</option>
                  <option value="gemini">Gemini</option>
                  <option value="custom">Custom</option>
                </select>
              </label>
              {agentType.kind === 'custom' ? (
                <label className="space-y-1.5">
                  <span className="text-[10px] font-medium text-zinc-500">Custom Command</span>
                  <input
                    className="w-full rounded-lg border border-zinc-800 bg-zinc-950 px-3 py-2.5 text-sm text-zinc-200 outline-none placeholder:text-zinc-600 focus:border-violet-500/50"
                    onChange={(e) => {
                      setCustomCommand(e.target.value);
                      setAgentType({ kind: 'custom', value: e.target.value });
                    }}
                    placeholder="e.g. aider --model gpt-4o"
                    value={customCommand}
                  />
                </label>
              ) : null}
              <label className="space-y-1.5">
                <span className="text-[10px] font-medium text-zinc-500">Repository Path</span>
                <input
                  className="w-full rounded-lg border border-zinc-800 bg-zinc-950 px-3 py-2.5 font-mono text-sm text-zinc-200 outline-none placeholder:text-zinc-600 focus:border-violet-500/50"
                  onChange={(e) => setRepoPath(e.target.value)}
                  placeholder="./  (current project)"
                  value={repoPath}
                />
              </label>
              <label className="space-y-1.5">
                <span className="text-[10px] font-medium text-zinc-500">Template</span>
                <select
                  className="w-full rounded-lg border border-zinc-800 bg-zinc-950 px-3 py-2.5 text-sm text-zinc-200 outline-none focus:border-violet-500/50"
                  onChange={(e) => {
                    if (e.target.value) setTask(e.target.value);
                  }}
                  value=""
                >
                  {TASK_TEMPLATES.map((t) => (
                    <option key={t.label} value={t.value}>{t.label}</option>
                  ))}
                </select>
              </label>
            </div>

            {/* Middle column: Task + File Ownership */}
            <div className="space-y-3">
              <label className="space-y-1.5">
                <span className="text-[10px] font-medium text-zinc-500">Task</span>
                <textarea
                  className="min-h-[72px] w-full rounded-lg border border-zinc-800 bg-zinc-950 px-3 py-2.5 text-sm text-zinc-200 outline-none placeholder:text-zinc-600 focus:border-violet-500/50"
                  onChange={(e) => setTask(e.target.value)}
                  placeholder="Describe the agent task..."
                  value={task}
                />
              </label>
              <label className="space-y-1.5">
                <span className="text-[10px] font-medium text-zinc-500">File Ownership</span>
                <textarea
                  className="min-h-[52px] w-full rounded-lg border border-zinc-800 bg-zinc-950 px-3 py-2.5 font-mono text-xs text-zinc-200 outline-none placeholder:text-zinc-600 focus:border-violet-500/50"
                  onChange={(e) => setFileOwnership(e.target.value)}
                  placeholder={"src/lib/auth.ts\nsrc/routes/login.tsx\n(one path per line)"}
                  value={fileOwnership}
                />
              </label>
            </div>

            {/* Right column: Actions */}
            <div className="flex items-end gap-2">
              <button
                className="rounded-lg border border-zinc-700 px-3 py-2 text-xs text-zinc-400 transition hover:border-zinc-500 hover:text-zinc-300"
                onClick={() => setSpawnOpen(false)}
                type="button"
              >
                Cancel
              </button>
              <button
                className="rounded-lg bg-violet-600 px-4 py-2 text-xs font-medium text-white transition hover:bg-violet-500 disabled:opacity-40"
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
      <footer className="border-t border-zinc-800/60 bg-zinc-900/90 px-3 py-2">
        <div className="flex items-center gap-1.5 overflow-x-auto">
          {agents.map((agent) => (
            <button
              className={`relative flex shrink-0 items-center gap-2 rounded-lg px-3 py-1.5 text-left transition ${
                selectedAgentId === agent.id
                  ? 'bg-zinc-800 text-zinc-100'
                  : 'text-zinc-400 hover:bg-zinc-800/50 hover:text-zinc-300'
              }`}
              key={agent.id}
              onClick={() => selectAgent(agent.id)}
              onContextMenu={(e) => {
                e.preventDefault();
                setContextMenu({ agentId: agent.id, x: e.clientX, y: e.clientY });
              }}
              type="button"
            >
              {selectedAgentId === agent.id ? (
                <span className="absolute bottom-0 left-3 right-3 h-0.5 rounded-full bg-violet-500" />
              ) : null}
              <span className={`h-2 w-2 rounded-full ${statusDot(agent.status)}`} />
              <span className="max-w-[12rem] truncate text-xs">{agent.task}</span>
              <span className="rounded border border-zinc-700/60 px-1.5 py-0.5 text-[9px] uppercase tracking-wider text-zinc-500">
                {agentTypeLabel(agent.agent_type)}
              </span>
              <span className="font-mono text-[9px] tabular-nums text-zinc-500">
                {formatUptime(agent.created_at)}
              </span>
              {/* ⋯ button */}
              <span
                className="ml-1 rounded p-0.5 text-zinc-600 transition hover:bg-zinc-700 hover:text-zinc-400"
                onClick={(e) => {
                  e.stopPropagation();
                  const rect = (e.target as HTMLElement).getBoundingClientRect();
                  setContextMenu({ agentId: agent.id, x: rect.left, y: rect.bottom + 4 });
                }}
              >
                <svg className="h-3 w-3" fill="currentColor" viewBox="0 0 16 16">
                  <circle cx="3" cy="8" r="1.5" />
                  <circle cx="8" cy="8" r="1.5" />
                  <circle cx="13" cy="8" r="1.5" />
                </svg>
              </span>
            </button>
          ))}
          <button
            className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg text-zinc-500 transition hover:bg-zinc-800 hover:text-zinc-300"
            onClick={() => setSpawnOpen(true)}
            type="button"
          >
            <svg className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth={2} viewBox="0 0 24 24">
              <path d="M12 5v14m-7-7h14" strokeLinecap="round" />
            </svg>
          </button>
        </div>
      </footer>
    </>
  );
};
