import { useEffect, useRef } from 'react';

import { activityDot } from '../lib/types';
import { useAgentStore } from '../stores/agent-store';

const formatTime = (timestamp: number): string => {
  const date = new Date(timestamp);
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
};

const timeAgo = (timestamp: number): string => {
  const seconds = Math.floor((Date.now() - timestamp) / 1000);
  if (seconds < 5) return 'just now';
  if (seconds < 60) return `${seconds}s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  return `${Math.floor(seconds / 3600)}h ago`;
};

export const ActivityFeed = () => {
  const activityLog = useAgentStore((state) => state.activityLog);
  const agents = useAgentStore((state) => state.agents);
  const toggleActivityFeed = useAgentStore((state) => state.toggleActivityFeed);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = scrollRef.current;
    if (el) {
      el.scrollTop = el.scrollHeight;
    }
  }, [activityLog.length]);

  const agentLabel = (agentId: string): string => {
    const agent = agents.find((a) => a.id === agentId);
    if (!agent) return agentId.slice(0, 8);
    return agent.task.length > 30 ? `${agent.task.slice(0, 30)}...` : agent.task;
  };

  return (
    <aside className="flex h-full w-64 flex-col border-l border-zinc-800/60 bg-zinc-900/80 backdrop-blur-sm">
      {/* Header */}
      <div className="flex items-center justify-between border-b border-zinc-800/60 px-3 py-2">
        <span className="text-[10px] font-semibold uppercase tracking-widest text-zinc-500">
          Activity
        </span>
        <button
          className="rounded p-0.5 text-zinc-500 transition hover:bg-zinc-800 hover:text-zinc-300"
          onClick={toggleActivityFeed}
          title="Close (⌘A)"
          type="button"
        >
          <svg className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth={2} viewBox="0 0 24 24">
            <path d="M6 18 18 6M6 6l12 12" strokeLinecap="round" />
          </svg>
        </button>
      </div>

      {/* Event list */}
      <div className="flex-1 overflow-y-auto" ref={scrollRef}>
        {activityLog.length === 0 ? (
          <div className="flex h-full items-center justify-center">
            <p className="text-xs text-zinc-600">No activity yet</p>
          </div>
        ) : (
          <div className="space-y-px py-1">
            {activityLog.map((event) => (
              <div
                className="group flex gap-2.5 px-3 py-1.5 transition hover:bg-zinc-800/40"
                key={event.id}
              >
                {/* Dot */}
                <div className="flex shrink-0 pt-1">
                  <span className={`h-2 w-2 rounded-full ${activityDot(event.type)}`} />
                </div>

                {/* Content */}
                <div className="min-w-0 flex-1">
                  <p className="truncate text-[11px] text-zinc-300">{event.message}</p>
                  <div className="mt-0.5 flex items-center gap-2 text-[9px] text-zinc-600">
                    <span className="truncate" title={event.agentId}>
                      {agentLabel(event.agentId)}
                    </span>
                    <span className="shrink-0" title={formatTime(event.timestamp)}>
                      {timeAgo(event.timestamp)}
                    </span>
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </aside>
  );
};
