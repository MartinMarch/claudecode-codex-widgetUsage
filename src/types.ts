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
}

export interface Chat {
  id: string;
  provider: Provider;
  title: string;
  cwd: string | null;
  lastActivity: string;
  working: boolean;
  fromClaude: boolean;
  contextPercent: number | null;
  contextApprox: boolean;
  subagents: Subagent[];
  moreSubagents: number;
}

export interface Snapshot {
  claude: ProviderUsage;
  codex: ProviderUsage;
  chats: Chat[];
  takenAt: string;
}

export interface PlatformInfo {
  os: string;
  nativeBlur: boolean;
}
