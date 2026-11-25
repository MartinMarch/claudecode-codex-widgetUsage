import type { LimitWindow } from "./types";

export function duration(ms: number): string {
  const min = Math.max(0, Math.round(ms / 60_000));
  if (min < 1) return "menos de 1 min";
  if (min < 60) return `${min} min`;
  const h = Math.floor(min / 60);
  const m = min % 60;
  if (h < 24) return m ? `${h} h ${m} min` : `${h} h`;
  const d = Math.floor(h / 24);
  const rh = h % 24;
  return rh ? `${d} d ${rh} h` : `${d} d`;
}

export function tokens(n: number): string {
  if (n >= 1e6) return `${(n / 1e6).toLocaleString("es", { maximumFractionDigits: 1 })} M`;
  if (n >= 1e3) return `${Math.round(n / 1e3)} k`;
  return String(n);
}

export function since(iso: string | null): string {
  if (!iso) return "";
  const s = Math.round((Date.now() - Date.parse(iso)) / 1000);
  if (s < 10) return "ahora";
  if (s < 60) return `hace ${s} s`;
  return `hace ${duration(s * 1000)}`;
}

export function folder(cwd: string | null): string {
  if (!cwd) return "carpeta desconocida";
  const parts = cwd.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? cwd;
}

/**
 * Where usage lands at reset if the current pace holds.
 * Returns null when there isn't enough elapsed time to estimate.
 */
export function projection(w: LimitWindow | null): { atReset: number; hitsLimitIn: number | null } | null {
  if (!w || w.usedPercent == null || !w.resetsAt) return null;
  const total = w.windowMinutes * 60_000;
  const remaining = Math.max(0, Date.parse(w.resetsAt) - Date.now());
  const elapsed = total - remaining;
  if (elapsed < total * 0.05 || w.usedPercent <= 0) return null;
  const rate = w.usedPercent / elapsed; // percent per ms
  const atReset = w.usedPercent + rate * remaining;
  const hitsLimitIn = atReset > 100 ? (100 - w.usedPercent) / rate : null;
  return { atReset, hitsLimitIn };
}
