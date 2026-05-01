use backend::api::handlers::profiling::get_metrics;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tokio::runtime::Runtime;

fn bench_metrics_handler(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    c.bench_function("get_metrics_handler", |b| {
        b.to_async(&rt).iter(|| async {
            let _ = get_metrics().await;
            black_box(())
        });
    });
}

criterion_group!(benches, bench_metrics_handler);
criterion_main!(benches);
