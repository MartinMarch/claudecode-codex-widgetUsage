import type { PlatformInfo, Snapshot } from "./types";
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
