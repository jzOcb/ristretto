import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

import type { AgentInfo } from '../lib/types';
import { useAgentStore } from '../stores/agent-store';

const isMacCommand = (event: KeyboardEvent) => event.metaKey && !event.ctrlKey;

export const useKeyboard = () => {
  const agents = useAgentStore((state) => state.agents);
  const selectedAgentId = useAgentStore((state) => state.selectedAgentId);
  const selectAgent = useAgentStore((state) => state.selectAgent);
  const toggleRawMode = useAgentStore((state) => state.toggleRawMode);
  const toggleDag = useAgentStore((state) => state.toggleDag);
  const toggleViewMode = useAgentStore((state) => state.toggleViewMode);
  const toggleActivityFeed = useAgentStore((state) => state.toggleActivityFeed);
  const toggleSettings = useAgentStore((state) => state.toggleSettings);
  const setPaletteOpen = useAgentStore((state) => state.setPaletteOpen);
  const setSpawnOpen = useAgentStore((state) => state.setSpawnOpen);
  const toggleMergePanel = useAgentStore((state) => state.toggleMergePanel);
  const setAgents = useAgentStore((state) => state.setAgents);
  const paletteOpen = useAgentStore((state) => state.paletteOpen);
  const showSettings = useAgentStore((state) => state.showSettings);
  const rawMode = useAgentStore((state) => state.rawMode);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!isMacCommand(event)) {
        if (event.key === 'Escape') {
          setPaletteOpen(false);
          if (showSettings) toggleSettings();
          return;
        }
        return;
      }

      const index = agents.findIndex((agent) => agent.id === selectedAgentId);
      const switchTo = (nextIndex: number) => {
        const clamped = agents.at(nextIndex);
        if (clamped) {
          selectAgent(clamped.id);
        }
      };

      switch (event.key.toLowerCase()) {
        case 'k':
          event.preventDefault();
          setPaletteOpen(!paletteOpen);
          break;
        case 't':
          event.preventDefault();
          setSpawnOpen(true);
          break;
        case 'w':
          event.preventDefault();
          if (selectedAgentId) {
            void invoke('kill_agent', { agentId: selectedAgentId }).then(async () => {
              const nextAgents = await invoke<AgentInfo[]>('list_agents');
              setAgents(nextAgents);
            });
          }
          break;
        case 'r':
          event.preventDefault();
          toggleRawMode();
          break;
        case 'd':
          event.preventDefault();
          toggleDag();
          break;
        case 'g':
          event.preventDefault();
          toggleViewMode();
          break;
        case 'a':
          event.preventDefault();
          toggleActivityFeed();
          break;
        case ',':
          event.preventDefault();
          toggleSettings();
          break;
        case 'm':
          event.preventDefault();
          toggleMergePanel();
          break;
        case '[':
          event.preventDefault();
          switchTo(Math.max(0, index - 1));
          break;
        case ']':
          event.preventDefault();
          switchTo(Math.min(agents.length - 1, index + 1));
          break;
        default:
          if (/^[1-9]$/.test(event.key)) {
            event.preventDefault();
            switchTo(Number(event.key) - 1);
          } else if (event.key === 'Escape' && rawMode) {
            event.preventDefault();
          }
      }
    };

    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [
    agents,
    paletteOpen,
    rawMode,
    selectAgent,
    selectedAgentId,
    setPaletteOpen,
    setAgents,
    setSpawnOpen,
    toggleMergePanel,
    toggleActivityFeed,
    toggleDag,
    toggleRawMode,
    toggleSettings,
    toggleViewMode,
  ]);
};
