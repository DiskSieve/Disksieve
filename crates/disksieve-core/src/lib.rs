//! # disksieve-core
//!
//! Pure Rust engine behind DiskSieve: filesystem scanning, the arena-based scan tree,
//! treemap viewport projection, dev-junk detection, duplicate hashing and snapshots.
//!
//! This crate has **no dependency on Tauri or any UI layer** — everything here is testable
//! with `cargo test -p disksieve-core`.

pub mod error;

pub use error::{CoreError, CoreResult};

/// Version of the core engine (taken from the workspace manifest).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_semver() {
        assert_eq!(VERSION.split('.').count(), 3, "{VERSION}");
    }
}
