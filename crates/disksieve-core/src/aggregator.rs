//! View projection: the largest children of one directory plus a single aggregated
//! "Other" bucket, so the UI never receives more than a bounded number of items.
//!
//! The full tree stays in Rust; the Tauri layer maps a [`Viewport`] to its IPC DTO.

use std::cmp::Ordering;

use crate::error::{CoreError, CoreResult};
use crate::tree::{Node, NodeId, ScanTree};

/// Default number of individual items in a viewport (the rest goes to "Other").
pub const DEFAULT_MAX_ITEMS: usize = 200;

/// Size used for ordering and for the treemap area.
///
/// `Delta` (growth since the previous snapshot) arrives with snapshots in v0.2.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SizeMetric {
    /// Space used on disk (cluster rounding, compression, sparse files).
    #[default]
    Allocated,
    /// Apparent file size.
    Logical,
}

impl SizeMetric {
    /// What `node` contributes to its parent under this metric: 0 for trashed nodes and
    /// hard links that were already counted elsewhere.
    pub fn value(self, node: &Node) -> u64 {
        let (logical, allocated, _) = node.contribution();
        match self {
            Self::Allocated => allocated,
            Self::Logical => logical,
        }
    }

    /// The other metric, used as a tie-breaker.
    fn secondary(self, node: &Node) -> u64 {
        match self {
            Self::Allocated => Self::Logical.value(node),
            Self::Logical => Self::Allocated.value(node),
        }
    }
}

/// Options for [`build_viewport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewportOptions {
    pub metric: SizeMetric,
    /// Maximum number of individual items; everything beyond is merged into "Other".
    pub max_items: usize,
}

impl Default for ViewportOptions {
    fn default() -> Self {
        Self {
            metric: SizeMetric::default(),
            max_items: DEFAULT_MAX_ITEMS,
        }
    }
}

/// One child shown individually. Everything else about it is read from the tree by `id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewportItem {
    pub id: NodeId,
    /// Value under the requested metric (0 for hard links counted elsewhere).
    pub value: u64,
}

/// Totals of the children merged into the "Other" bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OtherBucket {
    /// Number of merged children.
    pub count: u32,
    pub value: u64,
    pub logical_size: u64,
    pub allocated_size: u64,
    pub file_count: u32,
    pub newest_mtime: u32,
}

/// Projection of one directory, ready to be mapped to the IPC DTO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewport {
    /// Generation of the scan this view was built from.
    pub generation: u64,
    pub current: NodeId,
    /// From the root down to `current` (inclusive).
    pub breadcrumbs: Vec<NodeId>,
    pub metric: SizeMetric,
    /// Sum of all visible children (items + "Other"). It can be smaller than the
    /// directory's own total, which also includes the directory entry itself.
    pub total_value: u64,
    /// Largest children first.
    pub items: Vec<ViewportItem>,
    /// `None` when every child fits in `items`.
    pub other: Option<OtherBucket>,
}

/// Builds the view of directory `id`: up to `opts.max_items` largest children (by
/// `opts.metric`, descending) and one "Other" bucket with the rest.
///
/// Trashed children are skipped. Runs in O(children + k log k) without allocating per node
/// beyond one scratch vector.
///
/// # Errors
/// [`CoreError::InvalidNode`] for an unknown id, [`CoreError::NodeNotAllowed`] when the
/// node is a file or it (or one of its ancestors) was moved to Trash.
pub fn build_viewport(tree: &ScanTree, id: NodeId, opts: &ViewportOptions) -> CoreResult<Viewport> {
    let current = tree.node(id)?;
    if !current.is_dir() {
        return Err(CoreError::NodeNotAllowed {
            id,
            reason: "not a directory",
        });
    }

    let mut breadcrumbs: Vec<NodeId> = std::iter::once(id).chain(tree.ancestors(id)).collect();
    breadcrumbs.reverse();
    if breadcrumbs
        .iter()
        .any(|&b| tree.get(b).is_some_and(Node::is_trashed))
    {
        return Err(CoreError::NodeNotAllowed {
            id,
            reason: "moved to trash",
        });
    }

    let metric = opts.metric;
    let mut children: Vec<Candidate> = tree
        .children(id)
        .filter_map(|child| {
            let node = tree.get(child)?;
            (!node.is_trashed()).then(|| Candidate {
                value: metric.value(node),
                secondary: metric.secondary(node),
                id: child,
            })
        })
        .collect();

    // Largest first; ties broken by the other metric, then by name, then by id, so the
    // order is total and stable across calls.
    let order = |a: &Candidate, b: &Candidate| -> Ordering {
        b.value
            .cmp(&a.value)
            .then(b.secondary.cmp(&a.secondary))
            .then_with(|| tree.name(a.id).cmp(tree.name(b.id)))
            .then(a.id.cmp(&b.id))
    };

    let keep = opts.max_items.min(children.len());
    if keep > 0 && keep < children.len() {
        children.select_nth_unstable_by(keep - 1, order);
    }
    let (top, rest) = children.split_at_mut(keep);
    top.sort_unstable_by(order);

    let items: Vec<ViewportItem> = top
        .iter()
        .map(|c| ViewportItem {
            id: c.id,
            value: c.value,
        })
        .collect();

    let other = (!rest.is_empty()).then(|| {
        rest.iter().fold(OtherBucket::default(), |mut acc, c| {
            let node = &tree.get(c.id).expect("child ids come from the tree");
            let (logical, allocated, files) = node.contribution();
            acc.count = acc.count.saturating_add(1);
            acc.value = acc.value.saturating_add(c.value);
            acc.logical_size = acc.logical_size.saturating_add(logical);
            acc.allocated_size = acc.allocated_size.saturating_add(allocated);
            acc.file_count = acc.file_count.saturating_add(files);
            acc.newest_mtime = acc.newest_mtime.max(node.newest_mtime);
            acc
        })
    });

    let total_value = items
        .iter()
        .map(|i| i.value)
        .chain(other.map(|o| o.value))
        .fold(0u64, u64::saturating_add);

    Ok(Viewport {
        generation: tree.generation(),
        current: id,
        breadcrumbs,
        metric,
        total_value,
        items,
        other,
    })
}

struct Candidate {
    value: u64,
    secondary: u64,
    id: NodeId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{EntryMeta, NodeFlags, TreeBuilder, ROOT};
    use std::ffi::OsStr;
    use std::path::PathBuf;
    use std::time::Duration;

    fn file(logical: u64, allocated: u64) -> EntryMeta {
        EntryMeta {
            logical_size: logical,
            allocated_size: allocated,
            mtime: 1_000,
            flags: NodeFlags::empty(),
        }
    }

    fn dir() -> EntryMeta {
        EntryMeta {
            flags: NodeFlags::DIR,
            ..EntryMeta::default()
        }
    }

    fn builder() -> TreeBuilder {
        let root = PathBuf::from(if cfg!(windows) { r"C:\scan" } else { "/scan" });
        TreeBuilder::new(root, 3, dir())
    }

    fn push(b: &mut TreeBuilder, parent: NodeId, name: &str, meta: EntryMeta) -> NodeId {
        b.push(parent, OsStr::new(name), meta).unwrap()
    }

    fn ids(view: &Viewport) -> Vec<NodeId> {
        view.items.iter().map(|i| i.id).collect()
    }

    /// root/
    /// ├── big.bin     1 000 B logical, 8 192 alloc
    /// ├── sparse.img  9 000 B logical, 4 096 alloc
    /// └── docs/
    ///     ├── a.txt   100 / 4 096
    ///     └── b.txt   200 / 4 096
    struct Sample {
        tree: crate::tree::ScanTree,
        big: NodeId,
        sparse: NodeId,
        docs: NodeId,
        a: NodeId,
        b: NodeId,
    }

    fn sample() -> Sample {
        let mut bld = builder();
        let big = push(&mut bld, ROOT, "big.bin", file(1_000, 8_192));
        let sparse = push(&mut bld, ROOT, "sparse.img", file(9_000, 4_096));
        let docs = push(&mut bld, ROOT, "docs", dir());
        let a = push(&mut bld, docs, "a.txt", file(100, 4_096));
        let b = push(&mut bld, docs, "b.txt", file(200, 4_096));
        Sample {
            tree: bld.finish(Duration::ZERO),
            big,
            sparse,
            docs,
            a,
            b,
        }
    }

    #[test]
    fn root_view_sorted_by_allocated() {
        let s = sample();
        let view = build_viewport(&s.tree, ROOT, &ViewportOptions::default()).unwrap();
        assert_eq!(view.generation, 3);
        assert_eq!(view.current, ROOT);
        assert_eq!(view.breadcrumbs, vec![ROOT]);
        assert_eq!(view.metric, SizeMetric::Allocated);
        // docs = 8 192 (ties with big.bin; docs has smaller logical 300 < 1 000).
        assert_eq!(ids(&view), vec![s.big, s.docs, s.sparse]);
        assert_eq!(
            view.items.iter().map(|i| i.value).collect::<Vec<_>>(),
            vec![8_192, 8_192, 4_096]
        );
        assert_eq!(view.total_value, 20_480);
        assert_eq!(view.other, None);
    }

    #[test]
    fn logical_metric_reorders() {
        let s = sample();
        let opts = ViewportOptions {
            metric: SizeMetric::Logical,
            ..ViewportOptions::default()
        };
        let view = build_viewport(&s.tree, ROOT, &opts).unwrap();
        assert_eq!(ids(&view), vec![s.sparse, s.big, s.docs]);
        assert_eq!(view.total_value, 10_300);
    }

    #[test]
    fn nested_view_has_breadcrumbs() {
        let s = sample();
        let view = build_viewport(&s.tree, s.docs, &ViewportOptions::default()).unwrap();
        assert_eq!(view.breadcrumbs, vec![ROOT, s.docs]);
        // Equal allocated size → larger logical first.
        assert_eq!(ids(&view), vec![s.b, s.a]);
        assert_eq!(view.total_value, 8_192);
    }

    #[test]
    fn caps_items_and_merges_rest_into_other() {
        let mut bld = builder();
        // 250 files with distinct sizes 1..=250 KiB.
        for i in 1..=250u64 {
            push(
                &mut bld,
                ROOT,
                &format!("f{i:03}.dat"),
                file(i * 1_000, i * 1_024),
            );
        }
        let tree = bld.finish(Duration::ZERO);
        let view = build_viewport(&tree, ROOT, &ViewportOptions::default()).unwrap();

        assert_eq!(view.items.len(), DEFAULT_MAX_ITEMS);
        let values: Vec<u64> = view.items.iter().map(|i| i.value).collect();
        let expected: Vec<u64> = (51..=250u64).rev().map(|i| i * 1_024).collect();
        assert_eq!(values, expected, "the 200 largest, descending");

        let other = view.other.expect("other bucket");
        assert_eq!(other.count, 50);
        let small: u64 = (1..=50u64).sum();
        assert_eq!(other.value, small * 1_024);
        assert_eq!(other.allocated_size, small * 1_024);
        assert_eq!(other.logical_size, small * 1_000);
        assert_eq!(other.file_count, 50);
        assert_eq!(other.newest_mtime, 1_000);

        let all: u64 = (1..=250u64).sum();
        assert_eq!(view.total_value, all * 1_024);
    }

    #[test]
    fn exactly_max_items_needs_no_other() {
        let mut bld = builder();
        for i in 0..5u64 {
            push(&mut bld, ROOT, &format!("f{i}"), file(i, i));
        }
        let tree = bld.finish(Duration::ZERO);
        let opts = ViewportOptions {
            max_items: 5,
            ..ViewportOptions::default()
        };
        let view = build_viewport(&tree, ROOT, &opts).unwrap();
        assert_eq!(view.items.len(), 5);
        assert_eq!(view.other, None);
    }

    #[test]
    fn zero_max_items_puts_everything_in_other() {
        let s = sample();
        let opts = ViewportOptions {
            max_items: 0,
            ..ViewportOptions::default()
        };
        let view = build_viewport(&s.tree, ROOT, &opts).unwrap();
        assert!(view.items.is_empty());
        let other = view.other.unwrap();
        assert_eq!(other.count, 3);
        assert_eq!(other.value, 20_480);
        assert_eq!(other.file_count, 4);
        assert_eq!(view.total_value, 20_480);
    }

    #[test]
    fn trashed_children_are_skipped() {
        let mut s = sample();
        s.tree.mark_trashed(s.big).unwrap();
        let view = build_viewport(&s.tree, ROOT, &ViewportOptions::default()).unwrap();
        assert_eq!(ids(&view), vec![s.docs, s.sparse]);
        assert_eq!(view.total_value, 12_288);
    }

    #[test]
    fn trashed_directory_and_its_descendants_are_refused() {
        let mut s = sample();
        s.tree.mark_trashed(s.docs).unwrap();
        let err = build_viewport(&s.tree, s.docs, &ViewportOptions::default()).unwrap_err();
        assert!(matches!(err, CoreError::NodeNotAllowed { .. }), "{err}");

        let mut bld = builder();
        let outer = push(&mut bld, ROOT, "outer", dir());
        let inner = push(&mut bld, outer, "inner", dir());
        push(&mut bld, inner, "x", file(1, 1));
        let mut tree = bld.finish(Duration::ZERO);
        tree.mark_trashed(outer).unwrap();
        let err = build_viewport(&tree, inner, &ViewportOptions::default()).unwrap_err();
        assert!(matches!(err, CoreError::NodeNotAllowed { .. }), "{err}");
    }

    #[test]
    fn hard_links_are_listed_with_zero_value() {
        let mut bld = builder();
        let original = push(&mut bld, ROOT, "original.bin", file(5_000, 8_192));
        let link = push(
            &mut bld,
            ROOT,
            "link.bin",
            EntryMeta {
                flags: NodeFlags::HARDLINK,
                ..file(5_000, 8_192)
            },
        );
        let tree = bld.finish(Duration::ZERO);
        let view = build_viewport(&tree, ROOT, &ViewportOptions::default()).unwrap();
        assert_eq!(ids(&view), vec![original, link]);
        assert_eq!(view.items[1].value, 0);
        assert_eq!(view.total_value, 8_192);
        // The link keeps its real size for display.
        assert_eq!(tree.node(link).unwrap().allocated_size, 8_192);
    }

    #[test]
    fn ties_are_ordered_by_name() {
        let mut bld = builder();
        push(&mut bld, ROOT, "c", file(10, 10));
        let a = push(&mut bld, ROOT, "a", file(10, 10));
        let b = push(&mut bld, ROOT, "b", file(10, 10));
        let tree = bld.finish(Duration::ZERO);
        let opts = ViewportOptions {
            max_items: 2,
            ..ViewportOptions::default()
        };
        let view = build_viewport(&tree, ROOT, &opts).unwrap();
        assert_eq!(ids(&view), vec![a, b]);
        assert_eq!(view.other.unwrap().count, 1);
    }

    #[test]
    fn empty_directory_has_no_items() {
        let mut bld = builder();
        let empty = push(&mut bld, ROOT, "empty", dir());
        let tree = bld.finish(Duration::ZERO);
        let view = build_viewport(&tree, empty, &ViewportOptions::default()).unwrap();
        assert!(view.items.is_empty());
        assert_eq!(view.other, None);
        assert_eq!(view.total_value, 0);
    }

    #[test]
    fn rejects_files_and_unknown_ids() {
        let s = sample();
        let err = build_viewport(&s.tree, s.a, &ViewportOptions::default()).unwrap_err();
        assert!(matches!(err, CoreError::NodeNotAllowed { .. }), "{err}");
        let err = build_viewport(&s.tree, 9_999, &ViewportOptions::default()).unwrap_err();
        assert!(matches!(err, CoreError::InvalidNode(9_999)), "{err}");
    }
}
