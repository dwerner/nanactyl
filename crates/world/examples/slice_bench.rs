use std::time::Instant;

use core_executor::scoped::ScopedThreadPoolExecutor;
use core_executor::ThreadPoolExecutor;
use world::archetypes::player::{PlayerArchetype, PlayerBuilder, PlayerSliceMut};
use world::archetypes::Archetype;

fn run_sync_workload(player_archetype: &mut PlayerArchetype) {
    let mut player_slice = player_archetype.slice_mut();
    for mut player in player_slice.iter_mut() {
        update_player(&mut player);
    }
}

fn cpu_float() -> f32 {
    rand::random::<f32>() * rand::random::<f32>()
}

fn update_player(player: &mut world::archetypes::player::PlayerRef) {
    player.angles.x += cpu_float();
    player.angles.y = cpu_float();
    player.angles.z = cpu_float();
    player.health.hp += 1;
    player.linear_velocity_intention.x += cpu_float();
    player.linear_velocity_intention.y = cpu_float();
    player.linear_velocity_intention.z += cpu_float();
    player.angular_velocity_intention.x = cpu_float();
    player.angular_velocity_intention.y += cpu_float();
    player.angular_velocity_intention.z = cpu_float();
    player.pos.x += cpu_float();
    player.pos.y = cpu_float();
    player.pos.z += cpu_float();
    *player.perspective = *player.perspective * *player.view;
    *player.scale = cpu_float();
}

fn main() {
    println!("running bench");
    let max_threads = 32;
    let p = 2;
    for n in [1000, 10_000, 100_000, 200_000, 500_000, 5_000_000] {
        let mut player_archetype = PlayerArchetype::with_capacity(n);
        let mut builder = PlayerBuilder::default();
        builder.set_gfx(world::graphics::GfxIndex(42));
        player_archetype.set_default_builder(builder);
        let builder = player_archetype.builder();

        for e in 0..n {
            if e % 10000 == 0 {
                //println!("creating entity {}", e);
            }
            player_archetype.spawn(e as u32, builder.clone()).unwrap();
        }

        /// TODO: move this kind of thing into the slice interface
        fn split_slices<'a>(slices: Vec<PlayerSliceMut<'a>>) -> Vec<PlayerSliceMut<'a>> {
            let mut new_slices = Vec::new();
            for slice in slices {
                assert_eq!(slice.len(), slice.angles.len());
                let len = slice.len();
                let (left, right) = slice.split_at_mut(len / 2);
                new_slices.push(left);
                new_slices.push(right);
            }
            new_slices
        }

        {
            let mut slice_partitions = vec![player_archetype.slice_mut()];
            for _ in 0..p {
                slice_partitions = split_slices(slice_partitions);
            }

            let scoped_start = Instant::now();
            std::thread::scope(|scope| {
                let mut executor = ScopedThreadPoolExecutor::new(max_threads, scope);
                let mut futures = Vec::new();
                let start = Instant::now();
                for (index, partition) in slice_partitions.iter_mut().enumerate() {
                    let future = executor.spawners()[index].spawn(async move {
                        for mut player in partition.iter_mut() {
                            update_player(&mut player);
                        }
                    });
                    futures.push(future);
                }
                let work = futures_util::future::join_all(futures);
                futures_lite::future::block_on(work);
                println!(
                    "async workload with p = {}, n = {} took {} microseconds",
                    p,
                    n,
                    Instant::now().duration_since(start).as_micros()
                );
            });
            println!(
                "(scope end) async workload with p = {}, n = {} took {} microseconds",
                p,
                n,
                Instant::now().duration_since(scoped_start).as_micros()
            );
        }

        {
            let mut pool_exec = ThreadPoolExecutor::new(max_threads);
            let mut slice_partitions = vec![player_archetype.slice_mut()];
            for _ in 0..p {
                slice_partitions = split_slices(slice_partitions);
            }

            for mut partition in slice_partitions {
                pool_exec.spawn_on_any(async move {
                    for mut player in partition.iter_mut() {
                        update_player(&mut player);
                    }
                });
            }
        }

        let start = Instant::now();
        run_sync_workload(&mut player_archetype);
        println!(
            "sync workload with n = {} took {} microseconds",
            n,
            Instant::now().duration_since(start).as_micros()
        );
    }
}
