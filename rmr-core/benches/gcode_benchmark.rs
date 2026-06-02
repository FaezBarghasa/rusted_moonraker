use criterion::{criterion_group, criterion_main, Criterion};

fn bench_gcode_parsing(c: &mut Criterion) {
    c.bench_function("dummy_bench", |b| b.iter(|| 1 + 1));
}

criterion_group!(benches, bench_gcode_parsing);
criterion_main!(benches);
