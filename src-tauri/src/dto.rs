//! IPC data transfer objects — the only types that cross the Rust ↔ WebView boundary.
//!
//! Every DTO derives `TS` and is exported to `src/types/generated/` (see `.cargo/config.toml`).
//! Regenerate the TypeScript bindings with `npm run gen:types`; never edit them by hand.
//! Rules: `#[serde(rename_all = "camelCase")]`, `#[ts(export)]`, and `#[ts(type = "number")]`
//! on 64-bit integers (values stay below 2^53).

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_serializes_to_camel_case() {
        let json = serde_json::to_value(AppInfoDto::current()).unwrap();
        assert!(json.get("coreVersion").is_some(), "{json}");
        assert!(json.get("core_version").is_none(), "{json}");
    }
}
