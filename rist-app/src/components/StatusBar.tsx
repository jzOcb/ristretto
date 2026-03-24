import { invoke } from '@tauri-apps/api/core';

import { useAgentStore } from '../stores/agent-store';

export const StatusBar = () => {
  const connection = useAgentStore((state) => state.connection);
  const agents = useAgentStore((state) => state.agents);

  return (
    <header className="border-b border-zinc-800 bg-zinc-900/90 px-5 py-3 backdrop-blur">
      <div className="flex items-center justify-between gap-4">
        <div className="flex items-center gap-3">
          <div className="h-2.5 w-2.5 rounded-full bg-zinc-500 shadow-[0_0_0_4px_rgba(39,39,42,0.55)]" />
          <div>
            <p className="font-display text-sm uppercase tracking-[0.24em] text-zinc-400">
              Ristretto Ops Center
            </p>
            <p className="text-sm text-zinc-300">{connection.message}</p>
          </div>
        </div>

        <div className="flex items-center gap-3">
          <div className="rounded-full border border-zinc-800 bg-zinc-950/80 px-3 py-1 text-xs text-zinc-400">
            {agents.length} active agents
          </div>
          {!connection.connected ? (
            <button
              className="rounded-full border border-amber-500/40 bg-amber-500/10 px-3 py-1.5 text-xs font-medium text-amber-200 transition hover:bg-amber-500/20"
              onClick={() => void invoke('start_daemon')}
              type="button"
            >
              Start Daemon
            </button>
          ) : null}
        </div>
      </div>
    </header>
  );
};
