import { useState } from 'react';

import type { DiffBlock } from '../lib/types';

const lineTone = (kind: DiffBlock['lines'][number]['kind']) => {
  switch (kind) {
    case 'add':
      return 'bg-emerald-500/10 text-emerald-200';
    case 'remove':
      return 'bg-rose-500/10 text-rose-200';
    case 'meta':
      return 'bg-zinc-800/70 text-zinc-400';
    default:
      return 'text-zinc-300';
  }
};

export const DiffViewer = ({ block }: { block: DiffBlock }) => {
  const [open, setOpen] = useState(true);

  return (
    <article className="rounded-2xl border border-zinc-800 bg-zinc-900/75">
      <button
        className="flex w-full items-center justify-between px-4 py-3 text-left"
        onClick={() => setOpen((value) => !value)}
        type="button"
      >
        <div>
          <p className="text-sm font-medium text-zinc-100">Diff Output</p>
          <p className="text-xs text-zinc-400">
            {block.fileCount} {block.fileCount === 1 ? 'file' : 'files'} touched
          </p>
        </div>
        <span className="text-xs uppercase tracking-[0.2em] text-zinc-500">{open ? 'Hide' : 'Show'}</span>
      </button>
      <div className="grid overflow-hidden transition-[grid-template-rows] duration-200" style={{ gridTemplateRows: open ? '1fr' : '0fr' }}>
        <div className="min-h-0">
          <pre className="overflow-x-auto border-t border-zinc-800 p-3 font-mono text-xs leading-6">
            {block.lines.map((line, index) => (
              <div className={`px-2 ${lineTone(line.kind)}`} key={`${block.id}-${index}`}>
                {line.value || ' '}
              </div>
            ))}
          </pre>
        </div>
      </div>
    </article>
  );
};
