// Typed wrappers around Tauri `invoke()` with a browser fallback.
//
// Inside the desktop app every call goes to Rust. In a plain browser (`npm run dev`) there is
// no Tauri runtime, so the same functions resolve with demo data — the UI code never branches.
import { invoke } from "@tauri-apps/api/core";
import type { AppInfoDto } from "@/types/ipc";
import { version as appVersion } from "../../package.json";

/** True when running inside the Tauri desktop shell (not a plain browser tab). */
export const isTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** Error raised when a Rust command fails; `message` is the error string returned by Rust. */
export class IpcError extends Error {
  constructor(readonly command: string, message: string) {
    super(message);
    this.name = "IpcError";
  }
}

// Command name → argument and result types. Names must match `#[tauri::command]` fns
// registered in `src-tauri/src/lib.rs`; result types come from the generated DTOs.
interface Commands {
  app_info: { args: Record<string, never>; result: AppInfoDto };
}

async function call<K extends keyof Commands>(
  command: K,
  args: Commands[K]["args"],
): Promise<Commands[K]["result"]> {
  try {
    return await invoke<Commands[K]["result"]>(command, args);
  } catch (e) {
    throw new IpcError(command, typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
  }
}

// ── Browser fallback (demo data) ───────────────────────────────────────────────
const mock = {
  appInfo: (): AppInfoDto => ({
    name: "DiskSieve",
    version: appVersion,
    coreVersion: appVersion,
    os: "browser",
    arch: "demo",
  }),
};

// ── Public API ─────────────────────────────────────────────────────────────────
/** Static information about the running application. */
export const appInfo = (): Promise<AppInfoDto> =>
  isTauri() ? call("app_info", {}) : Promise.resolve(mock.appInfo());
