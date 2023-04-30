use std::time::Duration;

use logger::info;
use world::thing::{CameraFacet, ModelFacet, PhysicalFacet, Thing};
use world::AssetLoaderStateAndWorldLock;

#[no_mangle]
pub extern "C" fn load(state: &mut AssetLoaderStateAndWorldLock) {
    let world = &mut state.world;
    let logger = &world.logger.sub("asset-loader");
    let ico_model = models::Model::load(
        "assets/models/static/tank.obj",
        "assets/shaders/vertex_rustgpu.spv",
        "assets/shaders/fragment_rustgpu.spv",
    )
    .unwrap();
    let ico_model_facet = ModelFacet::new(ico_model);
    let ico_model_idx = world.add_model(ico_model_facet);

    let cube_model = models::Model::load(
        "assets/models/static/cube.obj",
        "assets/shaders/vertex_rustgpu.spv",
        "assets/shaders/fragment_rustgpu.spv",
    )
    .unwrap();

    let (bounding_mesh, _) = models::Mesh::load("assets/models/static/cube.obj").unwrap();
    let cube_model_facet = ModelFacet::new(cube_model);
    let cube_model_idx = world.add_model(cube_model_facet);

    for (x, z) in [(10.0, 10.0), (-10.0, -10.0)].into_iter() {
        info!(logger, "adding object at {}, {}", x, z);
        let physical = PhysicalFacet::new(x, 0.0, z, 1.0, &bounding_mesh);
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
                //cube_model_idx
            };
            let (x, z) = (i as f32, j as f32);
            let mut physical = PhysicalFacet::new(x * 4.0, 2.0, z * 10.0, 1.0, &bounding_mesh);
            physical.angles.y = j as f32 * 4.0;
            //physical.linear_velocity = Vector3::new(x, 0.0, z);
            physical.angular_velocity.y = 1.0;
            let physical_idx = world.add_physical(physical);
            let model_object = Thing::model(physical_idx, model_idx);
            world.add_thing(model_object).unwrap();
        }
    }

    {
        let sky_model = models::Model::load(
            "assets/models/static/skybox.obj",
            "assets/shaders/skybox_vertex.spv",
            "assets/shaders/skybox_fragment.spv",
        )
        .unwrap();
        let sky_phys = PhysicalFacet::new(0.0, 0.0, 0.0, 200.0, &sky_model.mesh);
        let model_facet = ModelFacet::new(sky_model);
        let sky_model_idx = world.add_model(model_facet);
        let sky_phys_idx = world.add_physical(sky_phys);
        let thing = Thing::model(sky_phys_idx, sky_model_idx);
        world.add_thing(thing).unwrap();
    }

    // {
    //     let arena_model = models::Model::load(
    //         "assets/models/static/arena.obj",
    //         "assets/shaders/vertex_rustgpu.spv",
    //         "assets/shaders/fragment_rustgpu.spv",
    //     )
    //     .unwrap();
    //     let arena_physical = PhysicalFacet::new(0.0, 0.0, 0.0, 1.0,
    // &arena_model.mesh);     let model_facet = ModelFacet::new(arena_model);
    //     let arena_model_idx = world.add_model(model_facet);
    //     let arena_phys_idx = world.add_physical(arena_physical);
    //     let arena_thing = Thing::model(arena_phys_idx, arena_model_idx);
    //     world.add_thing(arena_thing).unwrap();
    // }

    info!(
        world.logger,
        "loaded asset loader plugin (updates {}) - models {})",
        world.updates,
        world.facets.model_iter().count()
    );
}

#[no_mangle]
pub extern "C" fn update(_state: &mut AssetLoaderStateAndWorldLock, _dt: &Duration) {}

#[no_mangle]
pub extern "C" fn unload(state: &mut AssetLoaderStateAndWorldLock) {
    state.world.clear();
    info!(
        state.world.logger,
        "unloaded asset loader plugin ({})", state.world.updates
    );
}
