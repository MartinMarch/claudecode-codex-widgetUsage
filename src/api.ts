import type { LogEntry, PlatformInfo, Snapshot } from "./types";
import { mockSnapshot } from "./mock";

export const inTauri = "__TAURI_INTERNALS__" in window;

export async function fetchSnapshot(): Promise<Snapshot> {
  if (!inTauri) return mockSnapshot();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<Snapshot>("snapshot");
}

export async function fetchPlatform(): Promise<PlatformInfo> {
  if (!inTauri) return { os: "browser", nativeBlur: false };
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<PlatformInfo>("platform_info");
}

let lastHeight = 0;
export async function fitWindow(height: number): Promise<void> {
  const h = Math.ceil(height);
  if (!inTauri || h === lastHeight || h === 0) return;
  lastHeight = h;
  const { getCurrentWindow, LogicalSize } = await import("@tauri-apps/api/window");
  await getCurrentWindow().setSize(new LogicalSize(340, h));
}

export async function hideWindow(): Promise<void> {
  if (!inTauri) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().hide();
}

/** Skips every cache except active rate-limit waits; the next snapshot is fresh. */
export async function refreshNow(): Promise<void> {
  if (!inTauri) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("refresh");
}

export async function fetchLogs(): Promise<LogEntry[]> {
  if (!inTauri) {
    const ago = (s: number) => new Date(Date.now() - s * 1000).toISOString();
    return [
      { at: ago(5), source: "App", level: "ok", message: "Actualización manual", count: 1 },
      { at: ago(8), source: "Codex", level: "ok", message: "Uso leído: 22 % (5 h), 9 % (semana)", count: 14 },
      { at: ago(20), source: "Claude", level: "warn", message: "Anthropic limita las consultas (429). Próximo intento en 4 min", count: 2 },
      { at: ago(65), source: "Nube", level: "ok", message: "4 sesiones en la nube", count: 9 },
      { at: ago(70), source: "SSH root@192.168.1.10", level: "error", message: "No se pudo entrar por SSH. ¿Está autorizada tu clave?", count: 6 },
    ];
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<LogEntry[]>("logs");
}
