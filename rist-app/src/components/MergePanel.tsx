import { useState } from 'react';

import type { DiffHunkLine, MergeStrategy } from '../lib/types';
import { agentTypeLabel } from '../lib/types';
import { useAgentStore } from '../stores/agent-store';

const STRATEGIES: { value: MergeStrategy; label: string; desc: string }[] = [
  { value: 'ours', label: 'Ours', desc: 'Keep current branch changes on conflict' },
  { value: 'theirs', label: 'Theirs', desc: 'Accept agent branch changes on conflict' },
  { value: 'manual', label: 'Manual', desc: 'Pause on conflicts for manual resolution' },
];

const mockMergeAgent = async (
  _agentId: string,
  previewOnly: boolean,
  _strategy: MergeStrategy,
): Promise<{ diff: string; conflicts: string[] } | { success: true } | { error: string }> => {
  await new Promise((r) => setTimeout(r, 800));
  if (previewOnly) {
    return {
      diff: [
        'diff --git a/src/main.rs b/src/main.rs',
        'index 1a2b3c4..5d6e7f8 100644',
        '--- a/src/main.rs',
        '+++ b/src/main.rs',
        '@@ -10,6 +10,8 @@ fn main() {',
        '     let config = Config::load();',
        '     let runtime = Runtime::new();',
        '+    // Agent: added retry logic',
        '+    let retry = RetryPolicy::exponential(3);',
        '     runtime.start(config);',
        ' }',
        '',
        'diff --git a/src/lib/retry.rs b/src/lib/retry.rs',
        'new file mode 100644',
        '--- /dev/null',
        '+++ b/src/lib/retry.rs',
        '@@ -0,0 +1,12 @@',
        '+pub struct RetryPolicy {',
        '+    pub max_attempts: u32,',
        '+    pub backoff: BackoffStrategy,',
        '+}',
      ].join('\n'),
      conflicts: ['src/config.toml'],
    };
  }
  return { success: true };
};

const parseDiffLines = (raw: string): DiffHunkLine[] =>
  raw.split('\n').map((line) => {
    if (line.startsWith('+++') || line.startsWith('---') || line.startsWith('diff ') || line.startsWith('index ') || line.startsWith('@@') || line.startsWith('new file'))
      return { kind: 'meta' as const, value: line };
    if (line.startsWith('+')) return { kind: 'add' as const, value: line };
    if (line.startsWith('-')) return { kind: 'remove' as const, value: line };
    return { kind: 'context' as const, value: line };
  });

const lineTone = (kind: DiffHunkLine['kind']) => {
  switch (kind) {
    case 'add':
      return 'bg-emerald-500/10 text-emerald-200';
    case 'remove':
      return 'bg-rose-500/10 text-rose-200';
    case 'meta':
      return 'bg-zinc-800/50 text-zinc-500';
    default:
      return 'text-zinc-400';
  }
};

type MergeState =
  | { step: 'idle' }
  | { step: 'previewing' }
  | { step: 'preview'; diff: string; conflicts: string[] }
  | { step: 'merging' }
  | { step: 'success' }
  | { step: 'error'; message: string };

export const MergePanel = () => {
  const agents = useAgentStore((s) => s.agents);
  const showMergePanel = useAgentStore((s) => s.showMergePanel);
  const setShowMergePanel = useAgentStore((s) => s.setShowMergePanel);

  const eligibleAgents = agents.filter((a) => a.status === 'done' || a.status === 'idle');

  const [selectedAgentId, setSelectedAgentId] = useState<string>('');
  const [strategy, setStrategy] = useState<MergeStrategy>('theirs');
  const [state, setState] = useState<MergeState>({ step: 'idle' });

  if (!showMergePanel) return null;

  const selectedAgent = eligibleAgents.find((a) => a.id === selectedAgentId);

  const handlePreview = async () => {
    if (!selectedAgentId) return;
    setState({ step: 'previewing' });
    try {
      const result = await mockMergeAgent(selectedAgentId, true, strategy);
      if ('diff' in result) {
        setState({ step: 'preview', diff: result.diff, conflicts: result.conflicts });
      } else if ('error' in result) {
        setState({ step: 'error', message: result.error });
      }
    } catch (err) {
      setState({ step: 'error', message: String(err) });
    }
  };

  const handleMerge = async () => {
    if (!selectedAgentId) return;
    setState({ step: 'merging' });
    try {
      const result = await mockMergeAgent(selectedAgentId, false, strategy);
      if ('success' in result) {
        setState({ step: 'success' });
      } else if ('error' in result) {
        setState({ step: 'error', message: result.error });
      }
    } catch (err) {
      setState({ step: 'error', message: String(err) });
    }
  };

  const reset = () => {
    setState({ step: 'idle' });
    setSelectedAgentId('');
    setStrategy('theirs');
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" onClick={() => setShowMergePanel(false)}>
      <div
        className="flex max-h-[85vh] w-full max-w-2xl flex-col rounded-2xl border border-zinc-800/80 bg-[linear-gradient(180deg,rgba(24,24,27,0.98),rgba(9,9,11,0.99))] shadow-[0_32px_80px_rgba(0,0,0,0.5)]"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-zinc-800/60 px-5 py-4">
          <div>
            <p className="text-[10px] font-medium uppercase tracking-[0.12em] text-zinc-500">Merge Worktree</p>
            <h2 className="text-base font-semibold text-zinc-100">Merge Agent Work</h2>
          </div>
          <button
            className="rounded-lg p-1.5 text-zinc-500 transition hover:bg-zinc-800 hover:text-zinc-300"
            onClick={() => setShowMergePanel(false)}
            type="button"
          >
            <svg className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth={2} viewBox="0 0 24 24">
              <path d="M6 18L18 6M6 6l12 12" strokeLinecap="round" />
            </svg>
          </button>
        </div>

        {/* Body */}
        <div className="flex-1 space-y-4 overflow-y-auto p-5">
          {/* Agent selector */}
          <div className="grid gap-4 sm:grid-cols-2">
            <label className="space-y-1.5">
              <span className="text-[10px] font-medium text-zinc-500">Agent</span>
              <select
                className="w-full rounded-lg border border-zinc-800 bg-zinc-950 px-3 py-2.5 text-sm text-zinc-200 outline-none focus:border-violet-500/50"
                onChange={(e) => {
                  setSelectedAgentId(e.target.value);
                  setState({ step: 'idle' });
                }}
                value={selectedAgentId}
              >
                <option value="">Select agent...</option>
                {eligibleAgents.map((agent) => (
                  <option key={agent.id} value={agent.id}>
                    {agent.task.slice(0, 50)} ({agentTypeLabel(agent.agent_type)}) — {agent.status}
                  </option>
                ))}
              </select>
            </label>
            <label className="space-y-1.5">
              <span className="text-[10px] font-medium text-zinc-500">Strategy</span>
              <select
                className="w-full rounded-lg border border-zinc-800 bg-zinc-950 px-3 py-2.5 text-sm text-zinc-200 outline-none focus:border-violet-500/50"
                onChange={(e) => setStrategy(e.target.value as MergeStrategy)}
                value={strategy}
              >
                {STRATEGIES.map((s) => (
                  <option key={s.value} value={s.value}>{s.label} — {s.desc}</option>
                ))}
              </select>
            </label>
          </div>

          {/* Agent info */}
          {selectedAgent ? (
            <div className="rounded-lg border border-zinc-800/60 bg-zinc-900/60 p-3">
              <div className="flex items-center gap-3 text-xs">
                <span className="font-mono text-zinc-500">{selectedAgent.id.slice(0, 8)}</span>
                {selectedAgent.branch ? (
                  <span className="rounded border border-violet-500/20 bg-violet-500/10 px-1.5 py-0.5 font-mono text-[10px] text-violet-300">
                    {selectedAgent.branch}
                  </span>
                ) : null}
                <span className="text-zinc-500">{selectedAgent.workdir}</span>
              </div>
              {selectedAgent.file_ownership.length > 0 ? (
                <div className="mt-2 flex flex-wrap gap-1">
                  {selectedAgent.file_ownership.map((f) => (
                    <span className="rounded bg-zinc-800 px-1.5 py-0.5 font-mono text-[10px] text-zinc-400" key={f}>{f}</span>
                  ))}
                </div>
              ) : null}
            </div>
          ) : null}

          {/* Preview diff */}
          {state.step === 'preview' ? (
            <div className="space-y-3">
              {state.conflicts.length > 0 ? (
                <div className="rounded-lg border border-amber-500/25 bg-amber-500/8 p-3">
                  <p className="text-xs font-medium text-amber-300">
                    {state.conflicts.length} conflict{state.conflicts.length > 1 ? 's' : ''} detected
                  </p>
                  <div className="mt-1.5 flex flex-wrap gap-1">
                    {state.conflicts.map((f) => (
                      <span className="rounded bg-amber-500/15 px-1.5 py-0.5 font-mono text-[10px] text-amber-200" key={f}>{f}</span>
                    ))}
                  </div>
                </div>
              ) : (
                <div className="rounded-lg border border-emerald-500/25 bg-emerald-500/8 p-3">
                  <p className="text-xs font-medium text-emerald-300">No conflicts — clean merge</p>
                </div>
              )}

              <div className="rounded-lg border border-zinc-800/60 bg-zinc-900/60">
                <div className="border-b border-zinc-800/40 px-3.5 py-2.5">
                  <p className="text-xs font-medium text-zinc-100">Diff Preview</p>
                </div>
                <pre className="max-h-64 overflow-auto p-2.5 font-mono text-xs leading-6">
                  {parseDiffLines(state.diff).map((line, i) => (
                    <div className={`px-2 ${lineTone(line.kind)}`} key={i}>
                      {line.value || ' '}
                    </div>
                  ))}
                </pre>
              </div>
            </div>
          ) : null}

          {/* Status feedback */}
          {state.step === 'success' ? (
            <div className="rounded-lg border border-emerald-500/25 bg-emerald-500/8 p-4 text-center">
              <p className="text-sm font-medium text-emerald-300">Merge completed successfully</p>
              <button
                className="mt-2 text-xs text-zinc-400 underline underline-offset-2 hover:text-zinc-300"
                onClick={reset}
                type="button"
              >
                Merge another
              </button>
            </div>
          ) : null}

          {state.step === 'error' ? (
            <div className="rounded-lg border border-rose-500/25 bg-rose-500/8 p-4">
              <p className="text-xs font-medium text-rose-300">Merge failed</p>
              <p className="mt-1 font-mono text-[11px] text-rose-200/80">{state.message}</p>
            </div>
          ) : null}
        </div>

        {/* Footer actions */}
        <div className="flex items-center justify-end gap-2 border-t border-zinc-800/60 px-5 py-3">
          <button
            className="rounded-lg border border-zinc-700 px-3 py-2 text-xs text-zinc-400 transition hover:border-zinc-500 hover:text-zinc-300"
            onClick={() => setShowMergePanel(false)}
            type="button"
          >
            Cancel
          </button>
          {state.step !== 'preview' ? (
            <button
              className="rounded-lg border border-violet-500/30 bg-violet-500/10 px-4 py-2 text-xs font-medium text-violet-300 transition hover:bg-violet-500/20 disabled:opacity-40"
              disabled={!selectedAgentId || state.step === 'previewing'}
              onClick={handlePreview}
              type="button"
            >
              {state.step === 'previewing' ? 'Loading...' : 'Preview'}
            </button>
          ) : (
            <button
              className="rounded-lg bg-violet-600 px-4 py-2 text-xs font-medium text-white transition hover:bg-violet-500 disabled:opacity-40"
              disabled={state.step === 'merging' as any}
              onClick={handleMerge}
              type="button"
            >
              {state.step === ('merging' as any) ? 'Merging...' : 'Merge'}
            </button>
          )}
        </div>
      </div>
    </div>
  );
};
