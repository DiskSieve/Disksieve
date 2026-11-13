//! Parallel filesystem scanner.
//!
//! A `rayon` pool reads directories with `std::fs::read_dir`; one coordinating thread (the
//! caller's) owns the [`TreeBuilder`], pushes every listing into the arena and hands each new
//! subdirectory back to the pool. Metadata comes from `DirEntry::metadata()`, which on
//! Windows is served from the directory enumeration itself (no file is opened) and on Unix
//! costs one `lstat`.
//!
//! Rules (spec 2.2):
//! - links are never followed (symlinks, junctions, mount points → `REPARSE`);
//! - the scan stays on the root's filesystem (Unix `st_dev`; other volumes → `OTHER_VOLUME`);
//! - both logical size and size on disk are recorded; Unix hard links are counted once;
//! - online-only cloud files are recognised by attributes and never opened;
//! - an unreadable entry never aborts the scan — it is counted in `skipped_entries`;
//! - progress is reported about every 100 ms and the scan can be cancelled at any time.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use crate::error::{CoreError, CoreResult};
use crate::tree::{to_unix_secs, EntryMeta, NodeFlags, NodeId, ScanTree, TreeBuilder, ROOT};

/// Shared cancellation flag. Cloning is cheap; all clones observe the same state.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation of every operation holding a clone of this token.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Scan settings.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Generation number stored in the resulting tree.
    pub generation: u64,
    /// Minimum time between two progress callbacks.
    pub progress_interval: Duration,
    /// Worker threads; 0 = one per logical CPU.
    pub threads: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            generation: 0,
            progress_interval: Duration::from_millis(100),
            threads: 0,
        }
    }
}

/// Snapshot passed to the progress callback.
#[derive(Debug, Clone, Default)]
pub struct ScanProgress {
    /// Files found so far (including links and cloud placeholders).
    pub files: u64,
    /// Directories read so far.
    pub dirs: u64,
    /// Size on disk found so far (hard-link repeats excluded).
    pub allocated_bytes: u64,
    /// Entries that could not be read.
    pub skipped_entries: u64,
    /// Directory processed most recently.
    pub current_dir: PathBuf,
}

/// Scans `root` and returns the finished tree.
///
/// `on_progress` runs on the calling thread at most every `opts.progress_interval`, plus once
/// at the end. Fails with [`CoreError::InvalidRoot`] if `root` is not a readable directory and
/// with [`CoreError::Cancelled`] if `cancel` fires.
pub fn scan(
    root: impl AsRef<Path>,
    opts: &ScanOptions,
    cancel: &CancelToken,
    mut on_progress: impl FnMut(&ScanProgress),
) -> CoreResult<ScanTree> {
    let started = Instant::now();
    let root = root.as_ref();
    let root_meta = fs::metadata(root).map_err(|_| CoreError::InvalidRoot(root.to_path_buf()))?;
    if !root_meta.is_dir() {
        return Err(CoreError::InvalidRoot(root.to_path_buf()));
    }
    let volume = Arc::new(Volume::of(root, &root_meta));
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(opts.threads)
        .thread_name(|i| format!("disksieve-scan-{i}"))
        .build()
        .map_err(|e| CoreError::io(root, io::Error::other(e)))?;

    let mut builder = TreeBuilder::new(
        root,
        opts.generation,
        EntryMeta {
            flags: NodeFlags::DIR,
            ..EntryMeta::default()
        },
    );
    let (tx, rx) = mpsc::channel::<Listing>();
    let spawn = |dir: NodeId, path: PathBuf| {
        let (tx, volume, cancel) = (tx.clone(), Arc::clone(&volume), cancel.clone());
        pool.spawn(move || send_listing(&tx, dir, path, &volume, &cancel));
    };

    spawn(ROOT, root.to_path_buf());
    let mut pending: usize = 1;
    let mut hard_links: HashSet<(u64, u64)> = HashSet::new();
    let mut progress = ScanProgress::default();
    let mut last_report = Instant::now();

    while pending > 0 {
        if cancel.is_cancelled() {
            // Workers see the same token and stop on their own; their sends just fail.
            return Err(CoreError::Cancelled);
        }
        match rx.recv_timeout(opts.progress_interval) {
            Ok(listing) => {
                pending -= 1;
                let Listing {
                    dir,
                    path,
                    outcome,
                    entry_errors,
                } = listing;
                progress.dirs += 1;
                match outcome {
                    Err(err) if dir == ROOT => return Err(CoreError::io(path, err)),
                    Err(_) => {
                        builder.add_flags(dir, NodeFlags::ACCESS_DENIED)?;
                        builder.record_skipped(path.clone());
                        progress.skipped_entries += 1;
                    }
                    Ok(entries) => {
                        for entry in entries {
                            let mut meta = entry.meta;
                            if let Some(key) = entry.link_key {
                                if !hard_links.insert(key) {
                                    meta.flags |= NodeFlags::HARDLINK;
                                }
                            }
                            let id = builder.push(dir, &entry.name, meta)?;
                            if !meta.flags.contains(NodeFlags::DIR) {
                                progress.files += 1;
                                if !meta.flags.contains(NodeFlags::HARDLINK) {
                                    progress.allocated_bytes += meta.allocated_size;
                                }
                            }
                            if entry.descend {
                                pending += 1;
                                spawn(id, path.join(&entry.name));
                            }
                        }
                    }
                }
                for skipped in entry_errors {
                    builder.record_skipped(skipped);
                    progress.skipped_entries += 1;
                }
                progress.current_dir = path;
            }
            Err(RecvTimeoutError::Timeout) => {}
            // Unreachable while `tx` is alive; kept so a logic error cannot spin forever.
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if last_report.elapsed() >= opts.progress_interval {
            on_progress(&progress);
            last_report = Instant::now();
        }
    }

    on_progress(&progress);
    Ok(builder.finish(started.elapsed()))
}

/// Result of reading one directory, sent from a worker to the coordinator.
struct Listing {
    dir: NodeId,
    path: PathBuf,
    /// `Err` when the directory itself could not be opened.
    outcome: io::Result<Vec<Entry>>,
    /// Entries inside the directory that could not be read.
    entry_errors: Vec<PathBuf>,
}

struct Entry {
    name: OsString,
    meta: EntryMeta,
    /// Same-volume, non-link directory that should be read next.
    descend: bool,
    /// `(device, inode)` for files with more than one hard link (Unix only).
    link_key: Option<(u64, u64)>,
}

/// Properties of the scanned volume shared with workers.
struct Volume {
    #[cfg(unix)]
    dev: u64,
    #[cfg(windows)]
    cluster: u64,
}

impl Volume {
    #[cfg(unix)]
    fn of(_root: &Path, meta: &fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self { dev: meta.dev() }
    }

    #[cfg(windows)]
    fn of(root: &Path, _meta: &fs::Metadata) -> Self {
        // NTFS default; only used if the volume cannot be queried.
        let cluster = fs4::allocation_granularity(root).unwrap_or(4096).max(1);
        Self { cluster }
    }

    #[cfg(not(any(unix, windows)))]
    fn of(_root: &Path, _meta: &fs::Metadata) -> Self {
        Self {}
    }
}

/// Worker body: reads one directory and always sends exactly one [`Listing`] back, even if
/// reading panics — otherwise the coordinator would wait forever.
fn send_listing(
    tx: &Sender<Listing>,
    dir: NodeId,
    path: PathBuf,
    volume: &Volume,
    cancel: &CancelToken,
) {
    let listing = panic::catch_unwind(AssertUnwindSafe(|| {
        read_listing(dir, path.clone(), volume, cancel)
    }))
    .unwrap_or_else(|_| Listing {
        dir,
        path,
        outcome: Err(io::Error::other("scanner worker panicked")),
        entry_errors: Vec::new(),
    });
    // The coordinator is gone only after cancellation or an error; nothing left to do.
    let _ = tx.send(listing);
}

fn read_listing(dir: NodeId, path: PathBuf, volume: &Volume, cancel: &CancelToken) -> Listing {
    let mut entries = Vec::new();
    let mut entry_errors = Vec::new();
    let outcome = match fs::read_dir(&path) {
        Err(err) => Err(err),
        Ok(read_dir) => {
            for item in read_dir {
                if cancel.is_cancelled() {
                    break;
                }
                match item {
                    Ok(dir_entry) => match classify(&path, &dir_entry, volume) {
                        Ok(entry) => entries.push(entry),
                        Err(_) => entry_errors.push(dir_entry.path()),
                    },
                    Err(_) => entry_errors.push(path.clone()),
                }
            }
            Ok(entries)
        }
    };
    Listing {
        dir,
        path,
        outcome,
        entry_errors,
    }
}

fn mtime_of(meta: &fs::Metadata) -> u32 {
    to_unix_secs(meta.modified().unwrap_or(UNIX_EPOCH))
}

#[cfg(unix)]
fn classify(_dir: &Path, entry: &fs::DirEntry, volume: &Volume) -> io::Result<Entry> {
    use std::os::unix::fs::MetadataExt;

    let file_type = entry.file_type()?;
    let meta = entry.metadata()?; // lstat: never follows the link
    let mut flags = NodeFlags::empty();
    let mut descend = false;
    let mut logical = meta.len();
    let mut allocated = meta.blocks() * 512;
    let mut link_key = None;

    if file_type.is_symlink() {
        flags |= NodeFlags::REPARSE;
    } else if file_type.is_dir() {
        flags |= NodeFlags::DIR;
        // A directory's own logical "size" is filesystem bookkeeping, not user data.
        logical = 0;
        if meta.dev() == volume.dev {
            descend = true;
        } else {
            flags |= NodeFlags::OTHER_VOLUME;
            allocated = 0;
        }
    } else if meta.nlink() > 1 {
        link_key = Some((meta.dev(), meta.ino()));
    }

    #[cfg(target_os = "macos")]
    {
        // iCloud "Optimize Mac Storage": content not present locally.
        const SF_DATALESS: u32 = 0x4000_0000;
        if std::os::macos::fs::MetadataExt::st_flags(&meta) & SF_DATALESS != 0 {
            flags |= NodeFlags::CLOUD_ONLINE;
            descend = false;
        }
    }

    Ok(Entry {
        name: entry.file_name(),
        meta: EntryMeta {
            logical_size: logical,
            allocated_size: allocated,
            mtime: mtime_of(&meta),
            flags,
        },
        descend,
        link_key,
    })
}

#[cfg(windows)]
fn classify(dir: &Path, entry: &fs::DirEntry, volume: &Volume) -> io::Result<Entry> {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x200;
    const FILE_ATTRIBUTE_COMPRESSED: u32 = 0x800;
    const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
    const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x4_0000;
    const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

    // Both come from the FindNextFileW data — no handle is opened.
    let file_type = entry.file_type()?;
    let meta = entry.metadata()?;
    let attrs = meta.file_attributes();
    let mut flags = NodeFlags::empty();
    let mut descend = false;
    let mut logical = 0;
    let mut allocated = 0;

    if file_type.is_symlink() {
        // Symlinks, junctions and volume mount points (name-surrogate reparse tags).
        // Cloud placeholders use other tags and are handled below.
        flags |= NodeFlags::REPARSE;
    } else if file_type.is_dir() {
        flags |= NodeFlags::DIR;
        if attrs & FILE_ATTRIBUTE_RECALL_ON_OPEN != 0 {
            // Not yet synced cloud folder: listing it would fetch from the network.
            flags |= NodeFlags::CLOUD_ONLINE;
        } else {
            descend = true;
        }
    } else {
        logical = meta.len();
        if attrs & (FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) != 0 {
            flags |= NodeFlags::CLOUD_ONLINE;
        } else if attrs & (FILE_ATTRIBUTE_COMPRESSED | FILE_ATTRIBUTE_SPARSE_FILE) != 0 {
            allocated = filesize::file_real_size(dir.join(entry.file_name()))
                .unwrap_or_else(|_| round_up(logical, volume.cluster));
        } else {
            allocated = round_up(logical, volume.cluster);
        }
    }

    Ok(Entry {
        name: entry.file_name(),
        meta: EntryMeta {
            logical_size: logical,
            allocated_size: allocated,
            mtime: mtime_of(&meta),
            flags,
        },
        descend,
        link_key: None,
    })
}

#[cfg(not(any(unix, windows)))]
fn classify(_dir: &Path, entry: &fs::DirEntry, _volume: &Volume) -> io::Result<Entry> {
    let file_type = entry.file_type()?;
    let meta = entry.metadata()?;
    let is_dir = file_type.is_dir();
    let mut flags = NodeFlags::empty();
    if file_type.is_symlink() {
        flags |= NodeFlags::REPARSE;
    } else if is_dir {
        flags |= NodeFlags::DIR;
    }
    let logical = if is_dir { 0 } else { meta.len() };
    Ok(Entry {
        name: entry.file_name(),
        meta: EntryMeta {
            logical_size: logical,
            allocated_size: logical,
            mtime: mtime_of(&meta),
            flags,
        },
        descend: is_dir && !file_type.is_symlink(),
        link_key: None,
    })
}

/// Size on disk of a plain file: whole clusters. Small files stored inside the MFT are
/// slightly overestimated; the MFT engine (step 7.1) reports them exactly.
#[cfg(windows)]
fn round_up(len: u64, cluster: u64) -> u64 {
    len.div_ceil(cluster).saturating_mul(cluster)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write as _;

    fn write_file(path: &Path, len: usize) {
        let mut f = File::create(path).unwrap();
        f.write_all(&vec![0xAB; len]).unwrap();
    }

    fn run(root: &Path) -> ScanTree {
        scan(root, &ScanOptions::default(), &CancelToken::new(), |_| {}).unwrap()
    }

    #[test]
    fn scans_nested_tree_with_exact_logical_sizes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::create_dir(root.join("empty")).unwrap();
        write_file(&root.join("top.bin"), 1_000);
        write_file(&root.join("a/one.txt"), 2_000);
        write_file(&root.join("a/b/two.rs"), 30_000);
        write_file(&root.join(".hidden"), 7);

        let tree = run(root);
        let r = tree.node(ROOT).unwrap();
        assert_eq!(r.logical_size, 33_007);
        assert_eq!(r.file_count, 4);
        assert_eq!(tree.len(), 1 + 4 + 3); // root + files + a, a/b, empty

        let a = tree.child_by_name(ROOT, "a").unwrap();
        let b = tree.child_by_name(a, "b").unwrap();
        assert!(tree.node(b).unwrap().is_dir());
        assert_eq!(tree.node(a).unwrap().logical_size, 32_000);
        let two = tree.child_by_name(b, "two.rs").unwrap();
        assert_eq!(tree.extension(two), "rs");
        assert_eq!(
            tree.resolve_path(two).unwrap(),
            root.join("a").join("b").join("two.rs")
        );
        assert!(tree.child_by_name(ROOT, ".hidden").is_some());
        assert_eq!(tree.stats().skipped_entries, 0);
    }

    #[test]
    fn allocated_size_is_reported_for_regular_files() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(&tmp.path().join("data.bin"), 10_000);
        let tree = run(tmp.path());
        let f = tree.child_by_name(ROOT, "data.bin").unwrap();
        let n = tree.node(f).unwrap();
        assert_eq!(n.logical_size, 10_000);
        assert!(n.allocated_size > 0, "{n:?}");
        #[cfg(windows)]
        assert_eq!(n.allocated_size % 512, 0);
    }

    #[test]
    fn many_directories_in_parallel_add_up() {
        let tmp = tempfile::tempdir().unwrap();
        for d in 0..40 {
            let dir = tmp.path().join(format!("d{d:02}"));
            fs::create_dir(&dir).unwrap();
            for f in 0..25 {
                write_file(&dir.join(format!("f{f:02}")), 100);
            }
        }
        let opts = ScanOptions {
            threads: 4,
            generation: 42,
            ..ScanOptions::default()
        };
        let tree = scan(tmp.path(), &opts, &CancelToken::new(), |_| {}).unwrap();
        assert_eq!(tree.generation(), 42);
        let r = tree.node(ROOT).unwrap();
        assert_eq!(r.file_count, 1_000);
        assert_eq!(r.logical_size, 100_000);
        assert_eq!(tree.children(ROOT).count(), 40);
    }

    #[test]
    fn empty_root_scans_to_a_single_node() {
        let tmp = tempfile::tempdir().unwrap();
        let tree = run(tmp.path());
        assert_eq!(tree.len(), 1);
        assert_eq!(tree.node(ROOT).unwrap().file_count, 0);
    }

    #[test]
    fn invalid_roots_are_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("missing");
        let file = tmp.path().join("file.txt");
        write_file(&file, 1);
        for root in [missing, file] {
            let err = scan(&root, &ScanOptions::default(), &CancelToken::new(), |_| {});
            assert!(matches!(err, Err(CoreError::InvalidRoot(_))), "{root:?}");
        }
    }

    #[test]
    fn cancelled_token_stops_the_scan() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("sub")).unwrap();
        let cancel = CancelToken::new();
        cancel.cancel();
        let res = scan(tmp.path(), &ScanOptions::default(), &cancel, |_| {});
        assert!(matches!(res, Err(CoreError::Cancelled)));
    }

    #[test]
    fn cancelling_from_the_progress_callback_stops_the_scan() {
        let tmp = tempfile::tempdir().unwrap();
        for d in 0..20 {
            fs::create_dir(tmp.path().join(format!("d{d}"))).unwrap();
        }
        let cancel = CancelToken::new();
        let opts = ScanOptions {
            progress_interval: Duration::ZERO,
            ..ScanOptions::default()
        };
        let res = scan(tmp.path(), &opts, &cancel, |_| cancel.cancel());
        assert!(matches!(res, Err(CoreError::Cancelled)));
    }

    #[test]
    fn progress_ends_with_final_totals() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("x")).unwrap();
        write_file(&tmp.path().join("x/1"), 10);
        write_file(&tmp.path().join("2"), 10);
        let mut last = None;
        let mut calls = 0;
        scan(
            tmp.path(),
            &ScanOptions::default(),
            &CancelToken::new(),
            |p| {
                calls += 1;
                last = Some(p.clone());
            },
        )
        .unwrap();
        let last = last.unwrap();
        assert!(calls >= 1);
        assert_eq!((last.files, last.dirs, last.skipped_entries), (2, 2, 0));
        assert!(last.allocated_bytes > 0);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_loop_is_not_followed() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("sub");
        fs::create_dir(&sub).unwrap();
        write_file(&sub.join("f"), 5);
        std::os::unix::fs::symlink(tmp.path(), sub.join("loop")).unwrap();

        let tree = run(tmp.path());
        let sub_id = tree.child_by_name(ROOT, "sub").unwrap();
        let link = tree.child_by_name(sub_id, "loop").unwrap();
        let n = tree.node(link).unwrap();
        assert!(n.flags.contains(NodeFlags::REPARSE));
        assert!(!n.is_dir());
        assert_eq!(tree.children(link).count(), 0);
        assert_eq!(tree.len(), 4); // root, sub, f, loop
    }

    #[cfg(windows)]
    #[test]
    fn directory_symlink_is_not_followed_when_it_can_be_created() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("sub");
        fs::create_dir(&sub).unwrap();
        write_file(&sub.join("f"), 5);
        // Needs Developer Mode or admin rights; skip silently otherwise.
        if std::os::windows::fs::symlink_dir(tmp.path(), sub.join("loop")).is_err() {
            return;
        }
        let tree = run(tmp.path());
        let sub_id = tree.child_by_name(ROOT, "sub").unwrap();
        let link = tree.child_by_name(sub_id, "loop").unwrap();
        assert!(tree.node(link).unwrap().flags.contains(NodeFlags::REPARSE));
        assert_eq!(tree.len(), 4);
    }

    #[cfg(unix)]
    #[test]
    fn hard_links_are_counted_once() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(&tmp.path().join("orig"), 50_000);
        fs::hard_link(tmp.path().join("orig"), tmp.path().join("link")).unwrap();

        let tree = run(tmp.path());
        let r = tree.node(ROOT).unwrap();
        assert_eq!(r.logical_size, 50_000);
        assert_eq!(r.file_count, 2);
        let flagged = tree
            .children(ROOT)
            .filter(|&c| tree.node(c).unwrap().flags.contains(NodeFlags::HARDLINK))
            .count();
        assert_eq!(flagged, 1);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_directory_is_flagged_and_counted() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let locked = tmp.path().join("locked");
        fs::create_dir(&locked).unwrap();
        write_file(&locked.join("secret"), 10);
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        // Running as root ignores permissions: nothing to test then.
        let readable = fs::read_dir(&locked).is_ok();
        let tree = run(tmp.path());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        if readable {
            return;
        }

        let id = tree.child_by_name(ROOT, "locked").unwrap();
        assert!(tree
            .node(id)
            .unwrap()
            .flags
            .contains(NodeFlags::ACCESS_DENIED));
        assert_eq!(tree.stats().skipped_entries, 1);
        assert_eq!(tree.stats().skipped_samples, vec![locked]);
    }
}
