import "@fontsource-variable/unbounded";
import "@fontsource-variable/manrope";
import "./styles.css";

import { fetchPlatform, fetchSnapshot, fitWindow, hideWindow } from "./api";
import { bossCritter, claudeCritter, codexCritter, whip } from "./critters";
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
  const art = m.kind.startsWith("codex") ? codexCritter({ small, awake: m.working }) : claudeCritter({ small, awake: m.working });
  return `<span class="mate${m.working ? "" : " is-idle"}" title="${esc(m.name)}"><span class="sweat"></span>${art}</span>`;
}

function whoRow(provider: Provider, name: string, label: string, working: boolean, right: string): string {
  return `<div class="who-row"><span class="dot dot-${provider}${working ? " is-busy" : ""}"></span>
    <span class="name">${esc(name)} <span class="reset">· ${label}</span></span><span>${right}</span></div>`;
}

function project(p: Project): string {
  const boss = p.boss;
  const crew = p.crew.map(mate).join("");
  const more = p.moreCrew > 0 ? `<span class="crew-more">+${p.moreCrew}</span>` : "";
  const waiting = boss && !p.working && !p.crew.length ? `<span class="waiting">esperando tu respuesta</span>` : "";
  const list = [
    boss ? whoRow("claude", boss.title, "jefe", boss.working, boss.contextPercent == null ? "–" : `${Math.round(boss.contextPercent)} %`) : "",
    ...p.crew.map((m) =>
      whoRow(
        m.kind.startsWith("codex") ? "codex" : "claude",
        m.name,
        KIND_LABEL[m.kind],
        m.working,
        m.contextPercent != null ? `${Math.round(m.contextPercent)} %` : m.working ? "trabajando" : "en espera",
      ),
    ),
  ].join("");
  return `
  <section class="proj${p.working ? " is-working" : ""}${boss ? "" : " no-boss"}" tabindex="0" title="${esc(p.cwd ?? "")}">
    <div class="proj-head"><span class="proj-name">${esc(p.name || folder(p.cwd))}</span>
      ${boss ? ctx(boss.contextPercent, boss.contextApprox, "claude") : ""}</div>
    <div class="stage">
      ${boss ? `<div class="boss">${bossCritter(p.working)}${whip()}</div>` : ""}
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

function render(s: Snapshot, prev: Snapshot | null) {
  const pill = (u: ProviderUsage) =>
    `<span class="pill-item pill-${u.provider}"><i></i>${pct(u.primary?.usedPercent)}<small>%</small></span>`;
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
    <h2 class="projects-head">Proyectos activos</h2>
    ${s.projects.length ? s.projects.map(project).join("") : `<p class="empty">Ningún chat activo ahora mismo.</p>`}
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
