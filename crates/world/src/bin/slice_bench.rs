use core_executor::scoped::ScopedThreadPoolExecutor;
use world::archetypes::player::{PlayerArchetype, PlayerBuilder};
use world::archetypes::Archetype;

async fn run_async_workload<'scope, 'arch>(
    executor: &'scope mut ScopedThreadPoolExecutor<'scope>,
    player_archetype: &'arch mut PlayerArchetype,
    // num_partitions: usize,
) where
    'arch: 'scope,
{
    let mut slice_partitions = Vec::new();

    let player_slice = player_archetype.slice_mut();
    // let slice_size = player_slice.len() / num_partitions;
    let player_slice_len = player_slice.len();
    let (left, right) = player_slice.split_at_mut(player_slice_len / 2);
    slice_partitions.push(left);
    slice_partitions.push(right);

    // TODO: split and stuff into vec
    // let mut remaining = &player_slice;
    // for _ in 0..num_partitions {
    //     let (tip, rest) = remaining.split_at_mut(slice_size);
    //     remaining = rest;
    //     slice_partitions.push(tip);
    // }

    let mut futures = Vec::new();
    for mut partition in slice_partitions {
        let future = executor.spawn_on_any(async move {
            for player in partition.iter_mut() {
                player.angles.x += 1.0;
            }
        });
        futures.push(future);
    }
    futures_util::future::join_all(futures).await;
}

async fn run_sync_workload(player_archetype: &mut PlayerArchetype) {
    let mut player_slice = player_archetype.slice_mut();
    for player in player_slice.iter_mut() {
        player.angles.x += 1.0;
    }
}

fn main() {
    println!("ran bench");
    let cores = 8;
    let mut player_archetype = PlayerArchetype::default();

    let builder = PlayerBuilder::default();
    player_archetype.set_default_builder(builder);

    const N: u32 = 10000;
    for e in 0..N {
        let builder = player_archetype.builder();
        player_archetype.spawn(e, builder.clone()).unwrap();
    }
    let mut player_archetype2 = player_archetype.clone();
    std::thread::scope(|scope| {
        let mut executor = ScopedThreadPoolExecutor::new(cores, scope);

        let work = run_async_workload(&mut executor, &mut player_archetype);
        futures_lite::future::block_on(work);
    });

    run_sync_workload(&mut player_archetype2);
}
