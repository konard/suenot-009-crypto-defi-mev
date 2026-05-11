use criterion::{black_box, criterion_group, criterion_main, Criterion};
use crypto_defi_mev::uniswap_v2::{int_math, ConstantProductPool};
use crypto_defi_mev::uniswap_v3::{ConcentratedPool, Tick};
use crypto_defi_mev::curve::StableSwapPool;

fn bench_v2_swap(c: &mut Criterion) {
    let pool = ConstantProductPool::new(1_000_000.0, 1_000_000.0, 0.003).unwrap();
    c.bench_function("v2_out_given_in_float", |b| {
        b.iter(|| pool.out_given_in_x_to_y(black_box(1_000.0)).unwrap())
    });
    c.bench_function("v2_out_given_in_u128", |b| {
        b.iter(|| {
            int_math::out_given_in(
                black_box(1_000_000_000_000),
                black_box(1_000_000_000_000),
                black_box(1_000_000),
                30,
            )
        })
    });
}

fn bench_v3_swap(c: &mut Criterion) {
    let mut pool = ConcentratedPool::new(
        Tick { price_lower: 1_000.0, price_upper: 4_000.0, liquidity: 1e6 },
        2_000.0,
        0.003,
    );
    c.bench_function("v3_swap_x_to_y", |b| {
        b.iter(|| {
            let _ = pool.swap_x_to_y(black_box(0.001));
        })
    });
}

fn bench_stable_swap(c: &mut Criterion) {
    let pool = StableSwapPool::new(1_000_000.0, 1_000_000.0, 100.0, 0.0004);
    c.bench_function("stableswap_out_given_in", |b| {
        b.iter(|| pool.out_given_in_x_to_y(black_box(1_000.0)).unwrap())
    });
}

criterion_group!(benches, bench_v2_swap, bench_v3_swap, bench_stable_swap);
criterion_main!(benches);
