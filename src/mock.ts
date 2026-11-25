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
    base.projects = [];
  }
  return base;
}

function normal(): Snapshot {
  const now = Date.now();
  const at = (min: number) => new Date(now + min * 60_000).toISOString();
  const widgetCwd = "C:\\Code\\claudecode-codex-widgetUsage";
  const landingCwd = "C:\\Code\\landing-web";
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
    projects: [
      {
        id: widgetCwd.toLowerCase(),
        name: "claudecode-codex-widgetUsage",
        cwd: widgetCwd,
        lastActivity: at(0),
        working: true,
        boss: {
          id: "c1", provider: "claude", title: "Contexto de la herramienta",
          cwd: widgetCwd, lastActivity: at(0), working: true, fromClaude: false,
          contextPercent: 68, contextApprox: false, subagents: [], moreSubagents: 0,
        },
        crew: [
          { id: "x1", kind: "codex", name: "Pauli", working: true, contextPercent: 42, lastActivity: at(-0.05) },
          { id: "x2", kind: "codex", name: "Erdos", working: true, contextPercent: 37, lastActivity: at(-0.1) },
          { id: "x3", kind: "codex", name: "Newton", working: true, contextPercent: 54, lastActivity: at(-0.15) },
          { id: "a1", kind: "claude-sub", name: "Buscar tests", working: true, contextPercent: null, lastActivity: at(-0.2) },
          { id: "x4", kind: "codex", name: "graphify", working: false, contextPercent: 23, lastActivity: at(-1) },
        ],
        moreCrew: 0,
      },
      {
        id: landingCwd.toLowerCase(),
        name: "landing-web",
        cwd: landingCwd,
        lastActivity: at(-2),
        working: false,
        boss: {
          id: "c2", provider: "claude", title: "landing",
          cwd: landingCwd, lastActivity: at(-2), working: false, fromClaude: false,
          contextPercent: 91, contextApprox: false, subagents: [], moreSubagents: 0,
        },
        crew: [],
        moreCrew: 0,
      },
    ],
  };
}
