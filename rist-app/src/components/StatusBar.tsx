import { invoke } from '@tauri-apps/api/core';

import { useAgentStore } from '../stores/agent-store';

export const StatusBar = () => {
  const connection = useAgentStore((state) => state.connection);
  const agents = useAgentStore((state) => state.agents);

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
      </div>

      <div className="flex items-center gap-3">
        <div className="flex items-center gap-2.5 text-[10px] text-zinc-500">
          <span><kbd className="rounded border border-zinc-700 bg-zinc-800 px-1 py-0.5 font-mono text-zinc-400">⌘K</kbd> Search</span>
          <span><kbd className="rounded border border-zinc-700 bg-zinc-800 px-1 py-0.5 font-mono text-zinc-400">⌘T</kbd> Spawn</span>
          <span><kbd className="rounded border border-zinc-700 bg-zinc-800 px-1 py-0.5 font-mono text-zinc-400">⌘D</kbd> DAG</span>
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
