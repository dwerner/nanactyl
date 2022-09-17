use std::time::Duration;

use world::{
    thing::{CameraFacet, ModelFacet, PhysicalFacet, Thing},
    Vector3, World,
};

#[no_mangle]
pub extern "C" fn load(world: &mut World) {
    let ico_model = models::Model::load(
        "assets/models/static/ico.obj",
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
    let cube_model_facet = ModelFacet::new(cube_model);
    let cube_model_idx = world.add_model(cube_model_facet);

    let physical = PhysicalFacet::new(0.0, 0.0, 0.0);
    let camera_idx = world.add_camera(CameraFacet::new(&physical));
    let phys_idx = world.add_physical(physical);
    let camera = Thing::camera(phys_idx, camera_idx);
    let camera_thing_id = world
        .add_thing(camera)
        .expect("unable to add thing to world.");

    // TODO: special purpose hooks for object ids that are relevant?
    world.maybe_camera = Some(camera_thing_id);

    // initialize some state, lots of model_object entities
    for x in -5..5i32 {
        for y in -5..5i32 {
            for z in -5..5i32 {
                let model_idx = if (x + y) % 2 == 0 {
                    cube_model_idx
                } else {
                    ico_model_idx
                };
                let (x, y, z) = (x as f32, y as f32, z as f32);
                let mut physical = PhysicalFacet::new(x * 4.0, y * 4.0, z * 10.0);
                physical.linear_velocity = Vector3::new(1.0, 1.0, 1.0);
                let physical_idx = world.add_physical(physical);
                let model_object = Thing::model_object(physical_idx, model_idx);
                world.add_thing(model_object).unwrap();
            }
        }
    }

    println!(
        "loaded asset loader plugin (updates {}) - models {}",
        world.updates,
        world.facets.model_iter().count()
    );
}

#[no_mangle]
pub extern "C" fn update(world: &mut World, dt: &Duration) {
    world.maybe_tick(dt);
}

#[no_mangle]
pub extern "C" fn unload(world: &mut World) {
    world.clear();
    println!("unloaded asset loader plugin ({})", world.updates);
}
