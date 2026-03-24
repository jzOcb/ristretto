import { useState } from 'react';

import type { CommandBlock } from '../lib/types';

const toneClass = (status: CommandBlock['status']) => {
  switch (status) {
    case 'success':
      return 'border-emerald-500/30 bg-emerald-500/10 text-emerald-200';
    case 'failure':
      return 'border-rose-500/30 bg-rose-500/10 text-rose-200';
    default:
      return 'border-sky-500/30 bg-sky-500/10 text-sky-200';
  }
};

export const CommandCard = ({ block }: { block: CommandBlock }) => {
  const [open, setOpen] = useState(block.status !== 'success');

  return (
    <article className="rounded-2xl border border-zinc-800 bg-zinc-900/75">
      <button
        className="flex w-full items-center justify-between gap-3 px-4 py-3 text-left"
        onClick={() => setOpen((value) => !value)}
        type="button"
      >
        <div>
          <p className="font-mono text-sm text-zinc-100">{block.command}</p>
          <p className="mt-1 text-xs text-zinc-400">{block.summary}</p>
        </div>
        <span className={`rounded-full border px-2.5 py-1 text-[10px] uppercase tracking-[0.2em] ${toneClass(block.status)}`}>
          {block.status}
        </span>
      </button>
      <div
        className="grid overflow-hidden transition-[grid-template-rows] duration-200"
        style={{ gridTemplateRows: open ? '1fr' : '0fr' }}
      >
        <div className="min-h-0">
          <pre className="overflow-x-auto border-t border-zinc-800 px-4 py-3 font-mono text-xs leading-6 text-zinc-300">
            {block.content || 'Command started. Awaiting output...'}
          </pre>
        </div>
      </div>
    </article>
  );
};
