// Typed wrappers around Tauri `invoke()` with a browser fallback.
//
// Inside the desktop app every call goes to Rust. In a plain browser (`npm run dev`) there is
// no Tauri runtime, so the same functions resolve with demo data — the UI code never branches.
import { Channel, invoke } from "@tauri-apps/api/core";
import type { AppInfoDto, DriveDto, ScanProgressDto, ScanSummaryDto } from "@/types/ipc";
import { DRIVE_CAPACITY, TOTAL_FILES, initialDrive } from "@/data/mockDrive";
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

type NoArgs = Record<string, never>;

// Command name → argument and result types. Names must match `#[tauri::command]` fns
// registered in `src-tauri/src/lib.rs`; result types come from the generated DTOs.
interface Commands {
  app_info: { args: NoArgs; result: AppInfoDto };
  list_drives: { args: NoArgs; result: DriveDto[] };
  pick_folder: { args: NoArgs; result: string | null };
  start_scan: { args: { path: string; onProgress: Channel<ScanProgressDto> }; result: ScanSummaryDto | null };
  cancel_scan: { args: NoArgs; result: boolean };
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
const MOCK_SCAN_MS = 800;
let mockGeneration = 0;
let cancelMockScan: (() => void) | null = null;

const mock = {
  appInfo: (): AppInfoDto => ({
    name: "DiskSieve",
    version: appVersion,
    coreVersion: appVersion,
    os: "browser",
    arch: "demo",
  }),

  drives: (): DriveDto[] => [
    { mountPoint: "C:\\", name: "Local Disk", fileSystem: "NTFS", kind: "ssd", totalBytes: DRIVE_CAPACITY, availableBytes: DRIVE_CAPACITY - initialDrive.sizeBytes, isRemovable: false },
    { mountPoint: "D:\\", name: "Archive", fileSystem: "NTFS", kind: "hdd", totalBytes: 2 * DRIVE_CAPACITY, availableBytes: 0.8 * DRIVE_CAPACITY, isRemovable: false },
  ],

  summary: (path: string, generation: number, durationMs: number): ScanSummaryDto => ({
    generation,
    rootPath: path,
    fileCount: TOTAL_FILES,
    dirCount: Math.round(TOTAL_FILES / 9),
    logicalBytes: Math.round(initialDrive.sizeBytes * 0.97),
    allocatedBytes: initialDrive.sizeBytes,
    skippedEntries: 3,
    durationMs,
  }),

  /** Simulated scan: progress every 100 ms for ~0.8 s, cancellable like the real one. */
  scan: (path: string, onProgress: (p: ScanProgressDto) => void): Promise<ScanSummaryDto | null> => {
    cancelMockScan?.();
    const generation = ++mockGeneration;
    return new Promise((resolve) => {
      const t0 = performance.now();
      let timer: ReturnType<typeof setTimeout> | undefined;
      cancelMockScan = () => {
        clearTimeout(timer);
        cancelMockScan = null;
        resolve(null);
      };
      const tick = () => {
        const f = Math.min(1, (performance.now() - t0) / MOCK_SCAN_MS);
        onProgress({
          generation,
          files: Math.round(TOTAL_FILES * f),
          dirs: Math.round((TOTAL_FILES / 9) * f),
          allocatedBytes: Math.round(initialDrive.sizeBytes * f),
          skippedEntries: f > 0.5 ? 3 : 0,
          currentDir: path,
        });
        if (f < 1) {
          timer = setTimeout(tick, 100);
        } else {
          cancelMockScan = null;
          resolve(mock.summary(path, generation, Math.round(performance.now() - t0)));
        }
      };
      tick();
    });
  },
};

/** Summary shown in the browser demo before any scan is started. */
export const demoSummary = (): ScanSummaryDto => mock.summary("C:\\", 0, 1420);

// ── Public API ─────────────────────────────────────────────────────────────────
/** Static information about the running application. */
export const appInfo = (): Promise<AppInfoDto> =>
  isTauri() ? call("app_info", {}) : Promise.resolve(mock.appInfo());

/** Mounted volumes that can be scanned. */
export const listDrives = (): Promise<DriveDto[]> =>
  isTauri() ? call("list_drives", {}) : Promise.resolve(mock.drives());

/** Native folder picker; `null` when the user cancels (always `null` in the browser demo). */
export const pickFolder = (): Promise<string | null> =>
  isTauri() ? call("pick_folder", {}) : Promise.resolve(null);

/**
 * Scans `path`, calling `onProgress` up to ~10 times per second. Resolves with the summary,
 * or `null` when the scan was cancelled or replaced by a newer one.
 */
export const startScan = (
  path: string,
  onProgress: (progress: ScanProgressDto) => void,
): Promise<ScanSummaryDto | null> => {
  if (!isTauri()) return mock.scan(path, onProgress);
  const channel = new Channel<ScanProgressDto>();
  channel.onmessage = onProgress;
  return call("start_scan", { path, onProgress: channel });
};

/** Cancels the scan in progress; `false` when nothing was running. */
export const cancelScan = (): Promise<boolean> => {
  if (isTauri()) return call("cancel_scan", {});
  const running = cancelMockScan !== null;
  cancelMockScan?.();
  return Promise.resolve(running);
};
