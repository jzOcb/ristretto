import { create } from 'zustand';

import type {
  AgentInfo,
  AgentStatus,
  ConnectionState,
  Task,
  TaskStatus,
} from '../lib/types';

interface AgentStoreState {
  agents: AgentInfo[];
  tasks: Task[];
  outputs: Record<string, string>;
  selectedAgentId: string | null;
  connection: ConnectionState;
  rawMode: boolean;
  showDag: boolean;
  paletteOpen: boolean;
  spawnOpen: boolean;
  commandInput: string;
  setAgents: (agents: AgentInfo[]) => void;
  upsertAgent: (agent: AgentInfo) => void;
  setTasks: (tasks: Task[]) => void;
  updateTaskStatus: (taskId: string, status: TaskStatus) => void;
  updateAgentStatus: (agentId: string, status: AgentStatus, exitCode?: number | null) => void;
  appendOutput: (agentId: string, chunk: string) => void;
  replaceOutput: (agentId: string, output: string) => void;
  selectAgent: (agentId: string | null) => void;
  setConnection: (connection: ConnectionState) => void;
  toggleRawMode: () => void;
  setRawMode: (value: boolean) => void;
  toggleDag: () => void;
  setPaletteOpen: (value: boolean) => void;
  setSpawnOpen: (value: boolean) => void;
  setCommandInput: (value: string) => void;
}

export const useAgentStore = create<AgentStoreState>((set) => ({
  agents: [],
  tasks: [],
  outputs: {},
  selectedAgentId: null,
  connection: {
    connected: false,
    message: 'Connecting to daemon...',
  },
  rawMode: false,
  showDag: true,
  paletteOpen: false,
  spawnOpen: false,
  commandInput: '',
  setAgents: (agents) =>
    set((state) => ({
      agents,
      selectedAgentId:
        state.selectedAgentId && agents.some((agent) => agent.id === state.selectedAgentId)
          ? state.selectedAgentId
          : agents[0]?.id ?? null,
    })),
  upsertAgent: (agent) =>
    set((state) => {
      const nextAgents = state.agents.some((item) => item.id === agent.id)
        ? state.agents.map((item) => (item.id === agent.id ? { ...item, ...agent } : item))
        : [agent, ...state.agents];

      return {
        agents: nextAgents,
        selectedAgentId: state.selectedAgentId ?? agent.id,
      };
    }),
  setTasks: (tasks) => set({ tasks }),
  updateTaskStatus: (taskId, status) =>
    set((state) => ({
      tasks: state.tasks.map((task) => (task.id === taskId ? { ...task, status } : task)),
    })),
  updateAgentStatus: (agentId, status, exitCode) =>
    set((state) => ({
      agents: state.agents.map((agent) =>
        agent.id === agentId
          ? {
              ...agent,
              status,
              exit_code: exitCode ?? agent.exit_code,
            }
          : agent,
      ),
    })),
  appendOutput: (agentId, chunk) =>
    set((state) => ({
      outputs: {
        ...state.outputs,
        [agentId]: `${state.outputs[agentId] ?? ''}${chunk}`,
      },
    })),
  replaceOutput: (agentId, output) =>
    set((state) => ({
      outputs: {
        ...state.outputs,
        [agentId]: output,
      },
    })),
  selectAgent: (selectedAgentId) => set({ selectedAgentId }),
  setConnection: (connection) => set({ connection }),
  toggleRawMode: () => set((state) => ({ rawMode: !state.rawMode })),
  setRawMode: (rawMode) => set({ rawMode }),
  toggleDag: () => set((state) => ({ showDag: !state.showDag })),
  setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
  setSpawnOpen: (spawnOpen) => set({ spawnOpen }),
  setCommandInput: (commandInput) => set({ commandInput }),
}));
