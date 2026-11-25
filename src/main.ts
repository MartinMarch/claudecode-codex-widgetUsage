import "@fontsource-variable/unbounded";
import "@fontsource-variable/manrope";
import "./styles.css";

import { fetchPlatform, fetchSnapshot, fitWindow, hideWindow } from "./api";
import { bossCritter, claudeCritter, codexCritter, fillCritter, whip } from "./critters";
import { duration, folder, projection, since, tokens } from "./format";
import { gauge } from "./gauge";
import type { CrewMember, Project, Provider, ProviderUsage, Snapshot } from "./types";

const POLL_MS = 20_000;
const NAMES = { claude: "Claude Code", codex: "Codex" } as const;
const HOT = { claude: 80, codex: 85 } as const;
const KIND_LABEL = { codex: "Codex", "codex-sub": "Subagente de Codex", "claude-chat": "Chat de Claude", "claude-sub": "Subagente" } as const;

const root = document.getElementById("app")!;
let last: Snapshot | null = null;
let compact = false;
let firstPaint = true;

// Which environment tab is open ("all" or an environment id), remembered per viewer.
const ENV_KEY = "agent-gauge.env";
const ALL_TAB_MAX = 4;
let envFilter = (() => {
  try {
    return localStorage.getItem(ENV_KEY) ?? "all";
  } catch {
    return "all";
  }
})();

const esc = (s: string) =>
  s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

/** Escapes, then turns `command` spans from backend messages into <code>. */
const rich = (s: string) => esc(s).replace(/`([^`]+)`/g, "<code>$1</code>");

const pct = (n: number | null | undefined) => (n == null ? "–" : String(Math.round(n)));

function ctx(percent: number | null, approx: boolean, provider: Provider): string {
  if (percent == null) return `<span class="ctx">–</span>`;
  const hot = percent >= HOT[provider];
  return `<span class="ctx${hot ? " is-hot" : ""}" title="Contexto usado del chat"><span class="bar"><i style="width:${Math.min(100, percent)}%"></i></span>${approx ? "≈" : ""}${Math.round(percent)} %</span>`;
}

function account(u: ProviderUsage, working: boolean): string {
  const name = NAMES[u.provider];
  const critter = `<div class="critter"><span class="puff"></span>${u.provider === "claude" ? claudeCritter() : codexCritter()}</div>`;
  if (u.status !== "ok" || !u.primary) {
    return `
    <section class="acc acc-${u.provider} is-empty" aria-label="${name}">
      ${critter}
      <div class="meter">${gauge(u.provider, null, null)}
        <div><div class="readout"><span class="num">–</span></div><h2 class="who">${name}</h2></div>
      </div>
      <p class="note reveal-static" style="grid-column:1/-1">${rich(u.message ?? "Sin datos.")}</p>
    </section>`;
  }
  const p = projection(u.primary);
  const reset = u.primary.resetsAt ? Date.parse(u.primary.resetsAt) - Date.now() : null;
  const week = u.secondary?.usedPercent ?? null;
  const pace =
    p == null
      ? ""
      : p.hitsLimitIn != null
        ? `<p class="kv warn"><span>A este ritmo</span><span>límite en ${duration(p.hitsLimitIn)}</span></p>`
        : `<p class="kv"><span>A este ritmo</span><span>${Math.round(p.atReset)} % al renovar</span></p>`;
  return `
  <section class="acc acc-${u.provider}${working ? " is-working" : ""}" data-provider="${u.provider}" tabindex="0"
           aria-label="${name}: ${pct(u.primary.usedPercent)} % de la sesión de 5 horas">
    ${critter}
    <div class="meter">
      ${gauge(u.provider, u.primary.usedPercent, p?.atReset ?? null)}
      <div>
        <div class="readout"><span class="num">${pct(u.primary.usedPercent)}</span><span class="unit">%</span></div>
        <h2 class="who">${name}${u.plan ? `<span class="plan">${esc(u.plan)}</span>` : ""}</h2>
        <p class="reset">${reset != null ? `se renueva en ${duration(reset)}` : "sesión de 5 h"}</p>
      </div>
    </div>
    <div class="reveal"><div>
      ${week != null ? `<p class="kv"><span>Semana</span><span>${pct(week)} %</span></p><div class="bar"><i style="width:${Math.min(100, week)}%"></i></div>` : ""}
      ${pace}
      ${u.tokens5h ? `<p class="kv"><span>Tokens en 5 h</span><span>${tokens(u.tokens5h)}</span></p>` : ""}
    </div></div>
  </section>`;
}

function mate(m: CrewMember): string {
  const small = m.kind === "codex-sub" || m.kind === "claude-sub";
  const art = m.kind.startsWith("codex") ? codexCritter({ small, awake: !m.asleep }) : claudeCritter({ small, awake: !m.asleep });
  const state = m.asleep ? " is-asleep" : m.working ? "" : " is-idle";
  return `<span class="mate${state}" title="${esc(m.name)}"><span class="sweat"></span>${m.asleep ? `<span class="zzz">z</span>` : ""}${art}</span>`;
}

function whoRow(provider: Provider, name: string, label: string, working: boolean, right: string): string {
  return `<div class="who-row"><span class="dot dot-${provider}${working ? " is-busy" : ""}"></span>
    <span class="name">${esc(name)} <span class="reset">· ${label}</span></span><span>${right}</span></div>`;
}

function project(p: Project, envName?: string): string {
  const boss = p.boss;
  const crew = p.crew.map(mate).join("");
  const more = p.moreCrew > 0 ? `<span class="crew-more">+${p.moreCrew}</span>` : "";
  const waiting =
    boss && !p.working && !p.asleep && !p.crew.length ? `<span class="waiting">esperando tu respuesta</span>` : "";
  const list = [
    boss ? whoRow("claude", boss.title, "jefe", boss.working, boss.contextPercent == null ? "–" : `${Math.round(boss.contextPercent)} %`) : "",
    ...p.crew.map((m) =>
      whoRow(
        m.kind.startsWith("codex") ? "codex" : "claude",
        m.name,
        KIND_LABEL[m.kind],
        m.working,
        m.contextPercent != null ? `${Math.round(m.contextPercent)} %` : m.working ? "trabajando" : m.asleep ? "dormido" : "en espera",
      ),
    ),
  ].join("");
  return `
  <section class="proj${p.working ? " is-working" : ""}${p.asleep ? " is-asleep" : ""}${boss ? "" : " no-boss"}" tabindex="0" title="${esc(p.cwd ?? "")}">
    <div class="proj-head"><span class="proj-name">${esc(p.name || folder(p.cwd))}${envName ? `<span class="env-tag">${esc(envName)}</span>` : ""}</span>
      ${boss ? ctx(boss.contextPercent, boss.contextApprox, "claude") : ""}</div>
    <div class="stage">
      ${boss ? `<div class="boss">${p.asleep ? `<span class="zzz">z</span>` : ""}${bossCritter(!p.asleep)}${whip()}</div>` : ""}
      <div class="crew">${crew}${more}${waiting}</div>
    </div>
    <div class="reveal"><div>${list}</div></div>
  </section>`;
}

/** Which tools have something working right now (drives the walking critters). */
function workingTools(s: Snapshot): Record<Provider, boolean> {
  const out = { claude: false, codex: false };
  for (const p of s.projects) {
    if (p.boss?.working) out.claude = true;
    for (const m of p.crew) if (m.working) out[m.kind.startsWith("codex") ? "codex" : "claude"] = true;
  }
  return out;
}

/** Tabs to filter projects by where they run. Hidden when there is only one place. */
function tabs(s: Snapshot): string {
  const envs = s.environments.filter((e) => s.projects.some((p) => p.envId === e.id));
  if (envs.length < 2) return "";
  if (envFilter !== "all" && !envs.some((e) => e.id === envFilter)) envFilter = "all";
  const tab = (id: string, label: string, n: number) =>
    `<button type="button" class="tab${envFilter === id ? " is-on" : ""}" data-env="${esc(id)}" aria-pressed="${envFilter === id}">${esc(label)}<small>${n}</small></button>`;
  return `<div class="tabs" role="group" aria-label="Entorno">${tab("all", "Todos", s.projects.length)}${envs
    .map((e) => tab(e.id, e.name, s.projects.filter((p) => p.envId === e.id).length))
    .join("")}</div>`;
}

function projectsFor(s: Snapshot): string {
  const list = envFilter === "all" ? s.projects : s.projects.filter((p) => p.envId === envFilter);
  if (!list.length) return `<p class="empty">Nada en las últimas 24 horas.</p>`;
  const multi = new Set(s.projects.map((p) => p.envId)).size > 1;
  const envName = (p: Project) =>
    envFilter === "all" && multi ? s.environments.find((e) => e.id === p.envId)?.name : undefined;
  // "Todos" stays short: awake projects first, the rest live in their own tab.
  const shown =
    envFilter === "all"
      ? [...list].sort((a, b) => Number(a.asleep) - Number(b.asleep)).slice(0, ALL_TAB_MAX)
      : list;
  const hidden = list.length - shown.length;
  return (
    shown.map((p) => project(p, envName(p))).join("") +
    (hidden > 0 ? `<p class="more-projects">${hidden} más dormidos: elige un entorno arriba para verlos.</p>` : "")
  );
}

function render(s: Snapshot, prev: Snapshot | null) {
  const pill = (u: ProviderUsage) =>
    `<span class="pill-item pill-${u.provider}" title="${NAMES[u.provider]}: queda ${u.primary?.usedPercent == null ? "–" : 100 - Math.round(u.primary.usedPercent)} %">${fillCritter(u.provider, u.primary?.usedPercent ?? null)}<span>${pct(u.primary?.usedPercent)}<small>%</small></span></span>`;
  const busy = workingTools(s);

  root.innerHTML = `
  <header class="bar-top" data-tauri-drag-region>
    <span class="updated" data-tauri-drag-region>Actualizado ${since(s.takenAt)}</span>
    <span class="pill" data-tauri-drag-region>${pill(s.claude)}${pill(s.codex)}</span>
    <span class="controls">
      <button type="button" data-act="compact" aria-label="${compact ? "Expandir" : "Compactar"}" title="${compact ? "Expandir" : "Compactar"}">
        <svg viewBox="0 0 16 16"><path d="${compact ? "M4 6l4 4 4-4" : "M4 10l4-4 4 4"}"/></svg>
      </button>
      <button type="button" data-act="hide" aria-label="Ocultar (vuelve desde la barra de menús)" title="Ocultar">
        <svg viewBox="0 0 16 16"><path d="M4.5 4.5l7 7M11.5 4.5l-7 7"/></svg>
      </button>
    </span>
  </header>
  <div class="accounts">${account(s.claude, busy.claude)}${account(s.codex, busy.codex)}</div>
  <section class="projects" aria-label="Proyectos activos">
    <h2 class="projects-head">Proyectos</h2>
    ${tabs(s)}
    ${projectsFor(s)}
  </section>`;

  // Live usage: when a percentage rose since the last reading, bump it and float "+N %".
  for (const u of [s.claude, s.codex]) {
    const before = prev?.[u.provider].primary?.usedPercent;
    const now = u.primary?.usedPercent;
    if (before == null || now == null || Math.round(now) <= Math.round(before)) continue;
    const el = root.querySelector<HTMLElement>(`.acc-${u.provider}`);
    el?.querySelector(".num")?.classList.add("bump");
    const puff = el?.querySelector<HTMLElement>(".puff");
    if (puff) {
      puff.textContent = `+${Math.round(now) - Math.round(before)} %`;
      puff.classList.add("go");
    }
  }

  if (firstPaint) {
    root.classList.add("intro");
    // Let the arcs sweep in once, then stop animating between polls.
    requestAnimationFrame(() => requestAnimationFrame(() => root.classList.add("intro-go")));
    setTimeout(() => root.classList.remove("intro", "intro-go"), 1600);
    firstPaint = false;
  }
}

root.addEventListener("click", async (e) => {
  const tabBtn = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-env]");
  if (tabBtn) {
    envFilter = tabBtn.dataset.env ?? "all";
    try {
      localStorage.setItem(ENV_KEY, envFilter);
    } catch {
      /* private mode: the choice just lasts for this session */
    }
    if (last) render(last, null);
    return;
  }
  const btn = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-act]");
  if (!btn) return;
  if (btn.dataset.act === "hide") return hideWindow();
  compact = !compact;
  document.body.classList.toggle("compact", compact);
  if (last) render(last, null);
});

// The window hugs the panel: whatever the content needs, never a scrollbar.
new ResizeObserver(() => fitWindow(root.offsetHeight)).observe(root);

async function tick() {
  try {
    const next = await fetchSnapshot();
    render(next, last);
    last = next;
  } catch (err) {
    root.innerHTML = `<p class="fatal">No se pudo leer el uso: ${esc(String(err))}. Se reintentará en 20 s.</p>`;
  }
}

fetchPlatform().then((p) => {
  document.body.dataset.os = p.os;
  document.body.classList.toggle("native-blur", p.nativeBlur);
});
// Focus decides how solid the glass is (see `.has-focus` in styles.css).
const setFocus = () => document.body.classList.toggle("has-focus", document.hasFocus());
window.addEventListener("focus", setFocus);
window.addEventListener("blur", setFocus);
setFocus();
tick();
setInterval(tick, POLL_MS);
// Countdowns and "updated" text without re-reading data.
setInterval(() => last && !document.hidden && render(last, null), 30_000);
