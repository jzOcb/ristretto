import { useEffect, useState } from 'react';

import { agentTypeLabel, statusDot, statusTone } from '../lib/types';
import { useAgentStore } from '../stores/agent-store';

const formatUptime = (createdAt: string): string => {
  const elapsed = Math.max(0, Math.floor((Date.now() - new Date(createdAt).getTime()) / 1000));
  if (elapsed < 60) return `${elapsed}s`;
  if (elapsed < 3600) return `${Math.floor(elapsed / 60)}m ${elapsed % 60}s`;
  const hours = Math.floor(elapsed / 3600);
  const mins = Math.floor((elapsed % 3600) / 60);
  return `${hours}h ${mins}m`;
};

export const AgentCards = () => {
  const agents = useAgentStore((state) => state.agents);
  const selectAgent = useAgentStore((state) => state.selectAgent);
  const toggleViewMode = useAgentStore((state) => state.toggleViewMode);
  const [, setTick] = useState(0);

  useEffect(() => {
    const id = setInterval(() => setTick((t) => t + 1), 1000);
    return () => clearInterval(id);
  }, []);

  if (agents.length === 0) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="text-center">
          <p className="text-sm text-zinc-400">No agents running</p>
          <p className="mt-1 text-xs text-zinc-600">
            Press <kbd className="rounded border border-zinc-700 bg-zinc-800 px-1 py-0.5 font-mono text-zinc-400">⌘T</kbd> to spawn
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto p-4">
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
        {agents.map((agent) => {
          const contextPct = agent.context_usage?.percentage ?? 0;
          return (
            <button
              className="group relative rounded-lg border border-zinc-800/60 bg-zinc-900/60 p-4 text-left backdrop-blur-sm transition-all duration-200 hover:scale-[1.02] hover:border-violet-500/30 hover:shadow-[0_0_20px_rgba(139,92,246,0.08)]"
              key={agent.id}
              onClick={() => {
                selectAgent(agent.id);
                toggleViewMode();
              }}
              type="button"
            >
              {/* Header: name + status */}
              <div className="flex items-start justify-between gap-2">
                <p className="max-w-[80%] truncate text-sm font-medium text-zinc-100">
                  {agent.task}
                </p>
                <div className="flex shrink-0 items-center gap-1.5">
                  <span className={`h-2 w-2 rounded-full ${statusDot(agent.status)}`} />
                  <span className={`text-[10px] capitalize ${statusTone(agent.status)}`}>
                    {agent.status}
                  </span>
                </div>
              </div>

              {/* Model badge + uptime */}
              <div className="mt-2.5 flex items-center gap-2">
                <span className="rounded border border-violet-500/20 bg-violet-500/10 px-1.5 py-0.5 text-[9px] uppercase tracking-wider text-violet-300">
                  {agentTypeLabel(agent.agent_type)}
                </span>
                <span className="font-mono text-[10px] tabular-nums text-zinc-500">
                  {formatUptime(agent.created_at)}
                </span>
              </div>

              {/* Context usage bar */}
              <div className="mt-3">
                <div className="flex items-center justify-between text-[10px]">
                  <span className="text-zinc-500">Context</span>
                  <span className="font-mono tabular-nums text-zinc-400">{contextPct.toFixed(0)}%</span>
                </div>
                <div className="mt-1 h-1.5 w-full overflow-hidden rounded-full bg-zinc-800">
                  <div
                    className="h-full rounded-full bg-gradient-to-r from-violet-500 via-indigo-500 to-violet-500 transition-all duration-500"
                    style={{ width: `${Math.min(100, contextPct)}%` }}
                  />
                </div>
              </div>

              {/* Metadata row */}
              <div className="mt-3 flex items-center gap-3 text-[10px] text-zinc-500">
                <span className="flex items-center gap-1">
                  <svg className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth={1.5} viewBox="0 0 24 24">
                    <path d="M19.5 14.25v-2.625a3.375 3.375 0 0 0-3.375-3.375h-1.5A1.125 1.125 0 0 1 13.5 7.125v-1.5a3.375 3.375 0 0 0-3.375-3.375H8.25m2.25 0H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 0 0-9-9Z" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                  {agent.file_ownership.length} files
                </span>
                <span className="max-w-[60%] truncate" title={agent.workdir}>
                  {agent.workdir.split('/').slice(-2).join('/')}
                </span>
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
};
