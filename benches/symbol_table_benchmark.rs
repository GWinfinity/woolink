//! Benchmark for woolink SymbolUniverse
//!
//! 测试项目：
//! - 符号插入性能
//! - 并发读取性能 (1000+ 线程)
//! - O(1) 定义跳转性能
//! - SoA vs AoS 缓存效率

use std::sync::Arc;
use std::thread;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use woolink::{Symbol, SymbolId, SymbolKind, SymbolUniverse, Visibility};

/// Create test symbols
fn create_test_symbols(count: usize) -> Vec<Symbol> {
    (0..count)
        .map(|i| Symbol {
            id: (i + 1) as u32,
            package_id: 1,
            kind: SymbolKind::Function,
            visibility: Visibility::Public,
            name_offset: (i * 10) as u32,
            name_len: 8,
            doc_offset: 0,
            doc_len: 0,
            signature_offset: 0,
            signature_len: 0,
            def_file_id: 1,
            def_offset: (i * 100) as u32,
            chain_next: 0,
        })
        .collect()
}

fn bench_symbol_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("symbol_insert");

    for size in [1000, 10000, 100000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let universe = SymbolUniverse::new(size);
                let symbols = create_test_symbols(size);

                {
                    let mut guard = universe.write();
                    for sym in symbols {
                        let _ = guard.insert_symbol(sym);
                    }
                }
            });
        });
    }

    group.finish();
}

fn bench_symbol_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("symbol_lookup");

    // Setup universe with symbols
    let universe = SymbolUniverse::new(10000);
    let symbols = create_test_symbols(10000);

    {
        let mut guard = universe.write();
        for sym in &symbols {
            let _ = guard.insert_symbol(sym.clone());
        }
    }

    group.throughput(Throughput::Elements(1));
    group.bench_function("single_lookup", |b| {
        b.iter(|| {
            let guard = universe.read();
            let sym = guard.get_symbol(SymbolId::new(5000));
            black_box(sym);
        });
    });

    group.finish();
}

fn bench_concurrent_reads(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_reads");

    for thread_count in [4, 8, 16, 32].iter() {
        let universe = Arc::new(SymbolUniverse::new(10000));
        let symbols = create_test_symbols(10000);

        {
            let mut guard = universe.write();
            for sym in &symbols {
                let _ = guard.insert_symbol(sym.clone());
            }
        }

        group.bench_function(BenchmarkId::new("threads", thread_count), |b| {
            b.iter(|| {
                let handles: Vec<_> = (0..*thread_count)
                    .map(|_| {
                        let u = Arc::clone(&universe);
                        thread::spawn(move || {
                            for i in 0..100 {
                                let guard = u.read();
                                let sym = guard.get_symbol(SymbolId::new((i % 10000 + 1) as u32));
                                black_box(sym);
                            }
                        })
                    })
                    .collect();

                for h in handles {
                    h.join().unwrap();
                }
            });
        });
    }

    group.finish();
}

fn bench_definition_jump(c: &mut Criterion) {
    let mut group = c.benchmark_group("definition_jump");

    // Setup with chain: 1 -> 2 -> 3 -> 4 (terminal)
    let universe = SymbolUniverse::new(100);

    {
        let mut guard = universe.write();

        for i in 1..=4 {
            let sym = Symbol {
                id: i,
                package_id: 1,
                kind: SymbolKind::Function,
                visibility: Visibility::Public,
                name_offset: 0,
                name_len: 4,
                doc_offset: 0,
                doc_len: 0,
                signature_offset: 0,
                signature_len: 0,
                def_file_id: 1,
                def_offset: i * 100,
                chain_next: 0,
            };
            guard.insert_symbol(sym).unwrap();
        }

        // Create chain
        guard
            .link_alias(SymbolId::new(1), SymbolId::new(2))
            .unwrap();
        guard
            .link_alias(SymbolId::new(2), SymbolId::new(3))
            .unwrap();
        guard
            .link_alias(SymbolId::new(3), SymbolId::new(4))
            .unwrap();
    }

    group.throughput(Throughput::Elements(1));
    group.bench_function("jump_4_steps", |b| {
        b.iter(|| {
            let guard = universe.read();
            let result = guard.jump_to_definition(SymbolId::new(1));
            let _ = black_box(result);
        });
    });

    group.bench_function("direct_lookup", |b| {
        b.iter(|| {
            let guard = universe.read();
            let sym = guard.get_symbol(SymbolId::new(4));
            black_box(sym);
        });
    });

    group.finish();
}

fn bench_memory_layout(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_layout");

    // Test SoA vs AoS (simulated)
    let universe = SymbolUniverse::new(100000);
    let symbols = create_test_symbols(100000);

    {
        let mut guard = universe.write();
        for sym in &symbols {
            let _ = guard.insert_symbol(sym.clone());
        }
    }

    group.throughput(Throughput::Elements(10000));
    group.bench_function("sequential_scan", |b| {
        b.iter(|| {
            let guard = universe.read();
            let mut sum = 0u32;
            for i in 1..=10000 {
                if let Some(sym) = guard.get_symbol(SymbolId::new(i)) {
                    sum = sum.wrapping_add(sym.id);
                }
            }
            black_box(sum);
        });
    });

    group.bench_function("random_access", |b| {
        b.iter(|| {
            let guard = universe.read();
            let mut sum = 0u32;
            for i in (1..=10000).step_by(7) {
                if let Some(sym) = guard.get_symbol(SymbolId::new(i)) {
                    sum = sum.wrapping_add(sym.def_offset);
                }
            }
            black_box(sum);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_symbol_insert,
    bench_symbol_lookup,
    bench_concurrent_reads,
    bench_definition_jump,
    bench_memory_layout
);
criterion_main!(benches);
