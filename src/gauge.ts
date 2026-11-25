import type { Provider } from "./types";

const START = 150; // degrees, measured clockwise from 3 o'clock
const SWEEP = 240;
const R = 54;
const C = 70; // centre of the 140×140 viewBox

function point(pct: number, r = R): [number, number] {
  const a = ((START + (SWEEP * Math.min(100, Math.max(0, pct))) / 100) * Math.PI) / 180;
  return [C + r * Math.cos(a), C + r * Math.sin(a)];
}

function arc(from: number, to: number, r = R): string {
  const [x1, y1] = point(from, r);
  const [x2, y2] = point(to, r);
  const large = (SWEEP * (to - from)) / 100 > 180 ? 1 : 0;
  return `M ${x1.toFixed(2)} ${y1.toFixed(2)} A ${r} ${r} 0 ${large} 1 ${x2.toFixed(2)} ${y2.toFixed(2)}`;
}

const FULL = arc(0, 100);

/** Tick marks every 10 %, longer at 50 and 100. */
const TICKS = Array.from({ length: 11 }, (_, i) => {
  const p = i * 10;
  const long = p % 50 === 0;
  const [x1, y1] = point(p, R + 9);
  const [x2, y2] = point(p, R + (long ? 14 : 12));
  return `<line x1="${x1.toFixed(2)}" y1="${y1.toFixed(2)}" x2="${x2.toFixed(2)}" y2="${y2.toFixed(2)}" class="tick${long ? " tick-long" : ""}"/>`;
}).join("");

export function gauge(provider: Provider, used: number | null, projected: number | null): string {
  const v = used == null ? 0 : Math.min(100, Math.max(0, used));
  const hot = v >= 85 || (projected != null && projected > 100);
  const ghost =
    projected != null && projected > v + 1
      ? `<path d="${arc(v, Math.min(100, projected))}" class="ghost"/>
         <circle cx="${point(Math.min(100, projected))[0].toFixed(2)}" cy="${point(Math.min(100, projected))[1].toFixed(2)}" r="4.5" class="ghost-dot"/>`
      : "";
  return `
  <svg viewBox="0 0 140 128" class="gauge gauge-${provider}${hot ? " is-hot" : ""}" aria-hidden="true">
    <defs>
      <linearGradient id="g-${provider}" x1="0" y1="1" x2="1" y2="0">
        <stop offset="0" class="stop-a"/><stop offset="1" class="stop-b"/>
      </linearGradient>
    </defs>
    <g class="ticks">${TICKS}</g>
    <path d="${FULL}" class="track"/>
    ${ghost}
    <path d="${FULL}" pathLength="100" class="fill" stroke="url(#g-${provider})"
          style="--v:${v.toFixed(1)}"/>
  </svg>`;
}
