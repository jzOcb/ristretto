import { useState } from 'react';

import type { TestBlock } from '../lib/types';

export const TestResults = ({ block }: { block: TestBlock }) => {
  const [open, setOpen] = useState(block.failed > 0);

  return (
    <article className="rounded-lg border border-zinc-800/60 bg-zinc-900/60">
      <button
        className="flex w-full items-center justify-between gap-4 px-3.5 py-2.5 text-left"
        onClick={() => setOpen((value) => !value)}
        type="button"
      >
        <div>
          <p className="text-xs font-medium text-zinc-100">Test Results</p>
          <p className="text-[11px] text-zinc-500">
            {block.passed} passed, {block.failed} failed, {block.ignored} ignored
          </p>
        </div>
        <div className="flex items-center gap-1.5 text-[9px] uppercase tracking-wider">
          <span className="rounded-md bg-emerald-500/10 px-1.5 py-0.5 text-emerald-300">{block.passed} pass</span>
          <span className="rounded-md bg-rose-500/10 px-1.5 py-0.5 text-rose-300">{block.failed} fail</span>
        </div>
      </button>
      <div className="grid overflow-hidden transition-[grid-template-rows] duration-200" style={{ gridTemplateRows: open ? '1fr' : '0fr' }}>
        <div className="min-h-0 border-t border-zinc-800/40">
          <div className="max-h-72 overflow-auto px-3.5 py-2.5">
            {block.tests.length ? (
              <ul className="space-y-1.5">
                {block.tests.map((test) => (
                  <li className="flex items-center justify-between rounded-md bg-zinc-950/50 px-2.5 py-1.5 text-xs" key={test.name}>
                    <span className="font-mono text-zinc-300">{test.name}</span>
                    <span className={test.outcome === 'pass' ? 'text-emerald-400' : 'text-rose-400'}>
                      {test.outcome}
                    </span>
                  </li>
                ))}
              </ul>
            ) : (
              <pre className="font-mono text-xs leading-6 text-zinc-400">{block.raw}</pre>
            )}
          </div>
        </div>
      </div>
    </article>
  );
};
