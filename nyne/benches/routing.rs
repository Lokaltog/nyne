//! Routing tree benchmarks — captures baseline performance for route matching.
//!
//! Run: `cargo bench -p nyne -- --save-baseline pre`
//! Compare: `cargo bench -p nyne -- --baseline pre`

// Benches are external to the crate, so we can only access `pub` items.
// The routing module is pub(crate), so we benchmark through the public
// re-exports that exist, or test the tree indirectly.
//
// For now, this file benchmarks VfsPath::segments() (the hot path in
// route matching) and serves as the harness for future benchmarks
// once RouteTree is used by providers.

use criterion::{Criterion, criterion_group, criterion_main};
use nyne::types::VfsPath;

fn bench_vfs_path_segments(c: &mut Criterion) {
    let mut group = c.benchmark_group("vfs_path");

    let shallow = VfsPath::new("file.rs@/symbols").unwrap();
    let deep = VfsPath::new("dir/file.rs@/symbols/Foo@/callers").unwrap();

    group.bench_function("segments_depth_2", |b| {
        b.iter(|| shallow.segments());
    });

    group.bench_function("segments_depth_5", |b| {
        b.iter(|| deep.segments());
    });

    group.bench_function("components_iter_depth_5", |b| {
        b.iter(|| deep.components().count());
    });

    group.finish();
}

criterion_group!(benches, bench_vfs_path_segments);
criterion_main!(benches);
