import type {
  CommandBlock,
  DiffBlock,
  DiffHunkLine,
  ErrorBlock,
  OutputBlock,
  TestBlock,
  TestCaseResult,
  TextBlock,
} from './types';

const COMMAND_PATTERN = /^\$\s+.+/;
const DIFF_PATTERN = /^(diff --git|index [0-9a-f]+\.\.[0-9a-f]+|@@ |--- |\+\+\+ )/m;
const TEST_PATTERN =
  /(running \d+ tests|test result:|^\s*test .+ \.\.\. (ok|FAILED)|^\s*failures:$)/m;
const ERROR_PATTERN =
  /((^|\n)(error(\[[^\]]+\])?:|Error:|thread '.*' panicked at|Traceback \(most recent call last\):|Caused by:))/m;
const STACK_PATTERN = /^\s*(at |Stack trace:|[0-9]+:\s|File ".*", line \d+)/;

interface Segment {
  command?: string;
  content: string;
  startedAtLine: number;
}

const flushText = (blocks: OutputBlock[], text: string, seed: number) => {
  const trimmed = text.trim();
  if (!trimmed) {
    return;
  }

  const fallback = parseStandalone(trimmed, seed);
  blocks.push(...fallback);
};

const parseStandalone = (content: string, seed: number): OutputBlock[] => {
  if (DIFF_PATTERN.test(content)) {
    return [buildDiffBlock(content, undefined, seed)];
  }

  if (TEST_PATTERN.test(content)) {
    return [buildTestBlock(content, undefined, seed)];
  }

  if (ERROR_PATTERN.test(content)) {
    return [buildErrorBlock(content, undefined, seed)];
  }

  return [
    {
      id: `text-${seed}`,
      type: 'text',
      text: content,
    } satisfies TextBlock,
  ];
};

const parseCommandSegment = (segment: Segment, seed: number): OutputBlock[] => {
  const blocks: OutputBlock[] = [];
  const content = segment.content.trimEnd();

  if (segment.command) {
    blocks.push(buildCommandBlock(segment.command, content, segment.startedAtLine, seed));
  }

  if (!content.trim()) {
    return blocks;
  }

  if (DIFF_PATTERN.test(content)) {
    blocks.push(buildDiffBlock(content, segment.command, seed + 1));
  } else if (TEST_PATTERN.test(content)) {
    blocks.push(buildTestBlock(content, segment.command, seed + 1));
  } else if (ERROR_PATTERN.test(content)) {
    blocks.push(buildErrorBlock(content, segment.command, seed + 1));
  } else {
    blocks.push({
      id: `text-${seed + 1}`,
      type: 'text',
      text: content,
    });
  }

  return blocks;
};

const buildCommandBlock = (
  command: string,
  content: string,
  startedAtLine: number,
  seed: number,
): CommandBlock => {
  const failed = /(FAILED|error(\[[^\]]+\])?:|panicked at|fatal:|Command failed)/m.test(content);
  const success = /(Finished |test result: ok|All tests passed|Done in )/m.test(content);

  return {
    id: `command-${seed}`,
    type: 'command',
    command,
    content,
    summary: summarizeCommand(command, content),
    status: failed ? 'failure' : success ? 'success' : 'running',
    startedAtLine,
  };
};

const summarizeCommand = (command: string, content: string): string => {
  if (command.startsWith('$ cargo test') || TEST_PATTERN.test(content)) {
    const totals = content.match(/test result:\s+(ok|FAILED)\.\s+(\d+)\s+passed;\s+(\d+)\s+failed;\s+(\d+)\s+ignored;/);
    if (totals) {
      return `${totals[2]} passed, ${totals[3]} failed, ${totals[4]} ignored`;
    }
  }

  if (DIFF_PATTERN.test(content)) {
    const files = (content.match(/^diff --git /gm) ?? []).length;
    return `${files || 1} file ${files === 1 ? 'changed' : 'changed'}`;
  }

  if (ERROR_PATTERN.test(content)) {
    return 'Execution surfaced an error';
  }

  const preview = content.split('\n').find((line) => line.trim());
  return preview?.slice(0, 80) ?? command.replace(/^\$\s*/, '');
};

const buildDiffBlock = (content: string, command: string | undefined, seed: number): DiffBlock => {
  const lines: DiffHunkLine[] = content.split('\n').map((line) => {
    if (/^(diff --git|index |@@ |--- |\+\+\+ )/.test(line)) {
      return { kind: 'meta', value: line };
    }
    if (line.startsWith('+') && !line.startsWith('+++')) {
      return { kind: 'add', value: line };
    }
    if (line.startsWith('-') && !line.startsWith('---')) {
      return { kind: 'remove', value: line };
    }
    return { kind: 'context', value: line };
  });

  return {
    id: `diff-${seed}`,
    type: 'diff',
    command,
    fileCount: (content.match(/^diff --git /gm) ?? []).length || 1,
    lines,
  };
};

const buildTestBlock = (content: string, command: string | undefined, seed: number): TestBlock => {
  const tests: TestCaseResult[] = content
    .split('\n')
    .map((line) => line.match(/^\s*test\s+(.+)\s+\.\.\.\s+(ok|FAILED)$/))
    .filter((match): match is RegExpMatchArray => Boolean(match))
    .map((match) => ({
      name: match[1],
      outcome: match[2] === 'ok' ? 'pass' : 'fail',
    }));

  const totalMatch = content.match(
    /test result:\s+(ok|FAILED)\.\s+(\d+)\s+passed;\s+(\d+)\s+failed;\s+(\d+)\s+ignored;/,
  );

  return {
    id: `test-${seed}`,
    type: 'test',
    command,
    passed: totalMatch ? Number(totalMatch[2]) : tests.filter((test) => test.outcome === 'pass').length,
    failed: totalMatch ? Number(totalMatch[3]) : tests.filter((test) => test.outcome === 'fail').length,
    ignored: totalMatch ? Number(totalMatch[4]) : 0,
    raw: content,
    tests,
  };
};

const buildErrorBlock = (content: string, command: string | undefined, seed: number): ErrorBlock => {
  const lines = content.split('\n');
  const title = lines.find((line) => ERROR_PATTERN.test(line)) ?? 'Execution error';
  const stack = lines.filter((line) => STACK_PATTERN.test(line));
  const message = lines.filter((line) => !STACK_PATTERN.test(line)).join('\n').trim();

  return {
    id: `error-${seed}`,
    type: 'error',
    command,
    title: title.trim(),
    message,
    stack,
  };
};

export const parseOutput = (rawOutput: string): OutputBlock[] => {
  const normalized = rawOutput.replace(/\r\n/g, '\n');
  const lines = normalized.split('\n');
  const blocks: OutputBlock[] = [];

  let textBuffer = '';
  let currentSegment: Segment | null = null;

  for (const [index, line] of lines.entries()) {
    if (COMMAND_PATTERN.test(line)) {
      if (currentSegment) {
        blocks.push(...parseCommandSegment(currentSegment, blocks.length + index));
      } else {
        flushText(blocks, textBuffer, blocks.length + index);
        textBuffer = '';
      }

      currentSegment = {
        command: line.trim(),
        content: '',
        startedAtLine: index,
      };
      continue;
    }

    if (currentSegment) {
      currentSegment.content += `${line}\n`;
    } else {
      textBuffer += `${line}\n`;
    }
  }

  if (currentSegment) {
    blocks.push(...parseCommandSegment(currentSegment, blocks.length + lines.length));
  } else {
    flushText(blocks, textBuffer, blocks.length + lines.length);
  }

  return blocks.length
    ? blocks
    : [
        {
          id: 'text-empty',
          type: 'text',
          text: 'No output yet. Agent activity will appear here as structured blocks.',
        },
      ];
};
