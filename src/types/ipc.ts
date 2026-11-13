// IPC type contract (Rust ↔ TypeScript).
// Every DTO returned by a `#[tauri::command]` (serde `rename_all = "camelCase"`) gets an exact
// counterpart here. Filled in by roadmap step 1.3 — see `src/lib/ipc.ts` for the typed wrappers.
export {};
