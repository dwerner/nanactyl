use std::time::Duration;

use gfx::Model;
use logger::info;
use world::thing::{CameraFacet, GraphicsFacet, PhysicalFacet, Thing};
use world::AssetLoaderStateAndWorldLock;

// TODO:
// - convert this to be a StatefulPlugin
// - reload models and shaders when they change on disk
// |--> this will require a way to signal to the renderer that it needs to
// |--> dealloc and reload related buffers
#[no_mangle]
pub extern "C" fn load(state: &mut AssetLoaderStateAndWorldLock) {
    let world = &mut state.world;
    let logger = &world.logger.sub("asset-loader");

    let ico_model = Model::load_obj(
        "assets/models/static/tank.obj",
        "assets/shaders/spv/default_vertex.spv",
        "assets/shaders/spv/default_fragment.spv",
    )
    .unwrap();
    let ico_model_facet = GraphicsFacet::from_model(ico_model);
    let ico_model_idx = world.add_graphics(ico_model_facet);

    let cube_model = Model::load_obj(
        "assets/models/static/cube.obj",
        "assets/shaders/spv/default_vertex.spv",
        "assets/shaders/spv/default_fragment.spv",
    )
    .unwrap();
    let cube_model_facet = GraphicsFacet::from_model(cube_model);

    let cube_model_facet = cube_model_facet.into_debug_mesh();

    let cube_model_idx = world.add_graphics(cube_model_facet);

    for (x, z) in [(10.0, 10.0), (-10.0, -10.0)].into_iter() {
        info!(logger, "adding player camera object at {}, {}", x, z);
        let physical = PhysicalFacet::new_cuboid(x, 0.0, z, 1.0);
        let mut camera_facet = CameraFacet::new(&physical);
        camera_facet.set_associated_model(cube_model_idx);

        let camera_idx = world.add_camera(camera_facet);
        let phys_idx = world.add_physical(physical);
        let camera = Thing::camera(phys_idx, camera_idx);

        let camera_thing_id = world
            .add_thing(camera)
            .expect("unable to add thing to world");
        info!(logger, "added camera thing: {:?}", camera_thing_id)
    }

    // initialize some state, lots of model_object entities
    for i in -4..4i32 {
        for j in -6..6i32 {
            let model_idx = if (i + j) % 2 == 0 {
                cube_model_idx
            } else {
                ico_model_idx
            };
            let (x, z) = (i as f32, j as f32);
            let mut physical = PhysicalFacet::new_cuboid(x * 4.0, 2.0, z * 10.0, 1.0);
            physical.angles.y = j as f32 * 4.0;
            physical.angular_velocity_intention.y = 1.0;
            let physical_idx = world.add_physical(physical);
            let model_object = Thing::model(physical_idx, model_idx);
            world.add_thing(model_object).unwrap();
        }
    }

    let sky_model = Model::load_obj(
        "assets/models/static/skybox.obj",
        "assets/shaders/spv/skybox_vertex.spv",
        "assets/shaders/spv/skybox_fragment.spv",
    )
    .unwrap();
    let sky_phys = PhysicalFacet::new_cuboid(0.0, 0.0, 0.0, 200.0);
    let graphics = GraphicsFacet::from_model(sky_model);
    let sky_model_idx = world.add_graphics(graphics);
    let sky_phys_idx = world.add_physical(sky_phys);
    let thing = Thing::model(sky_phys_idx, sky_model_idx);
    world.add_thing(thing).unwrap();

    info!(
        logger,
        "loaded asset loader plugin (updates {}) - models {})",
        world.stats.updates,
        world.facets.gfx_iter().count()
    );
}

#[no_mangle]
pub extern "C" fn update(_state: &mut AssetLoaderStateAndWorldLock, _dt: &Duration) {}

#[no_mangle]
pub extern "C" fn unload(state: &mut AssetLoaderStateAndWorldLock) {
    state.world.clear();
    info!(
        state.world.logger,
        "unloaded asset loader plugin ({})", state.world.stats.updates
    );
}
