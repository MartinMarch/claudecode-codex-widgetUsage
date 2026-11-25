import type { Snapshot } from "./types";

/**
 * Realistic sample data for `npm run dev` in a plain browser.
 * Add `?demo=hot` (Claude about to hit its limit) or `?demo=empty`
 * (nothing installed, no sessions) to preview the other states.
 */
export function mockSnapshot(): Snapshot {
  const demo = new URLSearchParams(location.search).get("demo");
  const base = normal();
  if (demo === "hot") {
    base.claude.primary = { usedPercent: 88, resetsAt: new Date(Date.now() + 150 * 60_000).toISOString(), windowMinutes: 300 };
    base.claude.secondary = { usedPercent: 79, resetsAt: new Date(Date.now() + 26 * 3600_000).toISOString(), windowMinutes: 10080 };
  }
  if (demo === "empty") {
    base.claude = { ...base.claude, status: "error", primary: null, secondary: null, tokens5h: null,
      message: "La sesión de Claude Code caducó. Abre `claude` una vez para renovarla." };
    base.codex = { ...base.codex, status: "unavailable", primary: null, secondary: null, tokens5h: null, plan: null,
      message: "Codex no está instalado en este equipo." };
    base.processes = [];
  }
  return base;
}

function normal(): Snapshot {
  const now = Date.now();
  const at = (min: number) => new Date(now + min * 60_000).toISOString();
  const ago = (min: number) => Math.floor((now - min * 60_000) / 1000);
  const jitter = (n: number) => Math.max(0, n + (Math.random() - 0.5) * 6);
  return {
    takenAt: new Date(now).toISOString(),
    claude: {
      provider: "claude",
      status: "ok",
      message: null,
      plan: "max",
      primary: { usedPercent: 64, resetsAt: at(92), windowMinutes: 300 },
      secondary: { usedPercent: 41, resetsAt: at(60 * 24 * 3 + 200), windowMinutes: 10080 },
      tokens5h: 18_420_000,
      observedAt: new Date(now).toISOString(),
    },
    codex: {
      provider: "codex",
      status: "ok",
      message: null,
      plan: "plus",
      primary: { usedPercent: 22, resetsAt: at(201), windowMinutes: 300 },
      secondary: { usedPercent: 9, resetsAt: at(60 * 24 * 5 + 30), windowMinutes: 10080 },
      tokens5h: 2_310_000,
      observedAt: new Date(now - 4 * 60_000).toISOString(),
    },
    processes: [
      { pid: 48213, provider: "claude", cwd: "/Users/you/code/checkout-api", cpu: jitter(14), memoryMb: 312, startedAt: ago(38) },
      { pid: 47790, provider: "codex", cwd: "/Users/you/code/landing-web", cpu: jitter(3), memoryMb: 148, startedAt: ago(12) },
      { pid: 46102, provider: "claude", cwd: "/Users/you/code/infra", cpu: 0, memoryMb: 201, startedAt: ago(131) },
    ],
  };
}
