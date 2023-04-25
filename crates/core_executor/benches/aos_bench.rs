use async_std::task;
use core_executor::ThreadPoolExecutor;
use criterion::{black_box, criterion_group, criterion_main, Criterion};

#[derive(Clone)]
struct Data<const PADDING: usize> {
    value: u64,
    _pad: [u8; PADDING],
}

// Modify process_structs to take a generic Vec<T> and a closure as arguments
fn process_structs<T, F>(data_array: &mut Vec<T>, mut mutate_fn: F)
where
    F: FnMut(&mut T),
{
    for data in data_array.iter_mut() {
        mutate_fn(data);
    }
}

async fn workload_task<const PADDING: usize>(iterations: u64) {
    let mut data_array: Vec<Data<PADDING>> = vec![
        Data {
            value: 0,
            _pad: [0u8; PADDING]
        };
        iterations as usize
    ];
    let mutate_data = |data: &mut Data<PADDING>| {
        data.value += 1;
    };

    process_structs(&mut data_array, mutate_data);
}

async fn run_workload<const PADDING: usize>(executor: &mut ThreadPoolExecutor, iterations: u64) {
    let (_core, future) = executor.spawn_on_any(workload_task::<PADDING>(iterations));
    future.await.unwrap();
}

fn workload_benchmark(c: &mut Criterion) {
    println!("ran bench");
    let mut group = c.benchmark_group("workload_bench");
    let cores = 8;
    let mut executor = ThreadPoolExecutor::new(cores);

    for iterations in [1000, 10000, 100000] {
        const SIZE1: usize = 1024;
        group.bench_function(format!("workload_aos_padded_{SIZE1}_{iterations}"), |b| {
            b.iter(|| task::block_on(run_workload::<SIZE1>(&mut executor, black_box(iterations))))
        });
        const SIZE2: usize = 2048;
        group.bench_function(format!("workload_aos_padded_{SIZE2}_{iterations}"), |b| {
            b.iter(|| task::block_on(run_workload::<SIZE2>(&mut executor, black_box(iterations))))
        });
        const SIZE4: usize = 4096;
        group.bench_function(format!("workload_aos_padded_{SIZE4}_{iterations}"), |b| {
            b.iter(|| task::block_on(run_workload::<SIZE4>(&mut executor, black_box(iterations))))
        });
    }
}

criterion_group!(benches, workload_benchmark);
criterion_main!(benches);
