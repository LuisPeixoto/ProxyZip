import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { app } from "./stores/app.svelte";

export interface AvailableUpdate {
  version: string;
  notes: string;
}

let pending: Update | null = null;

export const appVersion = () => getVersion().catch(() => "");

/** Ask GitHub for a newer release. Returns it, or null when this version is the latest (or the check failed and `quiet`). */
export async function checkForUpdate(quiet: boolean): Promise<{ update: AvailableUpdate | null; error?: string }> {
  try {
    const found = await check();
    pending = found;
    if (!found) {
      app.update = null;
      return { update: null };
    }
    const update = { version: found.version, notes: found.body ?? "" };
    app.update = update;
    return { update };
  } catch (e) {
    if (!quiet) return { update: null, error: String(e) };
    return { update: null };
  }
}

/** Download the pending update, install it and restart. */
export async function installUpdate(): Promise<void> {
  if (!pending) return;
  let total = 0;
  let received = 0;
  app.updateProgress = 0;
  try {
    await pending.downloadAndInstall((event) => {
      if (event.event === "Started") total = event.data.contentLength ?? 0;
      else if (event.event === "Progress") {
        received += event.data.chunkLength;
        app.updateProgress = total ? Math.min(100, Math.round((received / total) * 100)) : null;
      }
    });
    await relaunch();
  } catch (e) {
    app.updateProgress = undefined;
    app.toast(`Update failed: ${e}`, "err");
  }
}
