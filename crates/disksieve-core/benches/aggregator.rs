//! Viewport projection cost. Target (spec 2.3): < 16 ms for any directory.
//!
//! Run with `cargo bench -p disksieve-core --bench aggregator`. The tree is built in memory,
//! so no filesystem access is involved.

use std::ffi::OsStr;
use std::hint::black_box;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use disksieve_core::{
    build_viewport, EntryMeta, NodeFlags, SizeMetric, TreeBuilder, ViewportOptions, ROOT,
};

fn flat_tree(children: u64) -> disksieve_core::ScanTree {
    let dir = EntryMeta {
        flags: NodeFlags::DIR,
        ..EntryMeta::default()
    };
    let mut b = TreeBuilder::new("/bench", 1, dir);
    b.reserve(children as usize);
    for i in 0..children {
        // Pseudo-random sizes so the selection is not trivially pre-sorted by logical size.
        let size = i.wrapping_mul(2_654_435_761) % 10_000_000;
        let meta = EntryMeta {
            logical_size: size,
            allocated_size: size.div_ceil(4096) * 4096,
            mtime: 0,
            flags: NodeFlags::empty(),
        };
        b.push(ROOT, OsStr::new(&format!("file{i}.bin")), meta)
            .expect("push");
    }
    b.finish(Duration::ZERO)
}

fn bench_viewport(c: &mut Criterion) {
    let mut group = c.benchmark_group("viewport");
    group.sample_size(20);
    for children in [1_000u64, 100_000, 1_000_000] {
        let tree = flat_tree(children);
        for metric in [SizeMetric::Allocated, SizeMetric::Logical] {
            let opts = ViewportOptions {
                metric,
                ..ViewportOptions::default()
            };
            group.bench_with_input(
                BenchmarkId::new(format!("{metric:?}"), children),
                &tree,
                |b, tree| b.iter(|| black_box(build_viewport(tree, ROOT, &opts).expect("view"))),
            );
        }
    }
    group.finish();
}

criterion_group!(benches, bench_viewport);
criterion_main!(benches);
