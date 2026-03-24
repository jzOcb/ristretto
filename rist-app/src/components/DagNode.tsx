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
      className={`min-w-[220px] rounded-2xl border px-4 py-3 shadow-[0_18px_38px_rgba(0,0,0,0.28)] transition ${
        selected
          ? 'border-zinc-500 bg-zinc-800'
          : 'border-zinc-800 bg-zinc-900/95'
      }`}
    >
      <div className="flex items-center justify-between gap-2">
        <span className={`h-2.5 w-2.5 rounded-full ${statusDot(nodeData.status as never)}`} />
        <span className="rounded-full border border-zinc-700 px-2 py-0.5 text-[10px] uppercase tracking-[0.2em] text-zinc-400">
          {nodeData.agentLabel}
        </span>
      </div>
      <p className="mt-3 text-sm font-medium text-zinc-100">{nodeData.title}</p>
      <div className="mt-4">
        <div className="mb-1 flex items-center justify-between text-[10px] uppercase tracking-[0.2em] text-zinc-500">
          <span>Context</span>
          <span>{Math.round(nodeData.contextPct)}%</span>
        </div>
        <div className="h-1.5 overflow-hidden rounded-full bg-zinc-800">
          <div
            className="h-full rounded-full bg-gradient-to-r from-amber-400 via-orange-400 to-rose-500"
            style={{ width: `${Math.min(100, Math.max(6, nodeData.contextPct))}%` }}
          />
        </div>
      </div>
    </div>
  );
};
