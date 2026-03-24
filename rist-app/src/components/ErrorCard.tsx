import { useState } from 'react';

import type { ErrorBlock } from '../lib/types';

export const ErrorCard = ({ block }: { block: ErrorBlock }) => {
  const [open, setOpen] = useState(false);

  return (
    <article className="rounded-lg border border-rose-500/25 bg-rose-500/8">
      <button
        className="flex w-full items-center justify-between gap-3 px-3.5 py-2.5 text-left"
        onClick={() => setOpen((value) => !value)}
        type="button"
      >
        <div>
          <p className="text-xs font-medium text-rose-100">{block.title}</p>
          <p className="mt-0.5 text-[11px] text-rose-200/70">{block.command ?? 'Agent output error'}</p>
        </div>
        <span className="text-[10px] text-rose-300/60">{open ? 'Collapse' : 'Expand'}</span>
      </button>
      <div className="grid overflow-hidden transition-[grid-template-rows] duration-200" style={{ gridTemplateRows: open ? '1fr' : '0fr' }}>
        <div className="min-h-0 border-t border-rose-400/15">
          <div className="space-y-2.5 px-3.5 py-2.5">
            <pre className="whitespace-pre-wrap font-mono text-xs leading-6 text-rose-100">{block.message}</pre>
            {block.stack.length ? (
              <div>
                <p className="mb-1.5 text-[9px] uppercase tracking-wider text-rose-200/60">Stack trace</p>
                <pre className="overflow-x-auto rounded-md bg-zinc-950/50 p-2.5 font-mono text-xs text-rose-100">
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
