import "@fontsource-variable/unbounded";
import "@fontsource-variable/manrope";
import "./styles.css";

import { fetchPlatform, fetchSnapshot, fitWindow, hideWindow } from "./api";
import { duration, projection, since, tokens } from "./format";
import { gauge } from "./gauge";
import type { ProviderUsage, Snapshot } from "./types";

const POLL_MS = 20_000;
const NAMES = { claude: "Claude Code", codex: "Codex" } as const;

const root = document.getElementById("app")!;
let last: Snapshot | null = null;
let compact = false;
let firstPaint = true;

const esc = (s: string) =>
  s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

/** Escapes, then turns `command` spans from backend messages into <code>. */
const rich = (s: string) => esc(s).replace(/`([^`]+)`/g, "<code>$1</code>");

function pct(n: number | null | undefined): string {
  return n == null ? "–" : String(Math.round(n));
}

function paceLine(u: ProviderUsage): string {
  const p = projection(u.primary);
  if (!p) return "";
  if (p.hitsLimitIn != null) {
    return `<p class="pace pace-warn">A este ritmo, límite en ${duration(p.hitsLimitIn)}</p>`;
  }
  return `<p class="pace">A este ritmo, ${Math.round(p.atReset)} % al renovar</p>`;
}

function card(u: ProviderUsage): string {
  const name = NAMES[u.provider];
  if (u.status !== "ok" || !u.primary) {
    return `
    <section class="meter meter-${u.provider} is-empty" aria-label="${name}">
      ${gauge(u.provider, null, null)}
      <div class="readout"><span class="num">–</span></div>
      <h2 class="who">${name}</h2>
      <p class="note">${rich(u.message ?? "Sin datos.")}</p>
    </section>`;
  }
  const p = projection(u.primary);
  const reset = u.primary.resetsAt ? Date.parse(u.primary.resetsAt) - Date.now() : null;
  const week = u.secondary?.usedPercent ?? null;
  const weekReset = u.secondary?.resetsAt ? Date.parse(u.secondary.resetsAt) - Date.now() : null;
  return `
  <section class="meter meter-${u.provider}" aria-label="${name}: ${pct(u.primary.usedPercent)} % de la sesión de 5 horas">
    ${gauge(u.provider, u.primary.usedPercent, p?.atReset ?? null)}
    <div class="readout"><span class="num">${pct(u.primary.usedPercent)}</span><span class="unit">%</span></div>
    <h2 class="who">${name}${u.plan ? ` <span class="plan">${esc(u.plan)}</span>` : ""}</h2>
    <p class="reset">${reset != null ? `Se renueva en ${duration(reset)}` : "Sesión de 5 h"}</p>
    ${paceLine(u)}
    ${
      week != null
        ? `<div class="week" title="${weekReset != null ? `Se renueva en ${duration(weekReset)}` : ""}">
             <div class="week-label"><span>Semana</span><span>${pct(week)} %</span></div>
             <div class="bar"><i style="width:${Math.min(100, week)}%"></i></div>
           </div>`
        : ""
    }
    ${u.tokens5h ? `<p class="tokens">${tokens(u.tokens5h)} tokens en 5 h</p>` : ""}
  </section>`;
}

function render(s: Snapshot) {
  const pill = (u: ProviderUsage) =>
    `<span class="pill-item pill-${u.provider}"><i></i>${pct(u.primary?.usedPercent)}<small>%</small></span>`;

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
  <div class="meters">${card(s.claude)}${card(s.codex)}</div>
  <section class="procs" aria-label="Sesiones en marcha">
    <h2 class="procs-head">En marcha <span>${s.chats.length}</span></h2>
    ${s.chats.length ? "" : `<p class="empty">Ninguna sesión abierta. Lanza <code>claude</code> o <code>codex</code> en una terminal y aparecerá aquí.</p>`}
  </section>`;

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
  if (last) render(last);
});

// The window hugs the panel: whatever the content needs, never a scrollbar.
new ResizeObserver(() => fitWindow(root.offsetHeight)).observe(root);

async function tick() {
  try {
    last = await fetchSnapshot();
    render(last);
  } catch (err) {
    root.innerHTML = `<p class="fatal">No se pudo leer el uso: ${esc(String(err))}. Se reintentará en 20 s.</p>`;
  }
}

fetchPlatform().then((p) => {
  document.body.dataset.os = p.os;
  document.body.classList.toggle("native-blur", p.nativeBlur);
});
tick();
setInterval(tick, POLL_MS);
// Countdowns and "updated" text without re-reading data.
setInterval(() => last && !document.hidden && render(last), 30_000);
