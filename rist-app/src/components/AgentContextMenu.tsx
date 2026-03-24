import { useEffect, useRef } from 'react';

import { useAgentStore } from '../stores/agent-store';

interface MenuItem {
  label: string;
  icon: string;
  action: () => void;
  danger?: boolean;
}

export const AgentContextMenu = () => {
  const contextMenu = useAgentStore((s) => s.contextMenu);
  const setContextMenu = useAgentStore((s) => s.setContextMenu);
  const setShowMergePanel = useAgentStore((s) => s.setShowMergePanel);
  const selectAgent = useAgentStore((s) => s.selectAgent);
  const setRawMode = useAgentStore((s) => s.setRawMode);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!contextMenu) return;
    const close = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setContextMenu(null);
      }
    };
    const closeKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setContextMenu(null);
    };
    window.addEventListener('mousedown', close);
    window.addEventListener('keydown', closeKey);
    return () => {
      window.removeEventListener('mousedown', close);
      window.removeEventListener('keydown', closeKey);
    };
  }, [contextMenu, setContextMenu]);

  if (!contextMenu) return null;

  const { agentId, x, y } = contextMenu;

  const items: MenuItem[] = [
    {
      label: 'View output',
      icon: '⌘',
      action: () => {
        selectAgent(agentId);
        setRawMode(false);
        setContextMenu(null);
      },
    },
    {
      label: 'Send to raw terminal',
      icon: '⌥',
      action: () => {
        selectAgent(agentId);
        setRawMode(true);
        setContextMenu(null);
      },
    },
    {
      label: 'Merge worktree',
      icon: '⇧',
      action: () => {
        setShowMergePanel(true);
        setContextMenu(null);
      },
    },
    {
      label: 'Copy agent ID',
      icon: '⌘C',
      action: () => {
        navigator.clipboard.writeText(agentId);
        setContextMenu(null);
      },
    },
    {
      label: 'Archive agent',
      icon: '⌫',
      action: () => {
        setContextMenu(null);
      },
    },
    {
      label: 'Kill agent',
      icon: '⌘W',
      danger: true,
      action: () => {
        setContextMenu(null);
      },
    },
  ];

  // Clamp to viewport
  const menuW = 200;
  const menuH = items.length * 36 + 8;
  const clampedX = Math.min(x, window.innerWidth - menuW - 8);
  const clampedY = Math.min(y, window.innerHeight - menuH - 8);

  return (
    <div className="fixed inset-0 z-50" onContextMenu={(e) => e.preventDefault()}>
      <div
        className="absolute rounded-xl border border-zinc-800/80 bg-zinc-900/98 py-1 shadow-[0_16px_48px_rgba(0,0,0,0.5)] backdrop-blur-xl"
        ref={ref}
        style={{ left: clampedX, top: clampedY, minWidth: menuW }}
      >
        {items.map((item, i) => (
          <div key={item.label}>
            {i === items.length - 1 ? (
              <div className="mx-2 my-1 border-t border-zinc-800/60" />
            ) : null}
            <button
              className={`flex w-full items-center justify-between px-3 py-2 text-left text-xs transition ${
                item.danger
                  ? 'text-rose-400 hover:bg-rose-500/10'
                  : 'text-zinc-300 hover:bg-zinc-800'
              }`}
              onClick={item.action}
              type="button"
            >
              <span>{item.label}</span>
              <span className="font-mono text-[10px] text-zinc-600">{item.icon}</span>
            </button>
          </div>
        ))}
      </div>
    </div>
  );
};
