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

export interface AgentProcess {
  pid: number;
  provider: Provider;
  cwd: string | null;
  cpu: number;
  memoryMb: number;
  /** Unix epoch seconds */
  startedAt: number;
}

export interface Snapshot {
  claude: ProviderUsage;
  codex: ProviderUsage;
  processes: AgentProcess[];
  takenAt: string;
}

export interface PlatformInfo {
  os: string;
  nativeBlur: boolean;
}
