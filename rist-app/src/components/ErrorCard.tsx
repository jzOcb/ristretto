import { useState } from 'react';

import type { ErrorBlock } from '../lib/types';

export const ErrorCard = ({ block }: { block: ErrorBlock }) => {
  const [open, setOpen] = useState(false);

  return (
    <article className="rounded-2xl border border-rose-500/30 bg-rose-500/10">
      <button
        className="flex w-full items-center justify-between gap-3 px-4 py-3 text-left"
        onClick={() => setOpen((value) => !value)}
        type="button"
      >
        <div>
          <p className="text-sm font-medium text-rose-100">{block.title}</p>
          <p className="mt-1 text-xs text-rose-200/80">{block.command ?? 'Agent output error'}</p>
        </div>
        <span className="text-xs uppercase tracking-[0.2em] text-rose-200">{open ? 'Collapse' : 'Expand'}</span>
      </button>
      <div className="grid overflow-hidden transition-[grid-template-rows] duration-200" style={{ gridTemplateRows: open ? '1fr' : '0fr' }}>
        <div className="min-h-0 border-t border-rose-400/20">
          <div className="space-y-3 px-4 py-3">
            <pre className="whitespace-pre-wrap font-mono text-xs leading-6 text-rose-100">{block.message}</pre>
            {block.stack.length ? (
              <div>
                <p className="mb-2 text-[10px] uppercase tracking-[0.2em] text-rose-200/80">Stack trace</p>
                <pre className="overflow-x-auto rounded-xl bg-zinc-950/50 p-3 font-mono text-xs text-rose-100">
                  {block.stack.join('\n')}
                </pre>
              </div>
            ) : null}
          </div>
        </div>
      </div>
    </article>
  );
};
