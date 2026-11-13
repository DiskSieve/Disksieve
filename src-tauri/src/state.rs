//! Shared application state: the last completed scan tree and the scan in progress.
//!
//! Only one scan runs at a time. Starting a new scan cancels the previous one; a result is
//! accepted only if it belongs to the newest scan (`generation`), so a slow, superseded scan
//! can never overwrite a newer tree.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use disksieve_core::{CancelToken, ScanTree};

/// Handle of the scan currently running.
#[derive(Debug, Clone)]
pub struct ScanHandle {
    pub generation: u64,
    pub cancel: CancelToken,
}

/// State managed by Tauri (`app.manage(AppState::default())`).
#[derive(Debug, Default)]
pub struct AppState {
    tree: RwLock<Option<ScanTree>>,
    scan: Mutex<Option<ScanHandle>>,
    last_generation: AtomicU64,
}

impl AppState {
    /// Registers a new scan, cancelling the one in progress (if any).
    pub fn begin_scan(&self) -> ScanHandle {
        let generation = self.last_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let handle = ScanHandle {
            generation,
            cancel: CancelToken::new(),
        };
        if let Some(previous) = self.scan_slot().replace(handle.clone()) {
            previous.cancel.cancel();
        }
        handle
    }

    /// Ends scan `generation`. Stores `tree` and returns `true` only if this is still the
    /// newest scan; a superseded scan's result is dropped.
    pub fn finish_scan(&self, generation: u64, tree: Option<ScanTree>) -> bool {
        let mut slot = self.scan_slot();
        if slot.as_ref().map(|h| h.generation) != Some(generation) {
            return false;
        }
        *slot = None;
        match tree {
            Some(tree) => {
                // Lock order: scan slot, then tree — the same everywhere.
                *self.tree_mut() = Some(tree);
                true
            }
            None => false,
        }
    }

    /// Cancels the scan in progress. Returns `false` when no scan is running.
    pub fn cancel_scan(&self) -> bool {
        match self.scan_slot().as_ref() {
            Some(handle) => {
                handle.cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Read access to the last completed scan tree.
    pub fn tree(&self) -> RwLockReadGuard<'_, Option<ScanTree>> {
        self.tree.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn tree_mut(&self) -> RwLockWriteGuard<'_, Option<ScanTree>> {
        self.tree.write().unwrap_or_else(PoisonError::into_inner)
    }

    fn scan_slot(&self) -> MutexGuard<'_, Option<ScanHandle>> {
        // A panic while holding the lock cannot leave the handle half-written, so the data
        // is still consistent and the poison flag can be ignored.
        self.scan.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use disksieve_core::{scan, ScanOptions};

    fn scan_dir(dir: &std::path::Path, generation: u64) -> ScanTree {
        let opts = ScanOptions {
            generation,
            ..ScanOptions::default()
        };
        scan(dir, &opts, &CancelToken::new(), |_| {}).unwrap()
    }

    #[test]
    fn generations_increase_and_new_scan_cancels_previous() {
        let state = AppState::default();
        let first = state.begin_scan();
        let second = state.begin_scan();
        assert_eq!(first.generation, 1);
        assert_eq!(second.generation, 2);
        assert!(first.cancel.is_cancelled());
        assert!(!second.cancel.is_cancelled());
    }

    #[test]
    fn newest_scan_result_is_stored() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), b"hello").unwrap();
        let state = AppState::default();
        let handle = state.begin_scan();
        assert!(state.finish_scan(
            handle.generation,
            Some(scan_dir(tmp.path(), handle.generation))
        ));
        let tree = state.tree();
        assert_eq!(tree.as_ref().unwrap().generation(), handle.generation);
        drop(tree);
        assert!(!state.cancel_scan(), "nothing running after finish");
    }

    #[test]
    fn superseded_scan_result_is_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        let state = AppState::default();
        let old = state.begin_scan();
        let new = state.begin_scan();
        assert!(!state.finish_scan(old.generation, Some(scan_dir(tmp.path(), old.generation))));
        assert!(state.tree().is_none());
        assert!(state.cancel_scan(), "the newer scan is still running");
        assert!(state.finish_scan(new.generation, Some(scan_dir(tmp.path(), new.generation))));
        assert_eq!(state.tree().as_ref().unwrap().generation(), new.generation);
    }

    #[test]
    fn cancelled_scan_keeps_previous_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let state = AppState::default();
        let first = state.begin_scan();
        assert!(state.finish_scan(
            first.generation,
            Some(scan_dir(tmp.path(), first.generation))
        ));

        let second = state.begin_scan();
        assert!(state.cancel_scan());
        assert!(second.cancel.is_cancelled());
        assert!(!state.finish_scan(second.generation, None));
        assert_eq!(
            state.tree().as_ref().unwrap().generation(),
            first.generation
        );
        assert!(!state.cancel_scan());
    }
}
