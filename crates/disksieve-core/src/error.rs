//! Error type shared by all `disksieve-core` modules.

use std::path::PathBuf;

/// Result alias used across `disksieve-core`.
pub type CoreResult<T> = Result<T, CoreError>;

/// Errors produced by the core engine.
///
/// Per-entry I/O failures during a scan (e.g. `Access Denied` on system folders) are **not**
/// surfaced as errors — they are counted in `skipped_entries`. `CoreError` is reserved for
/// failures that abort the whole operation.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// The requested scan root does not exist or is not a directory.
    #[error("scan root is not a directory: {}", .0.display())]
    InvalidRoot(PathBuf),

    /// A `NodeId` that does not exist in the current scan tree (e.g. from a stale UI request).
    #[error("node {0} does not exist in the scan tree")]
    InvalidNode(u32),

    /// The operation is not allowed on this node (scan root, already trashed, …).
    #[error("operation not allowed on node {id}: {reason}")]
    NodeNotAllowed { id: u32, reason: &'static str },

    /// The node's path contains a name that is not valid Unicode, so it cannot be rebuilt
    /// exactly. File operations on such nodes are refused instead of guessing the path.
    #[error("path of node {0} contains a non-Unicode name and cannot be resolved exactly")]
    UnrepresentablePath(u32),

    /// The operation was cancelled through its `CancelToken`.
    #[error("operation cancelled")]
    Cancelled,

    /// The scan tree hit a `u32` limit (more than ~4 billion nodes or 4 GiB of names).
    #[error("scan tree capacity exceeded")]
    TreeFull,

    /// Underlying I/O failure bound to a specific path.
    #[error("I/O error at {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl CoreError {
    /// Wraps an [`std::io::Error`] together with the path it occurred at.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;
    use std::io;

    #[test]
    fn invalid_root_message_contains_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        let msg = CoreError::InvalidRoot(missing.clone()).to_string();
        assert!(msg.contains("does-not-exist"), "{msg}");
    }

    #[test]
    fn io_error_keeps_path_and_source() {
        let err = CoreError::io(
            "/some/file",
            io::Error::from(io::ErrorKind::PermissionDenied),
        );
        assert!(err.to_string().contains("some"));
        assert!(err.source().is_some());
    }
}
