//! IPC data transfer objects — the only types that cross the Rust ↔ WebView boundary.
//!
//! Every DTO derives `TS` and is exported to `src/types/generated/` (see `.cargo/config.toml`).
//! Regenerate the TypeScript bindings with `npm run gen:types`; never edit them by hand.
//! Rules: `#[serde(rename_all = "camelCase")]`, `#[ts(export)]`, and `#[ts(type = "number")]`
//! on 64-bit integers (values stay below 2^53).

use disksieve_core::{NodeFlags, ScanProgress, ScanTree, ROOT};
use serde::Serialize;
use ts_rs::TS;

/// Static information about the running application, shown in the header and About dialog.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppInfoDto {
    /// Product name, e.g. `DiskSieve`.
    pub name: String,
    /// Desktop app version (from the workspace manifest).
    pub version: String,
    /// `disksieve-core` engine version.
    pub core_version: String,
    /// Operating system family: `windows`, `linux` or `macos`.
    pub os: String,
    /// CPU architecture, e.g. `x86_64` or `aarch64`.
    pub arch: String,
}

impl AppInfoDto {
    pub fn current() -> Self {
        Self {
            name: "DiskSieve".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            core_version: disksieve_core::VERSION.into(),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
        }
    }
}

/// A mounted volume that can be scanned.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DriveDto {
    /// Scan root, e.g. `C:\` or `/`.
    pub mount_point: String,
    /// Volume label (may be empty).
    pub name: String,
    /// File system, e.g. `NTFS`, `apfs`, `ext4`.
    pub file_system: String,
    /// `ssd`, `hdd` or `unknown`.
    pub kind: String,
    #[ts(type = "number")]
    pub total_bytes: u64,
    #[ts(type = "number")]
    pub available_bytes: u64,
    pub is_removable: bool,
}

/// Progress of a running scan, streamed through a `Channel` at most ~10 times per second.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScanProgressDto {
    #[ts(type = "number")]
    pub generation: u64,
    #[ts(type = "number")]
    pub files: u64,
    #[ts(type = "number")]
    pub dirs: u64,
    /// Size on disk found so far.
    #[ts(type = "number")]
    pub allocated_bytes: u64,
    #[ts(type = "number")]
    pub skipped_entries: u64,
    /// Directory processed most recently (display only).
    pub current_dir: String,
}

impl ScanProgressDto {
    pub fn new(generation: u64, p: &ScanProgress) -> Self {
        Self {
            generation,
            files: p.files,
            dirs: p.dirs,
            allocated_bytes: p.allocated_bytes,
            skipped_entries: p.skipped_entries,
            current_dir: p.current_dir.to_string_lossy().into_owned(),
        }
    }
}

/// Result of a completed scan. The tree itself stays in Rust.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScanSummaryDto {
    #[ts(type = "number")]
    pub generation: u64,
    pub root_path: String,
    pub file_count: u32,
    pub dir_count: u32,
    #[ts(type = "number")]
    pub logical_bytes: u64,
    #[ts(type = "number")]
    pub allocated_bytes: u64,
    #[ts(type = "number")]
    pub skipped_entries: u64,
    #[ts(type = "number")]
    pub duration_ms: u64,
}

impl ScanSummaryDto {
    pub fn from_tree(tree: &ScanTree) -> Self {
        let (file_count, logical_bytes, allocated_bytes) = tree.get(ROOT).map_or((0, 0, 0), |n| {
            (n.file_count, n.logical_size, n.allocated_size)
        });
        let dir_count = (0..tree.len())
            .filter_map(|i| tree.get(i as u32))
            .filter(|n| n.flags.contains(NodeFlags::DIR))
            .count();
        Self {
            generation: tree.generation(),
            root_path: tree.root_path().to_string_lossy().into_owned(),
            file_count,
            dir_count: u32::try_from(dir_count).unwrap_or(u32::MAX),
            logical_bytes,
            allocated_bytes,
            skipped_entries: tree.stats().skipped_entries,
            duration_ms: u64::try_from(tree.stats().duration.as_millis()).unwrap_or(u64::MAX),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_summary_counts_files_dirs_and_sizes() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();
        std::fs::write(tmp.path().join("sub").join("a.bin"), [0u8; 1000]).unwrap();
        std::fs::write(tmp.path().join("b.bin"), [0u8; 24]).unwrap();
        let opts = disksieve_core::ScanOptions {
            generation: 5,
            ..Default::default()
        };
        let tree = disksieve_core::scan(
            tmp.path(),
            &opts,
            &disksieve_core::CancelToken::new(),
            |_| {},
        )
        .unwrap();
        let summary = ScanSummaryDto::from_tree(&tree);
        assert_eq!(summary.generation, 5);
        assert_eq!(summary.file_count, 2);
        assert_eq!(summary.dir_count, 2, "root + sub");
        assert_eq!(summary.logical_bytes, 1024);
        assert!(summary.allocated_bytes >= 1024);

        let json = serde_json::to_value(&summary).unwrap();
        assert!(json.get("rootPath").is_some(), "{json}");
        assert!(json.get("durationMs").is_some(), "{json}");
    }

    #[test]
    fn app_info_serializes_to_camel_case() {
        let json = serde_json::to_value(AppInfoDto::current()).unwrap();
        assert!(json.get("coreVersion").is_some(), "{json}");
        assert!(json.get("core_version").is_none(), "{json}");
    }
}
