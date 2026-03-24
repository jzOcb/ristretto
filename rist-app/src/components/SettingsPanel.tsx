import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

import { useAgentStore } from '../stores/agent-store';

interface DaemonStatus {
  connected: boolean;
  version: string | null;
}

const shortcuts = [
  { keys: '⌘ K', action: 'Command palette' },
  { keys: '⌘ T', action: 'Spawn agent' },
  { keys: '⌘ W', action: 'Kill selected agent' },
  { keys: '⌘ R', action: 'Toggle raw/structured' },
  { keys: '⌘ D', action: 'Toggle DAG panel' },
  { keys: '⌘ G', action: 'Toggle grid/stream' },
  { keys: '⌘ A', action: 'Toggle activity feed' },
  { keys: '⌘ ,', action: 'Settings' },
  { keys: '⌘ [ / ]', action: 'Previous / next agent' },
  { keys: '⌘ 1-9', action: 'Jump to agent' },
  { keys: 'Esc', action: 'Close overlay' },
];

export function SettingsPanel() {
  const showSettings = useAgentStore((state) => state.showSettings);
  const toggleSettings = useAgentStore((state) => state.toggleSettings);
  const connection = useAgentStore((state) => state.connection);

  const [daemon, setDaemon] = useState<DaemonStatus>({ connected: false, version: null });
  const [darkMode, setDarkMode] = useState(true);

  useEffect(() => {
    if (!showSettings) return;
    invoke<string>('ping')
      .then((version) => setDaemon({ connected: true, version }))
      .catch(() => setDaemon({ connected: false, version: null }));
  }, [showSettings]);

  const onBackdropClick = useCallback(
    (event: React.MouseEvent) => {
      if (event.target === event.currentTarget) toggleSettings();
    },
    [toggleSettings],
  );

  if (!showSettings) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={onBackdropClick}
    >
      <div className="w-full max-w-lg rounded-2xl border border-zinc-800 bg-zinc-950/95 shadow-[0_24px_80px_rgba(0,0,0,0.5)]">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-zinc-800 px-6 py-4">
          <h2 className="text-lg font-semibold text-zinc-100">Settings</h2>
          <button
            onClick={toggleSettings}
            className="rounded-lg p-1.5 text-zinc-400 transition hover:bg-zinc-800 hover:text-zinc-200"
            type="button"
          >
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
              <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
            </svg>
          </button>
        </div>

        <div className="max-h-[70vh] space-y-5 overflow-y-auto px-6 py-5">
          {/* Daemon Status */}
          <section>
            <h3 className="mb-3 text-xs font-medium uppercase tracking-wider text-zinc-500">
              Daemon Status
            </h3>
            <div className="space-y-2 rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
              <div className="flex items-center justify-between">
                <span className="text-sm text-zinc-400">Connection</span>
                <span className="flex items-center gap-2 text-sm">
                  <span
                    className={`inline-block h-2 w-2 rounded-full ${
                      connection.connected ? 'bg-emerald-400' : 'bg-red-400'
                    }`}
                  />
                  <span className={connection.connected ? 'text-emerald-400' : 'text-red-400'}>
                    {connection.connected ? 'Connected' : 'Disconnected'}
                  </span>
                </span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-sm text-zinc-400">Version</span>
                <span className="font-mono text-sm text-zinc-300">
                  {daemon.version ?? '—'}
                </span>
              </div>
            </div>
          </section>

          {/* Theme */}
          <section>
            <h3 className="mb-3 text-xs font-medium uppercase tracking-wider text-zinc-500">
              Theme
            </h3>
            <div className="flex gap-2">
              <button
                onClick={() => setDarkMode(true)}
                className={`flex-1 rounded-lg border px-4 py-2.5 text-sm font-medium transition ${
                  darkMode
                    ? 'border-violet-500/50 bg-violet-500/10 text-violet-300'
                    : 'border-zinc-800 bg-zinc-900/50 text-zinc-400 hover:border-zinc-700'
                }`}
                type="button"
              >
                Dark
              </button>
              <button
                onClick={() => setDarkMode(false)}
                className={`flex-1 rounded-lg border px-4 py-2.5 text-sm font-medium transition ${
                  !darkMode
                    ? 'border-violet-500/50 bg-violet-500/10 text-violet-300'
                    : 'border-zinc-800 bg-zinc-900/50 text-zinc-400 hover:border-zinc-700'
                }`}
                type="button"
              >
                Light
              </button>
            </div>
          </section>

          {/* Keyboard Shortcuts */}
          <section>
            <h3 className="mb-3 text-xs font-medium uppercase tracking-wider text-zinc-500">
              Keyboard Shortcuts
            </h3>
            <div className="rounded-lg border border-zinc-800 bg-zinc-900/50">
              {shortcuts.map((shortcut, index) => (
                <div
                  key={shortcut.keys}
                  className={`flex items-center justify-between px-4 py-2 ${
                    index !== shortcuts.length - 1 ? 'border-b border-zinc-800/50' : ''
                  }`}
                >
                  <span className="text-sm text-zinc-400">{shortcut.action}</span>
                  <kbd className="rounded-md border border-zinc-700 bg-zinc-800 px-2 py-0.5 font-mono text-xs text-zinc-300">
                    {shortcut.keys}
                  </kbd>
                </div>
              ))}
            </div>
          </section>

          {/* About */}
          <section>
            <h3 className="mb-3 text-xs font-medium uppercase tracking-wider text-zinc-500">
              About
            </h3>
            <div className="space-y-2 rounded-lg border border-zinc-800 bg-zinc-900/50 p-4">
              <div className="flex items-center justify-between">
                <span className="text-sm text-zinc-400">Ristretto</span>
                <span className="font-mono text-sm text-zinc-300">v0.3.0</span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-sm text-zinc-400">GitHub</span>
                <a
                  href="https://github.com/ristretto-dev/ristretto"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-sm text-violet-400 transition hover:text-violet-300"
                >
                  ristretto-dev/ristretto
                </a>
              </div>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
