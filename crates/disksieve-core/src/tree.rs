//! Compact arena holding the whole scan result.
//!
//! Every filesystem entry is one fixed-size [`Node`] (48 B) in a flat `Vec`. Nodes hold no
//! heap allocations: names live in one shared buffer, extensions in a small dictionary, and
//! relations are `u32` indices (`parent` / `first_child` / `next_sibling`). This keeps a
//! 1 000 000-entry scan under ~80 MB of RAM.
//!
//! Lifecycle:
//! 1. the scanner feeds entries into a [`TreeBuilder`] (parents always before children);
//! 2. [`TreeBuilder::finish`] sums sizes bottom-up and sorts every child list by size;
//! 3. the resulting [`ScanTree`] is read-only, except for [`ScanTree::mark_trashed`], which
//!    keeps sums and ordering correct without a rescan.
//!
//! These types are internal to the core and are never serialized to the UI.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{CoreError, CoreResult};

/// Index of a node in [`ScanTree`]. A node does not store its own id.
pub type NodeId = u32;

/// Sentinel meaning "no parent / no child / no sibling".
pub const NO_NODE: NodeId = u32::MAX;

/// The scan root is always the first node.
pub const ROOT: NodeId = 0;

/// How many example paths of skipped entries are kept for the UI.
pub const MAX_SKIPPED_SAMPLES: usize = 50;

/// Longer "extensions" are treated as none (hash-named files, `name.with.many.dots`).
const MAX_EXTENSION_LEN: usize = 16;

bitflags::bitflags! {
    /// Per-node state bits.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct NodeFlags: u8 {
        /// Directory (otherwise a file).
        const DIR           = 1 << 0;
        /// Moved to the system Trash; its size is already subtracted from all ancestors.
        const TRASHED       = 1 << 1;
        /// Further occurrence of an already counted hard-linked file — excluded from byte sums.
        const HARDLINK      = 1 << 2;
        /// Online-only cloud file (OneDrive/iCloud) — must NEVER be opened for reading.
        const CLOUD_ONLINE  = 1 << 3;
        /// Symlink / junction / mount point — not followed.
        const REPARSE       = 1 << 4;
        /// Boundary of another filesystem — not scanned.
        const OTHER_VOLUME  = 1 << 5;
        /// Directory that could not be read.
        const ACCESS_DENIED = 1 << 6;
        /// The name was not valid Unicode and is stored lossily; the exact path cannot be
        /// rebuilt, so file operations on this node and its descendants are refused.
        const NAME_LOSSY    = 1 << 7;
    }
}

/// One filesystem entry. 48 bytes, no heap allocations.
///
/// For a directory the size, count and time fields describe the whole subtree once the tree
/// is finished.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Node {
    /// Offset of the name in [`ScanTree`]'s shared name buffer.
    pub name_off: u32,
    /// Name length in bytes (UTF-8).
    pub name_len: u16,
    /// Index into the extension dictionary (0 = no extension).
    pub ext_id: u16,
    pub parent: NodeId,
    /// Children form a singly linked list `first_child → next_sibling → …`, sorted by
    /// descending `allocated_size` once the tree is finished.
    pub first_child: NodeId,
    pub next_sibling: NodeId,
    pub flags: NodeFlags,
    /// `JunkKind` as `u8` (0 = not junk). Filled in by dev-junk detection.
    pub junk: u8,
    /// Logical size in bytes (directory: whole subtree).
    pub logical_size: u64,
    /// Size on disk in bytes (directory: whole subtree).
    pub allocated_size: u64,
    /// Number of files in the subtree (1 for a file).
    pub file_count: u32,
    /// Modification time as UNIX seconds (directory: newest file in the subtree).
    pub newest_mtime: u32,
}

// The memory budget depends on this; fail the build rather than a test if it ever grows.
const _: () = assert!(std::mem::size_of::<Node>() == 48);

impl Node {
    pub fn is_dir(&self) -> bool {
        self.flags.contains(NodeFlags::DIR)
    }

    pub fn is_trashed(&self) -> bool {
        self.flags.contains(NodeFlags::TRASHED)
    }

    /// What this node adds to its parent's totals: `(logical, allocated, files)`.
    ///
    /// Trashed nodes add nothing; hard links (already counted elsewhere) add only their files.
    pub fn contribution(&self) -> (u64, u64, u32) {
        if self.flags.contains(NodeFlags::TRASHED) {
            (0, 0, 0)
        } else if self.flags.contains(NodeFlags::HARDLINK) {
            (0, 0, self.file_count)
        } else {
            (self.logical_size, self.allocated_size, self.file_count)
        }
    }
}

/// Scan statistics kept next to the tree.
#[derive(Debug, Clone, Default)]
pub struct ScanStats {
    /// Wall-clock duration of the scan.
    pub duration: Duration,
    /// Entries that could not be read (permission denied, vanished, …).
    pub skipped_entries: u64,
    /// Up to [`MAX_SKIPPED_SAMPLES`] example paths of skipped entries.
    pub skipped_samples: Vec<PathBuf>,
}

/// Metadata of one entry handed to [`TreeBuilder`].
#[derive(Debug, Clone, Copy, Default)]
pub struct EntryMeta {
    pub logical_size: u64,
    pub allocated_size: u64,
    /// UNIX seconds, see [`to_unix_secs`].
    pub mtime: u32,
    /// Initial flags (`DIR`, `HARDLINK`, `CLOUD_ONLINE`, `REPARSE`, …).
    pub flags: NodeFlags,
}

/// Converts a timestamp to UNIX seconds clamped to the `u32` range (1970–2106).
pub fn to_unix_secs(time: SystemTime) -> u32 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(d) => u32::try_from(d.as_secs()).unwrap_or(u32::MAX),
        Err(_) => 0,
    }
}

/// Full result of one scan.
#[derive(Debug)]
pub struct ScanTree {
    root_path: PathBuf,
    nodes: Vec<Node>,
    names: String,
    extensions: Vec<Box<str>>,
    stats: ScanStats,
    generation: u64,
}

/// Builds a [`ScanTree`] entry by entry. Parents must be pushed before their children.
#[derive(Debug)]
pub struct TreeBuilder {
    tree: ScanTree,
    ext_index: HashMap<Box<str>, u16>,
}

impl TreeBuilder {
    /// Starts a tree whose root node is `root_path` itself (always a directory).
    pub fn new(root_path: impl Into<PathBuf>, generation: u64, root: EntryMeta) -> Self {
        let root_path = root_path.into();
        let root_name = root_path.to_string_lossy().into_owned();
        let mut builder = Self {
            tree: ScanTree {
                root_path,
                nodes: Vec::new(),
                names: String::new(),
                extensions: vec!["".into()],
                stats: ScanStats::default(),
                generation,
            },
            ext_index: HashMap::new(),
        };
        let name_len = u16::try_from(root_name.len()).unwrap_or(u16::MAX);
        let name = truncate_to_char_boundary(&root_name, usize::from(name_len));
        builder.tree.names.push_str(name);
        builder.tree.nodes.push(Node {
            name_off: 0,
            name_len: name.len() as u16,
            ext_id: 0,
            parent: NO_NODE,
            first_child: NO_NODE,
            next_sibling: NO_NODE,
            flags: (root.flags | NodeFlags::DIR) - NodeFlags::TRASHED - NodeFlags::HARDLINK,
            junk: 0,
            logical_size: root.logical_size,
            allocated_size: root.allocated_size,
            file_count: 0,
            newest_mtime: 0,
        });
        builder
    }

    /// Reserves room for `additional` more nodes.
    pub fn reserve(&mut self, additional: usize) {
        self.tree.nodes.reserve(additional);
    }

    /// Number of nodes pushed so far (including the root).
    pub fn len(&self) -> usize {
        self.tree.nodes.len()
    }

    /// Always `false`: the root node exists from the start.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Adds an entry under `parent` and returns its id.
    pub fn push(&mut self, parent: NodeId, name: &OsStr, meta: EntryMeta) -> CoreResult<NodeId> {
        match self.tree.get(parent) {
            None => return Err(CoreError::InvalidNode(parent)),
            Some(p) if !p.is_dir() => {
                return Err(CoreError::NodeNotAllowed {
                    id: parent,
                    reason: "parent is not a directory",
                })
            }
            Some(_) => {}
        }
        let id = NodeId::try_from(self.tree.nodes.len())
            .ok()
            .filter(|&id| id != NO_NODE)
            .ok_or(CoreError::TreeFull)?;

        let mut flags = meta.flags - NodeFlags::TRASHED;
        let lossy;
        let name: &str = match name.to_str() {
            Some(s) => s,
            None => {
                flags |= NodeFlags::NAME_LOSSY;
                lossy = name.to_string_lossy();
                &lossy
            }
        };
        let name_len = u16::try_from(name.len()).map_err(|_| CoreError::TreeFull)?;
        let name_off = u32::try_from(self.tree.names.len())
            .ok()
            .filter(|off| off.checked_add(u32::from(name_len)).is_some())
            .ok_or(CoreError::TreeFull)?;
        self.tree.names.push_str(name);

        let is_dir = flags.contains(NodeFlags::DIR);
        let ext_id = if is_dir {
            0
        } else {
            self.intern_extension(name)
        };
        let parent_node = &mut self.tree.nodes[parent as usize];
        let next_sibling = parent_node.first_child;
        parent_node.first_child = id;

        self.tree.nodes.push(Node {
            name_off,
            name_len,
            ext_id,
            parent,
            first_child: NO_NODE,
            next_sibling,
            flags,
            junk: 0,
            logical_size: meta.logical_size,
            allocated_size: meta.allocated_size,
            file_count: u32::from(!is_dir),
            newest_mtime: if is_dir { 0 } else { meta.mtime },
        });
        Ok(id)
    }

    /// Adds flags to an already pushed node (e.g. `ACCESS_DENIED` after a failed `read_dir`).
    pub fn add_flags(&mut self, id: NodeId, flags: NodeFlags) -> CoreResult<()> {
        let node = self
            .tree
            .nodes
            .get_mut(id as usize)
            .ok_or(CoreError::InvalidNode(id))?;
        node.flags |= flags - NodeFlags::TRASHED - NodeFlags::DIR;
        Ok(())
    }

    /// Counts an entry that could not be read and keeps it as an example if there is room.
    pub fn record_skipped(&mut self, path: impl Into<PathBuf>) {
        let stats = &mut self.tree.stats;
        stats.skipped_entries += 1;
        if stats.skipped_samples.len() < MAX_SKIPPED_SAMPLES {
            stats.skipped_samples.push(path.into());
        }
    }

    /// Sums sizes bottom-up, sorts every child list by size and releases spare capacity.
    pub fn finish(mut self, duration: Duration) -> ScanTree {
        let tree = &mut self.tree;
        tree.stats.duration = duration;

        // Children always have a larger index than their parent, so one reverse pass
        // sees every subtree complete before it is added to the level above.
        for i in (1..tree.nodes.len()).rev() {
            let node = tree.nodes[i];
            let (logical, allocated, files) = node.contribution();
            let parent = &mut tree.nodes[node.parent as usize];
            parent.logical_size = parent.logical_size.saturating_add(logical);
            parent.allocated_size = parent.allocated_size.saturating_add(allocated);
            parent.file_count = parent.file_count.saturating_add(files);
            parent.newest_mtime = parent.newest_mtime.max(node.newest_mtime);
        }

        let mut scratch: Vec<NodeId> = Vec::new();
        for dir in 0..tree.nodes.len() {
            if tree.nodes[dir].first_child == NO_NODE {
                continue;
            }
            scratch.clear();
            scratch.extend(tree.children(dir as NodeId));
            scratch.sort_unstable_by(|&a, &b| tree.order(a, b));
            let mut next = NO_NODE;
            for &child in scratch.iter().rev() {
                tree.nodes[child as usize].next_sibling = next;
                next = child;
            }
            tree.nodes[dir].first_child = next;
        }

        tree.nodes.shrink_to_fit();
        tree.names.shrink_to_fit();
        tree.extensions.shrink_to_fit();
        self.tree
    }

    fn intern_extension(&mut self, name: &str) -> u16 {
        let Some(ext) = extension_of(name) else {
            return 0;
        };
        let lower;
        let ext = if ext.bytes().any(|b| b.is_ascii_uppercase()) {
            lower = ext.to_ascii_lowercase();
            lower.as_str()
        } else {
            ext
        };
        if let Some(&id) = self.ext_index.get(ext) {
            return id;
        }
        // Dictionary full (65 535 distinct extensions): fall back to "no extension".
        let Ok(id) = u16::try_from(self.tree.extensions.len()) else {
            return 0;
        };
        let boxed: Box<str> = ext.into();
        self.tree.extensions.push(boxed.clone());
        self.ext_index.insert(boxed, id);
        id
    }
}

/// Extension of a file name, without the dot. Hidden files (`.bashrc`), trailing dots and
/// implausibly long or space-containing suffixes count as "no extension".
fn extension_of(name: &str) -> Option<&str> {
    let dot = name.rfind('.')?;
    let ext = &name[dot + 1..];
    let plausible = dot > 0
        && !ext.is_empty()
        && ext.len() <= MAX_EXTENSION_LEN
        && !ext.contains(char::is_whitespace);
    plausible.then_some(ext)
}

fn truncate_to_char_boundary(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

impl ScanTree {
    /// Path the scan started at (the root node).
    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    /// Scan number; the UI drops responses whose generation is not current.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn stats(&self) -> &ScanStats {
        &self.stats
    }

    /// Number of nodes, including the root.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Always `false`: a tree has at least its root.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id as usize)
    }

    /// Like [`get`](Self::get) but with an error suitable for IPC handlers.
    pub fn node(&self, id: NodeId) -> CoreResult<&Node> {
        self.get(id).ok_or(CoreError::InvalidNode(id))
    }

    /// Name of the node (the full root path for [`ROOT`]). Empty for an invalid id.
    pub fn name(&self, id: NodeId) -> &str {
        self.get(id).map_or("", |n| {
            let start = n.name_off as usize;
            &self.names[start..start + usize::from(n.name_len)]
        })
    }

    /// Lower-case extension without the dot, or `""`.
    pub fn extension(&self, id: NodeId) -> &str {
        self.get(id)
            .and_then(|n| self.extensions.get(usize::from(n.ext_id)))
            .map_or("", |e| e.as_ref())
    }

    /// Extension dictionary; index 0 is `""`.
    pub fn extensions(&self) -> &[Box<str>] {
        &self.extensions
    }

    /// Direct children, largest first. Includes trashed nodes — callers filter them out.
    pub fn children(&self, id: NodeId) -> Children<'_> {
        Children {
            tree: self,
            next: self.get(id).map_or(NO_NODE, |n| n.first_child),
        }
    }

    /// Ancestors from the parent up to the root (excluding `id` itself).
    pub fn ancestors(&self, id: NodeId) -> Ancestors<'_> {
        Ancestors {
            tree: self,
            next: self.get(id).map_or(NO_NODE, |n| n.parent),
        }
    }

    /// Child of `parent` with exactly this name.
    pub fn child_by_name(&self, parent: NodeId, name: &str) -> Option<NodeId> {
        self.children(parent).find(|&c| self.name(c) == name)
    }

    /// Exact filesystem path of the node, rebuilt by walking up to the root.
    ///
    /// Fails for nodes whose path contains a lossily stored (non-Unicode) name, so a file
    /// operation can never hit a different file than the one that was scanned.
    pub fn resolve_path(&self, id: NodeId) -> CoreResult<PathBuf> {
        let chain = self.chain_to_root(id)?;
        if chain
            .iter()
            .any(|&c| self.nodes[c as usize].flags.contains(NodeFlags::NAME_LOSSY))
        {
            return Err(CoreError::UnrepresentablePath(id));
        }
        Ok(self.join_chain(&chain))
    }

    /// Human-readable path; may contain replacement characters for non-Unicode names.
    pub fn display_path(&self, id: NodeId) -> String {
        self.chain_to_root(id)
            .map(|chain| self.join_chain(&chain).to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Marks a node as moved to Trash: subtracts its totals from every ancestor and keeps
    /// each affected child list sorted. O(depth × siblings), no rescan.
    ///
    /// `newest_mtime` of ancestors is left as is (it may now be an upper bound).
    pub fn mark_trashed(&mut self, id: NodeId) -> CoreResult<()> {
        let node = *self.node(id)?;
        if id == ROOT {
            return Err(CoreError::NodeNotAllowed {
                id,
                reason: "the scan root cannot be trashed",
            });
        }
        if node.is_trashed()
            || self
                .ancestors(id)
                .any(|a| self.nodes[a as usize].is_trashed())
        {
            return Err(CoreError::NodeNotAllowed {
                id,
                reason: "already trashed",
            });
        }

        let (logical, allocated, files) = node.contribution();
        self.nodes[id as usize].flags |= NodeFlags::TRASHED;

        let mut child = id;
        let mut current = node.parent;
        while current != NO_NODE {
            let n = &mut self.nodes[current as usize];
            n.logical_size = n.logical_size.saturating_sub(logical);
            n.allocated_size = n.allocated_size.saturating_sub(allocated);
            n.file_count = n.file_count.saturating_sub(files);
            let parent = n.parent;
            if child != id {
                self.reposition_after_shrink(child);
            }
            child = current;
            current = parent;
        }
        Ok(())
    }

    /// Approximate heap + inline memory used by the tree, in bytes.
    pub fn memory_usage(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.nodes.capacity() * std::mem::size_of::<Node>()
            + self.names.capacity()
            + self.extensions.capacity() * std::mem::size_of::<Box<str>>()
            + self.extensions.iter().map(|e| e.len()).sum::<usize>()
            + self.stats.skipped_samples.capacity() * std::mem::size_of::<PathBuf>()
            + self
                .stats
                .skipped_samples
                .iter()
                .map(|p| p.as_os_str().len())
                .sum::<usize>()
    }

    /// Sibling order: allocated size desc, logical size desc, then name for stability.
    fn order(&self, a: NodeId, b: NodeId) -> Ordering {
        let (na, nb) = (&self.nodes[a as usize], &self.nodes[b as usize]);
        nb.allocated_size
            .cmp(&na.allocated_size)
            .then(nb.logical_size.cmp(&na.logical_size))
            .then_with(|| self.name(a).cmp(self.name(b)))
    }

    /// Moves `id` later in its parent's child list after its size decreased.
    fn reposition_after_shrink(&mut self, id: NodeId) {
        let parent = self.nodes[id as usize].parent;
        if parent == NO_NODE {
            return;
        }
        // Unlink `id`, remembering its predecessor.
        let next = self.nodes[id as usize].next_sibling;
        let mut prev = NO_NODE;
        let mut cursor = self.nodes[parent as usize].first_child;
        while cursor != id {
            prev = cursor;
            cursor = self.nodes[cursor as usize].next_sibling;
        }
        if prev == NO_NODE {
            self.nodes[parent as usize].first_child = next;
        } else {
            self.nodes[prev as usize].next_sibling = next;
        }
        // It only shrank, so its new place is at or after the old one.
        cursor = next;
        while cursor != NO_NODE && self.order(cursor, id) == Ordering::Less {
            prev = cursor;
            cursor = self.nodes[cursor as usize].next_sibling;
        }
        self.nodes[id as usize].next_sibling = cursor;
        if prev == NO_NODE {
            self.nodes[parent as usize].first_child = id;
        } else {
            self.nodes[prev as usize].next_sibling = id;
        }
    }

    /// Ids from `id` up to (excluding) the root, deepest first.
    fn chain_to_root(&self, id: NodeId) -> CoreResult<Vec<NodeId>> {
        self.node(id)?;
        let mut chain = Vec::new();
        let mut current = id;
        while current != ROOT {
            chain.push(current);
            current = self.nodes[current as usize].parent;
        }
        Ok(chain)
    }

    fn join_chain(&self, chain: &[NodeId]) -> PathBuf {
        let mut path = self.root_path.clone();
        for &c in chain.iter().rev() {
            path.push(self.name(c));
        }
        path
    }
}

/// Iterator over a node's direct children. See [`ScanTree::children`].
pub struct Children<'a> {
    tree: &'a ScanTree,
    next: NodeId,
}

impl Iterator for Children<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let id = self.next;
        let node = self.tree.get(id)?;
        self.next = node.next_sibling;
        Some(id)
    }
}

/// Iterator from a node's parent up to the root. See [`ScanTree::ancestors`].
pub struct Ancestors<'a> {
    tree: &'a ScanTree,
    next: NodeId,
}

impl Iterator for Ancestors<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let id = self.next;
        let node = self.tree.get(id)?;
        self.next = node.parent;
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(size: u64, mtime: u32) -> EntryMeta {
        EntryMeta {
            logical_size: size,
            allocated_size: size.div_ceil(4096) * 4096,
            mtime,
            flags: NodeFlags::empty(),
        }
    }

    fn dir() -> EntryMeta {
        EntryMeta {
            flags: NodeFlags::DIR,
            ..EntryMeta::default()
        }
    }

    fn root_path() -> PathBuf {
        PathBuf::from(if cfg!(windows) { r"C:\scan" } else { "/scan" })
    }

    /// root/
    /// ├── small.txt   100 B   (alloc 4096)
    /// ├── src/
    /// │   ├── Main.RS 10 000 B (alloc 12 288)
    /// │   └── lib.rs  5 000 B  (alloc 8 192)
    /// └── empty/
    struct Sample {
        tree: ScanTree,
        small: NodeId,
        src: NodeId,
        main_rs: NodeId,
        lib_rs: NodeId,
        empty: NodeId,
    }

    fn sample() -> Sample {
        let mut b = TreeBuilder::new(root_path(), 7, dir());
        let small = b
            .push(ROOT, OsStr::new("small.txt"), file(100, 10))
            .unwrap();
        let src = b.push(ROOT, OsStr::new("src"), dir()).unwrap();
        let main_rs = b
            .push(src, OsStr::new("Main.RS"), file(10_000, 30))
            .unwrap();
        let lib_rs = b.push(src, OsStr::new("lib.rs"), file(5_000, 20)).unwrap();
        let empty = b.push(ROOT, OsStr::new("empty"), dir()).unwrap();
        Sample {
            tree: b.finish(Duration::from_millis(5)),
            small,
            src,
            main_rs,
            lib_rs,
            empty,
        }
    }

    /// Every child list is sorted by `ScanTree::order` and links back to its parent.
    fn assert_invariants(tree: &ScanTree) {
        for id in 0..tree.len() as NodeId {
            let kids: Vec<_> = tree.children(id).collect();
            for &k in &kids {
                assert_eq!(tree.nodes[k as usize].parent, id);
            }
            for pair in kids.windows(2) {
                assert_ne!(
                    tree.order(pair[0], pair[1]),
                    Ordering::Greater,
                    "children of {id} not sorted: {kids:?}"
                );
            }
        }
    }

    #[test]
    fn node_is_48_bytes() {
        assert_eq!(std::mem::size_of::<Node>(), 48);
    }

    #[test]
    fn finish_sums_sizes_counts_and_mtime_bottom_up() {
        let s = sample();
        let t = &s.tree;
        let src = t.node(s.src).unwrap();
        assert_eq!(src.logical_size, 15_000);
        assert_eq!(src.allocated_size, 12_288 + 8_192);
        assert_eq!(src.file_count, 2);
        assert_eq!(src.newest_mtime, 30);

        let root = t.node(ROOT).unwrap();
        assert_eq!(root.logical_size, 15_100);
        assert_eq!(root.allocated_size, 4_096 + 12_288 + 8_192);
        assert_eq!(root.file_count, 3);
        assert_eq!(root.newest_mtime, 30);

        let empty = t.node(s.empty).unwrap();
        assert_eq!((empty.allocated_size, empty.file_count), (0, 0));
        assert_eq!(t.stats().duration, Duration::from_millis(5));
        assert_eq!(t.generation(), 7);
    }

    #[test]
    fn directory_own_size_is_included() {
        let own = EntryMeta {
            allocated_size: 4096,
            ..dir()
        };
        let mut b = TreeBuilder::new(root_path(), 1, own);
        let sub = b.push(ROOT, OsStr::new("sub"), own).unwrap();
        b.push(sub, OsStr::new("a"), file(1, 1)).unwrap();
        let t = b.finish(Duration::ZERO);
        assert_eq!(t.node(sub).unwrap().allocated_size, 4096 + 4096);
        assert_eq!(t.node(ROOT).unwrap().allocated_size, 3 * 4096);
    }

    #[test]
    fn children_are_sorted_largest_first() {
        let s = sample();
        let root_kids: Vec<_> = s.tree.children(ROOT).collect();
        assert_eq!(root_kids, vec![s.src, s.small, s.empty]);
        let src_kids: Vec<_> = s.tree.children(s.src).collect();
        assert_eq!(src_kids, vec![s.main_rs, s.lib_rs]);
        assert_invariants(&s.tree);
    }

    #[test]
    fn names_and_extensions_are_interned() {
        let s = sample();
        let t = &s.tree;
        assert_eq!(t.name(s.main_rs), "Main.RS");
        assert_eq!(t.extension(s.main_rs), "rs");
        assert_eq!(t.extension(s.lib_rs), "rs");
        assert_eq!(
            t.node(s.main_rs).unwrap().ext_id,
            t.node(s.lib_rs).unwrap().ext_id
        );
        assert_eq!(t.extension(s.small), "txt");
        assert_eq!(t.extension(s.src), "");
        let expected: Vec<Box<str>> = vec!["".into(), "txt".into(), "rs".into()];
        assert_eq!(t.extensions(), expected.as_slice());
        assert_eq!(t.name(9_999), "");
    }

    #[test]
    fn extension_rules() {
        assert_eq!(extension_of("archive.tar.gz"), Some("gz"));
        assert_eq!(extension_of(".bashrc"), None);
        assert_eq!(extension_of("Makefile"), None);
        assert_eq!(extension_of("trailing."), None);
        assert_eq!(extension_of("v1.2 final"), None);
        assert_eq!(extension_of("blob.0123456789abcdef0123"), None);
    }

    #[test]
    fn directories_never_get_an_extension() {
        let mut b = TreeBuilder::new(root_path(), 1, dir());
        let d = b.push(ROOT, OsStr::new("conf.d"), dir()).unwrap();
        let t = b.finish(Duration::ZERO);
        assert_eq!(t.extension(d), "");
    }

    #[test]
    fn resolve_path_rebuilds_full_path() {
        let s = sample();
        let expected = root_path().join("src").join("Main.RS");
        assert_eq!(s.tree.resolve_path(s.main_rs).unwrap(), expected);
        assert_eq!(s.tree.resolve_path(ROOT).unwrap(), root_path());
        assert_eq!(
            s.tree.display_path(s.main_rs),
            expected.to_string_lossy().into_owned()
        );
        assert!(matches!(
            s.tree.resolve_path(1_000),
            Err(CoreError::InvalidNode(1_000))
        ));
    }

    #[test]
    fn root_name_is_the_root_path() {
        let s = sample();
        assert_eq!(s.tree.name(ROOT), root_path().to_string_lossy());
        assert_eq!(s.tree.root_path(), root_path());
    }

    #[cfg(unix)]
    fn non_unicode_name() -> std::ffi::OsString {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(vec![b'b', b'a', b'd', 0xFF])
    }

    #[cfg(windows)]
    fn non_unicode_name() -> std::ffi::OsString {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&[u16::from(b'b'), u16::from(b'a'), 0xD800])
    }

    #[test]
    fn non_unicode_names_are_flagged_and_never_resolved() {
        let mut b = TreeBuilder::new(root_path(), 1, dir());
        let bad_dir = b.push(ROOT, &non_unicode_name(), dir()).unwrap();
        let inner = b.push(bad_dir, OsStr::new("ok.txt"), file(1, 1)).unwrap();
        let t = b.finish(Duration::ZERO);

        assert!(t
            .node(bad_dir)
            .unwrap()
            .flags
            .contains(NodeFlags::NAME_LOSSY));
        assert!(t.name(bad_dir).contains('\u{FFFD}'));
        assert!(matches!(
            t.resolve_path(bad_dir),
            Err(CoreError::UnrepresentablePath(_))
        ));
        // A valid name below a lossy directory is still unreachable exactly.
        assert!(matches!(
            t.resolve_path(inner),
            Err(CoreError::UnrepresentablePath(_))
        ));
        assert!(t.display_path(inner).ends_with("ok.txt"));
    }

    #[test]
    fn hard_links_are_not_counted_twice() {
        let mut b = TreeBuilder::new(root_path(), 1, dir());
        b.push(ROOT, OsStr::new("a"), file(8192, 1)).unwrap();
        let link = EntryMeta {
            flags: NodeFlags::HARDLINK,
            ..file(8192, 1)
        };
        let l = b.push(ROOT, OsStr::new("b"), link).unwrap();
        let t = b.finish(Duration::ZERO);
        let root = t.node(ROOT).unwrap();
        assert_eq!(root.logical_size, 8192);
        assert_eq!(root.allocated_size, 8192);
        assert_eq!(root.file_count, 2);
        // The link itself still reports the file's size for display.
        assert_eq!(t.node(l).unwrap().logical_size, 8192);
    }

    #[test]
    fn push_rejects_bad_parents() {
        let mut b = TreeBuilder::new(root_path(), 1, dir());
        let f = b.push(ROOT, OsStr::new("f"), file(1, 1)).unwrap();
        assert!(matches!(
            b.push(f, OsStr::new("x"), file(1, 1)),
            Err(CoreError::NodeNotAllowed { .. })
        ));
        assert!(matches!(
            b.push(42, OsStr::new("x"), file(1, 1)),
            Err(CoreError::InvalidNode(42))
        ));
        assert_eq!(b.len(), 2);
    }

    #[test]
    fn builder_ignores_trashed_flag_on_input() {
        let mut b = TreeBuilder::new(root_path(), 1, dir());
        let meta = EntryMeta {
            flags: NodeFlags::TRASHED,
            ..file(10, 1)
        };
        let f = b.push(ROOT, OsStr::new("f"), meta).unwrap();
        b.add_flags(f, NodeFlags::TRASHED | NodeFlags::CLOUD_ONLINE)
            .unwrap();
        let t = b.finish(Duration::ZERO);
        let n = t.node(f).unwrap();
        assert!(!n.is_trashed());
        assert!(n.flags.contains(NodeFlags::CLOUD_ONLINE));
        assert_eq!(t.node(ROOT).unwrap().logical_size, 10);
    }

    #[test]
    fn mark_trashed_updates_ancestors_and_keeps_order() {
        let s = sample();
        let mut t = s.tree;
        t.mark_trashed(s.main_rs).unwrap();

        assert!(t.node(s.main_rs).unwrap().is_trashed());
        let src = t.node(s.src).unwrap();
        assert_eq!(
            (src.logical_size, src.allocated_size, src.file_count),
            (5_000, 8_192, 1)
        );
        let root = t.node(ROOT).unwrap();
        assert_eq!(
            (root.logical_size, root.allocated_size, root.file_count),
            (5_100, 4_096 + 8_192, 2)
        );
        // `src` is still the largest child of the root; order holds everywhere.
        assert_invariants(&t);

        t.mark_trashed(s.lib_rs).unwrap();
        // `src` is now empty: it moves behind `small.txt` and, tied with `empty` at 0 B,
        // is ordered by name.
        let root_kids: Vec<_> = t.children(ROOT).collect();
        assert_eq!(root_kids, vec![s.small, s.empty, s.src]);
        assert_eq!(t.node(ROOT).unwrap().allocated_size, 4_096);
        assert_invariants(&t);
    }

    #[test]
    fn mark_trashed_on_a_directory_removes_its_subtree() {
        let s = sample();
        let mut t = s.tree;
        t.mark_trashed(s.src).unwrap();
        let root = t.node(ROOT).unwrap();
        assert_eq!(
            (root.logical_size, root.allocated_size, root.file_count),
            (100, 4_096, 1)
        );
        assert!(matches!(
            t.mark_trashed(s.main_rs),
            Err(CoreError::NodeNotAllowed { .. })
        ));
        assert_invariants(&t);
    }

    #[test]
    fn mark_trashed_rejects_root_repeats_and_invalid_ids() {
        let s = sample();
        let mut t = s.tree;
        assert!(matches!(
            t.mark_trashed(ROOT),
            Err(CoreError::NodeNotAllowed { .. })
        ));
        t.mark_trashed(s.small).unwrap();
        assert!(matches!(
            t.mark_trashed(s.small),
            Err(CoreError::NodeNotAllowed { .. })
        ));
        assert!(matches!(
            t.mark_trashed(NO_NODE),
            Err(CoreError::InvalidNode(_))
        ));
        assert_eq!(t.node(ROOT).unwrap().file_count, 2);
    }

    #[test]
    fn reposition_moves_shrunk_directory_past_many_siblings() {
        let mut b = TreeBuilder::new(root_path(), 1, dir());
        let big = b.push(ROOT, OsStr::new("big"), dir()).unwrap();
        let payload = b
            .push(big, OsStr::new("payload"), file(1 << 30, 1))
            .unwrap();
        b.push(big, OsStr::new("tiny"), file(1, 1)).unwrap();
        for i in 0..50u64 {
            let name = format!("f{i:02}");
            b.push(ROOT, OsStr::new(&name), file((i + 1) * 10_000, 1))
                .unwrap();
        }
        let mut t = b.finish(Duration::ZERO);
        assert_eq!(t.children(ROOT).next(), Some(big));

        t.mark_trashed(payload).unwrap();
        assert_invariants(&t);
        // Only `tiny` (4 KiB on disk) is left in `big`; the smallest file f00 takes 12 KiB,
        // so `big` moves from first to last place.
        assert_eq!(t.children(ROOT).last(), Some(big));
        assert_eq!(t.children(ROOT).count(), 51);
    }

    #[test]
    fn ancestors_walk_to_root() {
        let s = sample();
        let chain: Vec<_> = s.tree.ancestors(s.main_rs).collect();
        assert_eq!(chain, vec![s.src, ROOT]);
        assert_eq!(s.tree.ancestors(ROOT).count(), 0);
        assert_eq!(s.tree.child_by_name(s.src, "lib.rs"), Some(s.lib_rs));
        assert_eq!(s.tree.child_by_name(s.src, "nope"), None);
    }

    #[test]
    fn skipped_entries_are_counted_and_sampled() {
        let mut b = TreeBuilder::new(root_path(), 1, dir());
        for i in 0..(MAX_SKIPPED_SAMPLES + 25) {
            b.record_skipped(format!("denied-{i}"));
        }
        let t = b.finish(Duration::ZERO);
        assert_eq!(t.stats().skipped_entries, MAX_SKIPPED_SAMPLES as u64 + 25);
        assert_eq!(t.stats().skipped_samples.len(), MAX_SKIPPED_SAMPLES);
    }

    #[test]
    fn unix_secs_are_clamped() {
        assert_eq!(to_unix_secs(UNIX_EPOCH), 0);
        assert_eq!(to_unix_secs(UNIX_EPOCH - Duration::from_secs(5)), 0);
        assert_eq!(to_unix_secs(UNIX_EPOCH + Duration::from_secs(42)), 42);
        let far = UNIX_EPOCH + Duration::from_secs(u64::from(u32::MAX) + 10);
        assert_eq!(to_unix_secs(far), u32::MAX);
    }

    /// 1 000 000 nodes (1 000 directories × 999 files) must fit the 80 MB budget.
    #[test]
    fn million_nodes_fit_memory_budget() {
        const DIRS: u32 = 1_000;
        const FILES_PER_DIR: u32 = 999;
        let mut b = TreeBuilder::new(root_path(), 1, dir());
        b.reserve((DIRS * (FILES_PER_DIR + 1)) as usize);
        let mut name = String::new();
        for d in 0..DIRS {
            name.clear();
            name.push_str(&format!("directory_{d:04}"));
            let dir_id = b.push(ROOT, OsStr::new(&name), dir()).unwrap();
            for f in 0..FILES_PER_DIR {
                name.clear();
                name.push_str(&format!(
                    "document_{f:04}.{}",
                    ["txt", "rs", "png"][f as usize % 3]
                ));
                b.push(dir_id, OsStr::new(&name), file(u64::from(f) * 100, f))
                    .unwrap();
            }
        }
        let t = b.finish(Duration::ZERO);
        assert_eq!(t.len(), 1 + (DIRS * (FILES_PER_DIR + 1)) as usize);
        assert_eq!(t.node(ROOT).unwrap().file_count, DIRS * FILES_PER_DIR);

        let used = t.memory_usage();
        assert!(used <= 80 * 1024 * 1024, "{} MiB", used / (1024 * 1024));
    }
}
