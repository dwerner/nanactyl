use std::time::{Duration, Instant};

use core_executor::scoped::ScopedThreadPoolExecutor;
use core_executor::ThreadPoolExecutor;
use glam::{Mat4, Vec3};
use hecs::World;
use world::graphics::Shape;
use world::health::HealthFacet;

fn run_sync_workload(player_slice: &mut [Player]) {
    for mut player in player_slice.iter_mut() {
        update_player(&mut player);
    }
}

fn cpu_float() -> f32 {
    rand::random::<f32>() * rand::random::<f32>()
}

struct Player {
    pos: Vec3,
    angles: Vec3,
    scale: f32,
    view: Mat4,
    perspective: Mat4,
    linear_velocity_intention: Vec3,
    angular_velocity_intention: Vec3,
    shape: Shape,
    health: HealthFacet,
}
impl Player {
    fn new() -> Self {
        let perspective = Mat4::perspective_lh(
            1.7,    //aspect
            0.75,   //fovy
            0.1,    // near
            1000.0, //far
        );
        Player {
            pos: Vec3::ZERO,
            view: Mat4::IDENTITY,
            perspective,
            angles: Vec3::ZERO,
            scale: 1.0,
            linear_velocity_intention: Vec3::ZERO,
            angular_velocity_intention: Vec3::ZERO,
            shape: Shape::cuboid(1.0, 1.0, 1.0),
            health: HealthFacet::new(100),
        }
    }
}

fn update_player(player: &mut Player) {
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
    player.perspective = player.perspective * player.view;
    player.scale = cpu_float();
}

#[derive(Clone)]
struct Stats {
    splits: usize,
    entities: usize,
    microseconds: u64,
}

#[derive(Clone)]
enum Stage {
    Sync(Stats),
    ScopedThread(Stats),
    ScopedAsync(Stats),
}

fn main() {
    println!("running bench");
    let max_threads = 32;
    let mut world = World::new();
    let mut runs = Vec::new();
    for num_entities in [100, 1000, 10_000, 100_000, 200_000] {
        let mut stages = Vec::new();
        for splits in [1, 2, 3, 4, 5] {
            for e in 0..num_entities {
                if e % 10000 == 0 {
                    //println!("creating entity {}", e);
                }
                world.spawn((Player::new(),));
            }

            /// TODO: move this kind of thing into the slice interface
            fn split_slices(slices: Vec<&mut [Player]>) -> Vec<&mut [Player]> {
                let mut new_slices = Vec::new();
                for slice in slices {
                    let len = slice.len();
                    let (left, right) = slice.split_at_mut(len / 2);
                    new_slices.push(left);
                    new_slices.push(right);
                }
                new_slices
            }

            {
                println!("running threaded workload");
                let mut archetypes = world.archetypes();
                let _empty = archetypes.next().unwrap();
                let arch = archetypes.next().unwrap();
                let mut player_column = arch.get::<&mut Player>().unwrap();

                // It turns out there is a way to get a column and split it in hecs.
                let mid = player_column.len() / 2;
                let (left, right) = player_column.split_at_mut(mid);
                let mut slice_partitions = vec![left, right];
                for _ in 1..splits {
                    slice_partitions = split_slices(slice_partitions);
                }

                let scoped_start = Instant::now();
                std::thread::scope(|scope| {
                    let mut executor = ScopedThreadPoolExecutor::new(max_threads, scope);
                    let mut futures = Vec::new();
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
                });
                stages.push(Stage::ScopedThread(Stats {
                    splits,
                    entities: num_entities,
                    microseconds: Instant::now().duration_since(scoped_start).as_micros() as u64,
                }));
            }

            std::thread::sleep(Duration::from_millis(500));

            {
                println!("running async workload");
                let mut exec = ThreadPoolExecutor::new(32);
                let start = Instant::now();
                let mut archetypes = world.archetypes();
                let _empty = archetypes.next().unwrap();
                let arch = archetypes.next().unwrap();
                let mut player_column = arch.get::<&mut Player>().unwrap();
                let mid = player_column.len() / 2;
                let (left, right) = player_column.split_at_mut(mid);
                let mut slice_partitions = vec![left, right];
                for _ in 1..splits {
                    slice_partitions = split_slices(slice_partitions);
                }

                let s = exec.scope_and_block(|scope| {
                    for partition in slice_partitions {
                        scope.spawn(async move {
                            for mut player in partition.iter_mut() {
                                update_player(&mut player);
                            }
                            partition.len()
                        });
                    }
                    42
                });
                stages.push(Stage::ScopedAsync(Stats {
                    splits,
                    entities: num_entities,
                    microseconds: Instant::now().duration_since(start).as_micros() as u64,
                }));
            }
        }

        std::thread::sleep(Duration::from_millis(500));
        {
            println!("running single threaded workload");
            let start = Instant::now();
            let mut archetypes = world.archetypes();
            // the first archetype in the world is always empty
            let _empty = archetypes.next().unwrap();
            let arch = archetypes.next().unwrap();
            let mut player_column = arch.get::<&mut Player>().unwrap();
            run_sync_workload(&mut *player_column);

            stages.push(Stage::Sync(Stats {
                splits: 0,
                entities: num_entities,
                microseconds: Instant::now().duration_since(start).as_micros() as u64,
            }));
        }
        runs.push(stages);
    }

    println!("stage, partitions, entities, time in micros");
    for stages in runs.iter() {
        for stage in stages.iter() {
            match stage {
                Stage::ScopedAsync(stats) => {
                    println!(
                        "scoped-async,{},{},{}",
                        stats.splits, stats.entities, stats.microseconds
                    )
                }
                Stage::ScopedThread(stats) => {
                    println!(
                        "scoped-thread, {},{},{}",
                        stats.splits, stats.entities, stats.microseconds
                    )
                }
                Stage::Sync(stats) => {
                    println!(
                        "sync,{},{},{}",
                        stats.splits, stats.entities, stats.microseconds
                    )
                }
            }
        }
    }
}
