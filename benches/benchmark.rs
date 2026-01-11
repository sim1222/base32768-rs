use criterion::{Criterion, criterion_group, criterion_main};
use rand::{Rng as _, SeedableRng, rngs::StdRng};
use std::hint::black_box;

fn criterion_benchmark(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(789);

    // let decoder = base32768::FastDecoder::new(
    let decoder = base32768::LudicrousDecoder::new(
        base32768_table::Z15_REPERTOIRE,
        base32768_table::Z7_REPERTOIRE,
    );

    let mut group = c.benchmark_group("base32768");

    group.throughput(criterion::Throughput::Bytes(1));
    group.bench_function("encoderOneByte", |b| {
        b.iter_batched(
            || {
                let mut data = [0u8; 1];
                rng.fill(&mut data[..]);
                data
            },
            |data| {
                black_box(base32768::encode(black_box(&data)));
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.throughput(criterion::Throughput::Bytes(10_000));
    group.bench_function("encoderTenKilobytes", |b| {
        b.iter_batched(
            || {
                let mut data = [0u8; 10_000];
                rng.fill(&mut data[..]);
                data
            },
            |data| {
                black_box(base32768::encode(black_box(&data)));
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.throughput(criterion::Throughput::Bytes(1_000_000));
    group.bench_function("encoderOneMegabyte", |b| {
        b.iter_batched(
            || {
                let mut data = [0u8; 1_000_000];
                rng.fill(&mut data[..]);
                data
            },
            |data| {
                black_box(base32768::encode(black_box(&data)));
            },
            criterion::BatchSize::LargeInput,
        );
    });

    group.throughput(criterion::Throughput::Bytes(1));
    group.bench_function("decoderOneByte", |b| {
        b.iter_batched(
            || {
                let mut data = [0u8; 1];
                rng.fill(&mut data[..]);
                base32768::encode(&data)
            },
            |data| {
                black_box(decoder.decode(black_box(&data)));
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.throughput(criterion::Throughput::Bytes(10_000));
    group.bench_function("decoderTenKilobytes", |b| {
        b.iter_batched(
            || {
                let mut data = [0u8; 10_000];
                rng.fill(&mut data[..]);
                base32768::encode(&data)
            },
            |data| {
                black_box(decoder.decode(black_box(&data)));
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.throughput(criterion::Throughput::Bytes(1_000_000));
    group.bench_function("decoderOneMegabyte", |b| {
        b.iter_batched(
            || {
                let mut data = [0u8; 1_000_000];
                rng.fill(&mut data[..]);
                base32768::encode(&data)
            },
            |data| {
                black_box(decoder.decode(black_box(&data)));
            },
            criterion::BatchSize::LargeInput,
        );
    });

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
