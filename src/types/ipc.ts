// IPC type contract (Rust ↔ TypeScript).
// DTO types are generated from Rust by ts-rs into ./generated — never edit them by hand.
// Regenerate after changing `src-tauri/src/dto.rs`: `npm run gen:types`.
export type { AppInfoDto } from "./generated/AppInfoDto";
export type { DriveDto } from "./generated/DriveDto";
export type { ScanProgressDto } from "./generated/ScanProgressDto";
export type { ScanSummaryDto } from "./generated/ScanSummaryDto";
