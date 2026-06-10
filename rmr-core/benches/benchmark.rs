use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::fs::File;
use std::io::Write;
use rmr_core::files::analyzer::analyze_gcode;

fn benchmark_analyzer(c: &mut Criterion) {
    let filepath = "test_file.gcode";
    {
        let mut file = File::create(filepath).unwrap();
        for i in 0..10000 {
            writeln!(file, "G1 X10 Y10 Z{}", i).unwrap();
            if i % 100 == 0 {
                writeln!(file, ";LAYER:{}", i / 100).unwrap();
            }
        }
    }

    c.bench_function("analyze_gcode", |b| {
        b.iter(|| analyze_gcode(black_box(filepath)))
    });

    std::fs::remove_file(filepath).unwrap();
}

criterion_group!(benches, benchmark_analyzer);
criterion_main!(benches);