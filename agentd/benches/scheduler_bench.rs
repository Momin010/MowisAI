use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::sync::Arc;
use std::time::Duration;

// Mock benchmark for scheduler performance
fn scheduler_benchmark(c: &mut Criterion) {
    c.bench_function("scheduler_dispatch", |b| {
        b.iter(|| {
            // Mock scheduler dispatch operation
            let task_count = black_box(1000);
            let mut total = 0;
            for i in 0..task_count {
                total += i;
            }
            black_box(total)
        })
    });
}

fn agent_execution_benchmark(c: &mut Criterion) {
    c.bench_function("agent_execution", |b| {
        b.iter(|| {
            // Mock agent execution
            let iterations = black_box(100);
            let mut result = 0;
            for i in 0..iterations {
                result += i * 2;
            }
            black_box(result)
        })
    });
}

criterion_group!(benches, scheduler_benchmark, agent_execution_benchmark);
criterion_main!(benches);