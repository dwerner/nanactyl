use std::os::unix::thread;
use std::time::{Duration, Instant};

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
    let mut stages = Vec::new();
    for splits in [4] {
        for n in [
            100, 1000, 10_000, 100_000, 200_000, 500_000, 5_000_000, 10_000_000,
        ] {
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
                for _ in 0..splits {
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
                    entities: n,
                    microseconds: Instant::now().duration_since(scoped_start).as_micros() as u64,
                }));
            }

            std::thread::sleep(Duration::from_millis(500));

            let mut exec = ThreadPoolExecutor::new(32);
            let start = Instant::now();
            let mut slice_partitions = vec![player_archetype.slice_mut()];
            for _ in 0..splits {
                slice_partitions = split_slices(slice_partitions);
            }
            let s = exec.scope_and_block(|scope| {
                for mut partition in slice_partitions {
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
                entities: n,
                microseconds: Instant::now().duration_since(start).as_micros() as u64,
            }));

            std::thread::sleep(Duration::from_millis(500));

            let start = Instant::now();
            run_sync_workload(&mut player_archetype);
            stages.push(Stage::Sync(Stats {
                splits,
                entities: n,
                microseconds: Instant::now().duration_since(start).as_micros() as u64,
            }));
        }
    }

    for stage in stages.iter() {
        match stage {
            Stage::ScopedAsync(stats) => println!(
                "scoped async workload with p = {} n = {} took {} microseconds",
                stats.splits, stats.entities, stats.microseconds
            ),
            Stage::ScopedThread(stats) => println!(
                "scoped thread workload with p = {} n = {} took {} microseconds",
                stats.splits, stats.entities, stats.microseconds
            ),
            Stage::Sync(stats) => println!(
                "sync workload with p = {} n = {} took {} microseconds",
                stats.splits, stats.entities, stats.microseconds
            ),
        }
    }

    {
        use plotters::prelude::*;
        let root = BitMapBackend::new("bench.png", (1280, 1024)).into_drawing_area();
        root.fill(&WHITE).unwrap();

        let mut chart = ChartBuilder::on(&root)
            .caption("Workload", ("sans-serif", 50))
            .margin(5)
            .x_label_area_size(30)
            .y_label_area_size(30)
            .build_cartesian_2d(0..10_000_000, 0..1_100_000)
            .unwrap();
        chart.configure_mesh().draw().unwrap();

        let sync_series = stages
            .iter()
            .filter_map(|stage| match stage {
                Stage::Sync(stats) => Some((stats.entities as i32, stats.microseconds as i32)),
                _ => None,
            })
            .collect::<Vec<_>>();
        chart
            .draw_series(LineSeries::new(sync_series, &GREEN))
            .unwrap()
            .label("sync")
            .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &GREEN));

        let async_scoped_series = stages
            .iter()
            .filter_map(|stage| match stage {
                Stage::ScopedAsync(stats) => {
                    Some((stats.entities as i32, stats.microseconds as i32))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        chart
            .draw_series(LineSeries::new(async_scoped_series, &BLUE))
            .unwrap()
            .label("async scoped")
            .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &BLUE));

        let thread_scoped_series = stages
            .iter()
            .filter_map(|stage| match stage {
                Stage::ScopedThread(stats) => {
                    Some((stats.entities as i32, stats.microseconds as i32))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        chart
            .draw_series(LineSeries::new(thread_scoped_series, &RED))
            .unwrap()
            .label("thread scoped")
            .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &RED));

        chart
            .configure_series_labels()
            .background_style(&WHITE.mix(0.8))
            .border_style(&BLACK)
            .draw()
            .unwrap();

        root.present().unwrap();
    }
}
