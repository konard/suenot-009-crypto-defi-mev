use criterion::{black_box, criterion_group, criterion_main, Criterion};
use crypto_defi_mev::arbitrage::{numerical_search, TriangularArb};
use crypto_defi_mev::sandwich::{SandwichSimulator, VictimTrade};
use crypto_defi_mev::uniswap_v2::ConstantProductPool;

fn bench_two_pool_arb(c: &mut Criterion) {
    let a = ConstantProductPool::new(1_000.0, 100_000.0, 0.003).unwrap();
    let b = ConstantProductPool::new(1_000.0, 110_000.0, 0.003).unwrap();
    c.bench_function("two_pool_numerical_search", |bencher| {
        bencher.iter(|| numerical_search(black_box(&a), black_box(&b)));
    });
}

fn bench_triangular_arb(c: &mut Criterion) {
    let xy = ConstantProductPool::new(1_000.0, 2_000.0, 0.003).unwrap();
    let yz = ConstantProductPool::new(2_000.0, 4_100.0, 0.003).unwrap();
    let zx = ConstantProductPool::new(4_000.0, 1_000.0, 0.003).unwrap();
    let tri = TriangularArb { xy: &xy, yz: &yz, zx: &zx };
    c.bench_function("triangular_arb_optimal", |bencher| {
        bencher.iter(|| tri.optimal_input(black_box(500.0)));
    });
}

fn bench_sandwich(c: &mut Criterion) {
    let pool = ConstantProductPool::new(1_000.0, 1_000_000.0, 0.003).unwrap();
    let v = VictimTrade { max_input_x: 50.0, min_output_y: 30_000.0 };
    let sim = SandwichSimulator { gas_cost_x: 0.0 };
    c.bench_function("sandwich_optimal", |bencher| {
        bencher.iter(|| sim.optimal(black_box(&pool), black_box(v)));
    });
}

criterion_group!(benches, bench_two_pool_arb, bench_triangular_arb, bench_sandwich);
criterion_main!(benches);
