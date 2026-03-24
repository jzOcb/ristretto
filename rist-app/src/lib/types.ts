export type SessionId = string;

export type AgentType =
  | { kind: 'claude' }
  | { kind: 'codex' }
  | { kind: 'gemini' }
  | { kind: 'custom'; value: string }
  | { kind: 'unknown' };

export type AgentStatus =
  | 'idle'
  | 'working'
  | 'thinking'
  | 'waiting'
  | 'stuck'
  | 'done'
  | 'error'
  | 'unknown';

export type TaskStatus =
  | 'pending'
  | 'assigned'
  | 'working'
  | 'review'
  | 'done'
  | 'blocked'
  | 'unknown';

export type Priority = 'critical' | 'high' | 'medium' | 'low' | 'unknown';

export interface ContextUsage {
  estimated_tokens: number;
  max_tokens: number;
  percentage: number;
}

export interface AgentInfo {
  id: SessionId;
  agent_type: AgentType;
  model: string | null;
  task: string;
  status: AgentStatus;
  workdir: string;
  branch: string | null;
  file_ownership: string[];
  created_at: string;
  last_output_at: string | null;
  context_usage: ContextUsage | null;
  exit_code: number | null;
  metadata: Record<string, string>;
}

export interface Task {
  id: string;
  title: string;
  description: string | null;
  status: TaskStatus;
  priority: Priority;
  agent_type: AgentType | null;
  owner: SessionId | null;
  depends_on: string[];
  file_ownership: string[];
}

export interface CommandBlock {
  id: string;
  type: 'command';
  command: string;
  content: string;
  summary: string;
  status: 'running' | 'success' | 'failure';
  startedAtLine: number;
}

export interface DiffHunkLine {
  kind: 'meta' | 'add' | 'remove' | 'context';
  value: string;
}

export interface DiffBlock {
  id: string;
  type: 'diff';
  command?: string;
  fileCount: number;
  lines: DiffHunkLine[];
}

export interface TestCaseResult {
  name: string;
  outcome: 'pass' | 'fail';
}

export interface TestBlock {
  id: string;
  type: 'test';
  command?: string;
  passed: number;
  failed: number;
  ignored: number;
  raw: string;
  tests: TestCaseResult[];
}

export interface ErrorBlock {
  id: string;
  type: 'error';
  command?: string;
  title: string;
  message: string;
  stack: string[];
}

export interface TextBlock {
  id: string;
  type: 'text';
  text: string;
}

export type OutputBlock = CommandBlock | DiffBlock | TestBlock | ErrorBlock | TextBlock;

export interface ConnectionState {
  connected: boolean;
  message: string;
}

export interface AgentOutputEvent {
  agent_id: string;
  data: string;
}

export interface AgentStatusEvent {
  agent_id: string;
  old_status: AgentStatus;
  new_status: AgentStatus;
  exit_code: number | null;
}

export interface TaskUpdateEvent {
  task_id: string;
  status: TaskStatus;
}

export interface ContextWarningEvent {
  agent_id: string;
  usage_pct: number;
}

export interface LoopDetectedEvent {
  agent_id: string;
  pattern: string;
}

export type ActivityEventType = 'spawn' | 'done' | 'error' | 'warning' | 'loop' | 'status';

export interface ActivityEvent {
  id: string;
  timestamp: number;
  agentId: string;
  type: ActivityEventType;
  message: string;
}

export const activityDot = (type: ActivityEventType): string => {
  switch (type) {
    case 'spawn':
      return 'bg-violet-500';
    case 'done':
      return 'bg-emerald-500';
    case 'error':
      return 'bg-rose-500';
    case 'warning':
      return 'bg-amber-400';
    case 'loop':
      return 'bg-amber-400';
    case 'status':
      return 'bg-sky-500';
  }
};

export const agentTypeLabel = (agentType: AgentType | null | undefined): string => {
  if (!agentType) {
    return 'unassigned';
  }

  switch (agentType.kind) {
    case 'custom':
      return agentType.value;
    case 'claude':
    case 'codex':
    case 'gemini':
    case 'unknown':
      return agentType.kind;
    default:
      return 'unknown';
  }
};

export const statusTone = (status: AgentStatus | TaskStatus): string => {
  switch (status) {
    case 'done':
      return 'text-emerald-300';
    case 'working':
    case 'assigned':
    case 'review':
      return 'text-sky-300';
    case 'blocked':
    case 'stuck':
    case 'error':
      return 'text-rose-300';
    case 'waiting':
      return 'text-amber-300';
    default:
      return 'text-zinc-400';
  }
};

export const statusDot = (status: AgentStatus | TaskStatus): string => {
  switch (status) {
    case 'done':
      return 'bg-emerald-500';
    case 'working':
    case 'assigned':
    case 'review':
      return 'bg-sky-500';
    case 'blocked':
    case 'stuck':
    case 'error':
      return 'bg-rose-500';
    case 'waiting':
      return 'bg-amber-400';
    default:
      return 'bg-zinc-500';
  }
};
