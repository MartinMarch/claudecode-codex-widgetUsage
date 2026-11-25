// Mirrors src-tauri/src/model.rs. Keep both in sync.

export type Provider = "claude" | "codex";
export type Status = "ok" | "unavailable" | "error";

export interface LimitWindow {
  usedPercent: number | null;
  resetsAt: string | null;
  windowMinutes: number;
}

export interface ProviderUsage {
  provider: Provider;
  status: Status;
  message: string | null;
  plan: string | null;
  primary: LimitWindow | null;
  secondary: LimitWindow | null;
  tokens5h: number | null;
  observedAt: string | null;
}

export interface Subagent {
  id: string;
  name: string;
  kind: string | null;
  lastActivity: string;
  working: boolean;
  asleep: boolean;
}

export interface Chat {
  id: string;
  envId: string;
  provider: Provider;
  title: string;
  cwd: string | null;
  lastActivity: string;
  working: boolean;
  asleep: boolean;
  fromClaude: boolean;
  contextPercent: number | null;
  contextApprox: boolean;
  subagents: Subagent[];
  moreSubagents: number;
}

export type CrewKind = "codex" | "codex-sub" | "claude-chat" | "claude-sub";

export interface CrewMember {
  id: string;
  kind: CrewKind;
  name: string;
  working: boolean;
  asleep: boolean;
  contextPercent: number | null;
  lastActivity: string;
}

export interface Project {
  id: string;
  envId: string;
  name: string;
  cwd: string | null;
  lastActivity: string;
  working: boolean;
  asleep: boolean;
  boss: Chat | null;
  crew: CrewMember[];
  moreCrew: number;
}

export type EnvKind = "local" | "ssh" | "cloud";

export interface Environment {
  id: string;
  name: string;
  kind: EnvKind;
}

export interface Snapshot {
  claude: ProviderUsage;
  codex: ProviderUsage;
  projects: Project[];
  environments: Environment[];
  takenAt: string;
}

export interface PlatformInfo {
  os: string;
  nativeBlur: boolean;
}

export type LogLevel = "ok" | "warn" | "error";

/** One line of the activity log (src-tauri/src/activity.rs). Newest first. */
export interface LogEntry {
  at: string;
  source: string;
  level: LogLevel;
  message: string;
  /** How many times in a row this same message happened. */
  count: number;
}
