import { useState } from 'react';

import type { CommandBlock } from '../lib/types';

const toneClass = (status: CommandBlock['status']) => {
  switch (status) {
    case 'success':
      return 'border-emerald-500/30 bg-emerald-500/10 text-emerald-300';
    case 'failure':
      return 'border-rose-500/30 bg-rose-500/10 text-rose-300';
    default:
      return 'border-violet-500/30 bg-violet-500/10 text-violet-300';
  }
};

export const CommandCard = ({ block }: { block: CommandBlock }) => {
  const [open, setOpen] = useState(block.status !== 'success');

  return (
    <article className="rounded-lg border border-zinc-800/60 bg-zinc-900/60">
      <button
        className="flex w-full items-center justify-between gap-3 px-3.5 py-2.5 text-left"
        onClick={() => setOpen((value) => !value)}
        type="button"
      >
        <div>
          <p className="font-mono text-xs text-zinc-100">{block.command}</p>
          <p className="mt-0.5 text-[11px] text-zinc-500">{block.summary}</p>
        </div>
        <span className={`rounded-md border px-2 py-0.5 text-[9px] uppercase tracking-wider ${toneClass(block.status)}`}>
          {block.status}
        </span>
      </button>
      <div
        className="grid overflow-hidden transition-[grid-template-rows] duration-200"
        style={{ gridTemplateRows: open ? '1fr' : '0fr' }}
      >
        <div className="min-h-0">
          <pre className="overflow-x-auto border-t border-zinc-800/40 px-3.5 py-2.5 font-mono text-xs leading-6 text-zinc-400">
            {block.content || 'Command started. Awaiting output...'}
          </pre>
        </div>
      </div>
    </article>
  );
};
