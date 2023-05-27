use std::time::Duration;

use gfx::{Graphic, Model};
use logger::info;
use world::bundles::{Player, StaticObject};
use world::components::Spatial;
use world::{AssetLoaderStateAndWorldLock, Vec3};

// TODO:
// - convert this to be a StatefulPlugin
// - reload models and shaders when they change on disk
// |--> this will require a way to signal to the renderer that it needs to
// |--> dealloc and reload related buffers
#[no_mangle]
pub extern "C" fn load(state: &mut AssetLoaderStateAndWorldLock) {
    let world = &mut state.world;
    let logger = &world.logger.sub("asset-loader");

    let tank_model = Model::load_obj(
        "assets/models/static/tank.obj",
        "assets/shaders/spv/default_vertex.spv",
        "assets/shaders/spv/default_fragment.spv",
    )
    .unwrap();
    let tank_gfx = world.add_model(tank_model);

    let cube_model = Model::load_obj(
        "assets/models/static/cube.obj",
        "assets/shaders/spv/default_vertex.spv",
        "assets/shaders/spv/default_fragment.spv",
    )
    .unwrap();
    let cube_gfx = world.add_model(cube_model);

    for (x, z) in [(10.0, 10.0), (-10.0, -10.0)].into_iter() {
        info!(logger, "adding player camera object at {}, {}", x, z);
        let pos = Vec3::new(x, 0.0, z);
        let tank = Player::new(world.root, tank_gfx, Spatial::new_at(pos));
        let tank_id = world.add_player(tank);
        info!(logger, "added tank: {:?}", tank_id)
    }

    // initialize some state, lots of model_object entities
    for i in -4..4i32 {
        for j in -4..4i32 {
            let model_prefab = if (i + j) % 2 == 0 { tank_gfx } else { cube_gfx };
            let (x, z) = (i as f32, j as f32);
            let object = StaticObject::new(
                world.root,
                model_prefab,
                Spatial::new_at(Vec3::new(x * 4.0, 2.0, z * 10.0)).with_angles(Vec3::new(
                    0.0,
                    j as f32 * 4.0,
                    0.0,
                )),
            );
            world.hecs_world.spawn(object);
        }
    }

    let sky_model = Model::load_obj(
        "assets/models/static/skybox.obj",
        "assets/shaders/spv/skybox_vertex.spv",
        "assets/shaders/spv/skybox_fragment.spv",
    )
    .unwrap();

    let sky_prefab = world.add_model(sky_model);
    let sky = StaticObject::new(world.root, sky_prefab, Spatial::new_with_scale(200.0));
    world.hecs_world.spawn(sky);
}

#[no_mangle]
pub extern "C" fn update(_state: &mut AssetLoaderStateAndWorldLock, _dt: &Duration) {}

#[no_mangle]
pub extern "C" fn unload(state: &mut AssetLoaderStateAndWorldLock) {
    state.world.hecs_world.clear();
    info!(
        state.world.logger,
        "unloaded asset loader plugin ({})", state.world.stats.updates
    );
}
