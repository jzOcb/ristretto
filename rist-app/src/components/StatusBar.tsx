import { useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';

import { useAgentStore } from '../stores/agent-store';

export const StatusBar = () => {
  const connection = useAgentStore((state) => state.connection);
  const agents = useAgentStore((state) => state.agents);

  const health = useMemo(() => {
    let healthy = 0;
    let warnings = 0;
    let issues = 0;
    for (const agent of agents) {
      if (agent.status === 'stuck' || agent.status === 'error') {
        issues++;
      } else if (agent.context_usage && agent.context_usage.percentage > 70) {
        warnings++;
      } else {
        healthy++;
      }
    }
    return { healthy, warnings, issues };
  }, [agents]);

  return (
    <header className="flex items-center justify-between border-b border-zinc-800/60 bg-zinc-900/80 px-4 py-1.5 backdrop-blur-sm">
      <div className="flex items-center gap-2.5">
        <div
          className={`h-2 w-2 rounded-full ${
            connection.connected
              ? 'bg-emerald-400 shadow-[0_0_6px_rgba(52,211,153,0.4)]'
              : 'bg-zinc-500'
          }`}
        />
        <span className="text-xs font-medium text-zinc-300">Ristretto</span>
        <span className="text-xs text-zinc-500">{connection.message}</span>
        <span className="rounded-md border border-zinc-800 bg-zinc-900 px-1.5 py-0.5 text-[10px] tabular-nums text-zinc-400">
          {agents.length} agents
        </span>
        {agents.length > 0 ? (
          <div className="flex items-center gap-2 ml-1">
            {health.healthy > 0 ? (
              <span className="flex items-center gap-1 rounded-md border border-emerald-500/20 bg-emerald-500/5 px-1.5 py-0.5 text-[10px] tabular-nums text-emerald-300">
                <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" />
                {health.healthy}
              </span>
            ) : null}
            {health.warnings > 0 ? (
              <span className="flex items-center gap-1 rounded-md border border-amber-500/20 bg-amber-500/5 px-1.5 py-0.5 text-[10px] tabular-nums text-amber-300">
                <span className="h-1.5 w-1.5 rounded-full bg-amber-400" />
                {health.warnings}
              </span>
            ) : null}
            {health.issues > 0 ? (
              <span className="flex items-center gap-1 rounded-md border border-rose-500/20 bg-rose-500/5 px-1.5 py-0.5 text-[10px] tabular-nums text-rose-300">
                <span className="h-1.5 w-1.5 rounded-full bg-rose-500" />
                {health.issues}
              </span>
            ) : null}
          </div>
        ) : null}
      </div>

      <div className="flex items-center gap-3">
        <div className="flex items-center gap-2.5 text-[10px] text-zinc-500">
          <span><kbd className="rounded border border-zinc-700 bg-zinc-800 px-1 py-0.5 font-mono text-zinc-400">⌘K</kbd> Search</span>
          <span><kbd className="rounded border border-zinc-700 bg-zinc-800 px-1 py-0.5 font-mono text-zinc-400">⌘T</kbd> Spawn</span>
          <span><kbd className="rounded border border-zinc-700 bg-zinc-800 px-1 py-0.5 font-mono text-zinc-400">⌘D</kbd> DAG</span>
          <span><kbd className="rounded border border-zinc-700 bg-zinc-800 px-1 py-0.5 font-mono text-zinc-400">⌘G</kbd> Grid</span>
          <span><kbd className="rounded border border-zinc-700 bg-zinc-800 px-1 py-0.5 font-mono text-zinc-400">⌘A</kbd> Feed</span>
        </div>
        {!connection.connected ? (
          <button
            className="rounded-md border border-violet-500/40 bg-violet-500/10 px-2.5 py-1 text-[11px] font-medium text-violet-200 transition hover:bg-violet-500/20"
            onClick={() => void invoke('start_daemon')}
            type="button"
          >
            Start Daemon
          </button>
        ) : null}
      </div>
    </header>
  );
};
