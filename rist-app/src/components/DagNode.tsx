import type { NodeProps } from '@xyflow/react';

import { statusDot } from '../lib/types';

interface DagNodeData {
  title: string;
  status: string;
  agentLabel: string;
  contextPct: number;
}

export const DagNode = ({ data, selected }: NodeProps) => {
  const nodeData = data as unknown as DagNodeData;

  return (
    <div
      className={`min-w-[210px] rounded-lg border px-3.5 py-2.5 shadow-[0_12px_28px_rgba(0,0,0,0.2)] transition ${
        selected
          ? 'border-violet-500/50 bg-zinc-800'
          : 'border-zinc-800/60 bg-zinc-900/95'
      }`}
    >
      <div className="flex items-center justify-between gap-2">
        <span className={`h-2 w-2 rounded-full ${statusDot(nodeData.status as never)}`} />
        <span className="rounded border border-zinc-700/60 px-1.5 py-0.5 text-[9px] uppercase tracking-wider text-zinc-500">
          {nodeData.agentLabel}
        </span>
      </div>
      <p className="mt-2 text-sm font-medium text-zinc-100">{nodeData.title}</p>
      <div className="mt-3">
        <div className="mb-1 flex items-center justify-between text-[9px] uppercase tracking-wider text-zinc-500">
          <span>Context</span>
          <span>{Math.round(nodeData.contextPct)}%</span>
        </div>
        <div className="h-1 overflow-hidden rounded-full bg-zinc-800">
          <div
            className="h-full rounded-full bg-gradient-to-r from-violet-500 via-indigo-400 to-violet-300"
            style={{ width: `${Math.min(100, Math.max(6, nodeData.contextPct))}%` }}
          />
        </div>
      </div>
    </div>
  );
};
