import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import type {
  AgentInfo,
  AgentOutputEvent,
  AgentStatusEvent,
  ConnectionState,
  ContextWarningEvent,
  LoopDetectedEvent,
  Task,
  TaskUpdateEvent,
} from '../lib/types';
import { useAgentStore } from '../stores/agent-store';

export const useDaemon = () => {
  const setAgents = useAgentStore((state) => state.setAgents);
  const setTasks = useAgentStore((state) => state.setTasks);
  const replaceOutput = useAgentStore((state) => state.replaceOutput);
  const appendOutput = useAgentStore((state) => state.appendOutput);
  const updateTaskStatus = useAgentStore((state) => state.updateTaskStatus);
  const updateAgentStatus = useAgentStore((state) => state.updateAgentStatus);
  const setConnection = useAgentStore((state) => state.setConnection);

  useEffect(() => {
    let mounted = true;

    const hydrate = async () => {
      try {
        const [agents, tasks] = await Promise.all([
          invoke<AgentInfo[]>('list_agents'),
          invoke<Task[]>('get_task_graph'),
        ]);

        if (!mounted) {
          return;
        }

        setAgents(agents);
        setTasks(tasks);
        setConnection({
          connected: true,
          message: `Connected to ristd · ${agents.length} agents`,
        });

        await Promise.all(
          agents.map(async (agent) => {
            const buffer = await invoke<string>('get_agent_buffer', { agentId: agent.id });
            if (mounted) {
              replaceOutput(agent.id, buffer);
            }
          }),
        );
      } catch (error) {
        if (mounted) {
          setConnection({
            connected: false,
            message: String(error),
          });
        }
      }
    };

    const subscriptions = Promise.all([
      listen<ConnectionState>('daemon-connection', (event) => {
        setConnection(event.payload);
      }),
      listen<AgentOutputEvent>('agent-output', (event) => {
        appendOutput(event.payload.agent_id, event.payload.data);
      }),
      listen<AgentStatusEvent>('agent-status', (event) => {
        updateAgentStatus(event.payload.agent_id, event.payload.new_status, event.payload.exit_code);
      }),
      listen<TaskUpdateEvent>('task-update', (event) => {
        updateTaskStatus(event.payload.task_id, event.payload.status);
      }),
      listen<ContextWarningEvent>('context-warning', (event) => {
        appendOutput(
          event.payload.agent_id,
          `\n[context-warning] Usage reached ${event.payload.usage_pct.toFixed(1)}%\n`,
        );
      }),
      listen<LoopDetectedEvent>('loop-detected', (event) => {
        appendOutput(event.payload.agent_id, `\n[loop-detected] ${event.payload.pattern}\n`);
      }),
    ]);

    void hydrate();

    return () => {
      mounted = false;
      void subscriptions.then((unlisteners) => {
        for (const unlisten of unlisteners) {
          unlisten();
        }
      });
    };
  }, [appendOutput, replaceOutput, setAgents, setConnection, setTasks, updateAgentStatus, updateTaskStatus]);
};
