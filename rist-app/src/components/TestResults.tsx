import { useState } from 'react';

import type { TestBlock } from '../lib/types';

export const TestResults = ({ block }: { block: TestBlock }) => {
  const [open, setOpen] = useState(block.failed > 0);

  return (
    <article className="rounded-2xl border border-zinc-800 bg-zinc-900/75">
      <button
        className="flex w-full items-center justify-between gap-4 px-4 py-3 text-left"
        onClick={() => setOpen((value) => !value)}
        type="button"
      >
        <div>
          <p className="text-sm font-medium text-zinc-100">Test Results</p>
          <p className="text-xs text-zinc-400">
            {block.passed} passed, {block.failed} failed, {block.ignored} ignored
          </p>
        </div>
        <div className="flex items-center gap-2 text-[10px] uppercase tracking-[0.2em]">
          <span className="rounded-full bg-emerald-500/10 px-2 py-1 text-emerald-200">{block.passed} pass</span>
          <span className="rounded-full bg-rose-500/10 px-2 py-1 text-rose-200">{block.failed} fail</span>
        </div>
      </button>
      <div className="grid overflow-hidden transition-[grid-template-rows] duration-200" style={{ gridTemplateRows: open ? '1fr' : '0fr' }}>
        <div className="min-h-0 border-t border-zinc-800">
          <div className="max-h-72 overflow-auto px-4 py-3">
            {block.tests.length ? (
              <ul className="space-y-2">
                {block.tests.map((test) => (
                  <li className="flex items-center justify-between rounded-xl bg-zinc-950/70 px-3 py-2 text-xs" key={test.name}>
                    <span className="font-mono text-zinc-300">{test.name}</span>
                    <span className={test.outcome === 'pass' ? 'text-emerald-300' : 'text-rose-300'}>
                      {test.outcome}
                    </span>
                  </li>
                ))}
              </ul>
            ) : (
              <pre className="font-mono text-xs leading-6 text-zinc-300">{block.raw}</pre>
            )}
          </div>
        </div>
      </div>
    </article>
  );
};
