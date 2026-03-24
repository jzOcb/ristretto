import { useMemo } from 'react';
import dagre from 'dagre';
import {
  Background,
  Controls,
  MarkerType,
  ReactFlow,
  type Edge,
  type Node,
  type NodeTypes,
} from '@xyflow/react';

import '@xyflow/react/dist/style.css';

import { useAgentStore } from '../stores/agent-store';
import { agentTypeLabel } from '../lib/types';
import { DagNode } from './DagNode';

const nodeTypes: NodeTypes = {
  dagNode: DagNode,
};

const dagreGraph = new dagre.graphlib.Graph();
dagreGraph.setDefaultEdgeLabel(() => ({}));

const layout = (nodes: Node[], edges: Edge[]) => {
  dagreGraph.setGraph({ rankdir: 'LR', ranksep: 48, nodesep: 28 });

  for (const node of nodes) {
    dagreGraph.setNode(node.id, { width: 248, height: 130 });
  }

  for (const edge of edges) {
    dagreGraph.setEdge(edge.source, edge.target);
  }

  dagre.layout(dagreGraph);

  return nodes.map((node) => {
    const position = dagreGraph.node(node.id);
    return {
      ...node,
      position: {
        x: position.x - 124,
        y: position.y - 65,
      },
    };
  });
};

export const DagPanel = () => {
  const tasks = useAgentStore((state) => state.tasks);
  const agents = useAgentStore((state) => state.agents);
  const selectAgent = useAgentStore((state) => state.selectAgent);

  const { nodes, edges } = useMemo(() => {
    const nodes: Node[] = tasks.map((task) => {
      const owner = agents.find((agent) => agent.id === task.owner);
      const contextPct = owner?.context_usage?.percentage ?? 8;
      const label = agentTypeLabel(task.agent_type ?? owner?.agent_type ?? { kind: 'unknown' });

      return {
        id: task.id,
        type: 'dagNode',
        data: {
          title: task.title,
          status: task.status,
          agentLabel: label,
          contextPct,
          ownerId: task.owner,
        },
        position: { x: 0, y: 0 },
      };
    });

    const edges: Edge[] = tasks.flatMap((task) =>
      task.depends_on.map((dependency) => ({
        id: `${dependency}-${task.id}`,
        source: dependency,
        target: task.id,
        markerEnd: {
          type: MarkerType.ArrowClosed,
          width: 16,
          height: 16,
          color: '#52525b',
        },
        style: {
          stroke: '#52525b',
          strokeWidth: 1.5,
        },
        animated: task.status === 'working',
      })),
    );

    return { nodes: layout(nodes, edges), edges };
  }, [agents, tasks]);

  return (
    <section className="panel-shell min-h-[420px] overflow-hidden">
      <div className="panel-header">
        <div>
          <p className="panel-eyebrow">Task Graph</p>
          <h2 className="panel-title">Live DAG</h2>
        </div>
      </div>
      <div className="h-[calc(100%-4.5rem)] min-h-[360px]">
        <ReactFlow
          fitView
          edges={edges}
          nodes={nodes}
          nodeTypes={nodeTypes}
          onNodeClick={(_, node) => {
            const ownerId = (node.data as { ownerId?: string }).ownerId;
            if (ownerId) {
              selectAgent(ownerId);
            }
          }}
          proOptions={{ hideAttribution: true }}
        >
          <Background color="#27272a" gap={20} size={1} />
          <Controls
            className="!rounded-2xl !border !border-zinc-800 !bg-zinc-900/90 !text-zinc-200"
            showInteractive={false}
          />
        </ReactFlow>
      </div>
    </section>
  );
};
