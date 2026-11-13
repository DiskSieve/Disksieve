//! Scanner throughput on a generated tree: 100 directories × 100 small files.
//!
//! Run with `cargo bench -p disksieve-core --bench scanner`. Results depend heavily on the
//! OS file cache; the first iteration warms it.

use std::fs;
use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use disksieve_core::scanner::{scan, CancelToken, ScanOptions};

const DIRS: usize = 100;
const FILES_PER_DIR: usize = 100;

fn bench_scan(c: &mut Criterion) {
    let tmp = tempfile::tempdir().expect("temp dir");
    for d in 0..DIRS {
        let dir = tmp.path().join(format!("dir{d:03}"));
        fs::create_dir(&dir).expect("create dir");
        for f in 0..FILES_PER_DIR {
            fs::write(dir.join(format!("file{f:03}.bin")), [0u8; 512]).expect("write file");
        }
    }

    let mut group = c.benchmark_group("scanner");
    group.sample_size(10);
    group.throughput(Throughput::Elements((DIRS * (FILES_PER_DIR + 1)) as u64));
    group.bench_function("scan_10k_entries", |b| {
        b.iter(|| {
            let tree = scan(
                tmp.path(),
                &ScanOptions::default(),
                &CancelToken::new(),
                |_| {},
            )
            .expect("scan");
            black_box(tree.len())
        })
    });
    group.finish();
}

criterion_group!(benches, bench_scan);
criterion_main!(benches);
