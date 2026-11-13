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
