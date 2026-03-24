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
  const pushActivity = useAgentStore((state) => state.pushActivity);

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
        for (const agent of agents) {
          pushActivity({ agentId: agent.id, type: 'spawn', message: `Agent spawned: ${agent.task.slice(0, 60)}` });
        }
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
        const { agent_id, new_status, old_status } = event.payload;
        if (new_status === 'done') {
          pushActivity({ agentId: agent_id, type: 'done', message: `Agent completed` });
        } else if (new_status === 'error') {
          pushActivity({ agentId: agent_id, type: 'error', message: `Agent errored` });
        } else if (new_status !== old_status) {
          pushActivity({ agentId: agent_id, type: 'status', message: `${old_status} → ${new_status}` });
        }
      }),
      listen<TaskUpdateEvent>('task-update', (event) => {
        updateTaskStatus(event.payload.task_id, event.payload.status);
      }),
      listen<ContextWarningEvent>('context-warning', (event) => {
        appendOutput(
          event.payload.agent_id,
          `\n[context-warning] Usage reached ${event.payload.usage_pct.toFixed(1)}%\n`,
        );
        pushActivity({
          agentId: event.payload.agent_id,
          type: 'warning',
          message: `Context usage ${event.payload.usage_pct.toFixed(1)}%`,
        });
      }),
      listen<LoopDetectedEvent>('loop-detected', (event) => {
        appendOutput(event.payload.agent_id, `\n[loop-detected] ${event.payload.pattern}\n`);
        pushActivity({
          agentId: event.payload.agent_id,
          type: 'loop',
          message: `Loop detected: ${event.payload.pattern}`,
        });
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
  }, [appendOutput, pushActivity, replaceOutput, setAgents, setConnection, setTasks, updateAgentStatus, updateTaskStatus]);
};
