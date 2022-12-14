use std::time::Duration;

use world::thing::{CameraFacet, ModelFacet, PhysicalFacet, Thing};
use world::World;

#[no_mangle]
pub extern "C" fn load(world: &mut World) {
    let ico_model = models::Model::load(
        "assets/models/static/tank_smooth.obj",
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

    for _ in 0..2 {
        let physical = PhysicalFacet::new(0.0, 4.0, -10.0, &bounding_mesh);
        let mut camera_facet = CameraFacet::new(&physical);
        camera_facet.set_associated_model(ico_model_idx);

        let camera_idx = world.add_camera(camera_facet);
        let phys_idx = world.add_physical(physical);
        let camera = Thing::camera(phys_idx, camera_idx);

        let _camera_thing_id = world
            .add_thing(camera)
            .expect("unable to add thing to world");
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
            let mut physical = PhysicalFacet::new(x * 4.0, 2.0, z * 10.0, &bounding_mesh);
            physical.angles.y = j as f32 * 4.0;
            //physical.linear_velocity = Vector3::new(x, 0.0, z);
            physical.angular_velocity.y = 1.0;
            let physical_idx = world.add_physical(physical);
            let model_object = Thing::model(physical_idx, model_idx);
            world.add_thing(model_object).unwrap();
        }
    }

    {
        let arena_model = models::Model::load(
            "assets/models/static/arena.obj",
            "assets/shaders/vertex_rustgpu.spv",
            "assets/shaders/fragment_rustgpu.spv",
        )
        .unwrap();
        let arena_physical = PhysicalFacet::new(0.0, 0.0, 0.0, &arena_model.mesh);
        let model_facet = ModelFacet::new(arena_model);
        let arena_model_idx = world.add_model(model_facet);
        let arena_phys_idx = world.add_physical(arena_physical);
        let arena_thing = Thing::model(arena_phys_idx, arena_model_idx);
        world.add_thing(arena_thing).unwrap();
    }

    println!(
        "loaded asset loader plugin (updates {}) - models {}",
        world.updates,
        world.facets.model_iter().count()
    );
}

#[no_mangle]
pub extern "C" fn update(_world: &mut World, _dt: &Duration) {}

#[no_mangle]
pub extern "C" fn unload(world: &mut World) {
    world.clear();
    println!("unloaded asset loader plugin ({})", world.updates);
}
