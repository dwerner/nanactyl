use std::time::Duration;

use render::{LockWorldAndRenderState, RenderScene, SceneError, SceneModelRef};
use world::Vector3;

#[no_mangle]
pub extern "C" fn load(state: &mut LockWorldAndRenderState) {
    println!(
        "loaded world render update plugin ({})",
        state.world().updates
    );
    update_render_scene(state).unwrap();
    // random gdc guy from amd: "Shoud provide a 'more declarative api' rather than hand back buffers"...
}

#[no_mangle]
pub extern "C" fn update(state: &mut LockWorldAndRenderState, _dt: &Duration) {
    state.update_models();
    update_render_scene(state).unwrap();
}

#[no_mangle]
pub extern "C" fn unload(state: &mut LockWorldAndRenderState) {
    println!(
        "unloaded world render update plugin ({})",
        state.world().updates
    );
}

pub fn update_render_scene(zelf: &mut LockWorldAndRenderState) -> Result<(), SceneError> {
    // TODO Fix hardcoded cameras.
    let c1 = zelf
        .world()
        .get_camera_facet(0u32.into())
        .map_err(SceneError::World)?;
    let c2 = zelf
        .world()
        .get_camera_facet(1u32.into())
        .map_err(SceneError::World)?;

    let cameras = vec![c1, c2];
    let mut drawables = vec![];

    for (id, thing) in zelf.world().things().iter().enumerate() {
        let model_ref = match &thing.facets {
            world::thing::ThingType::Camera { phys, camera } => {
                let phys = zelf
                    .world()
                    .facets
                    .physical(*phys)
                    .ok_or_else(|| SceneError::NoSuchPhys(*phys))?;
                let cam = zelf
                    .world()
                    .facets
                    .camera(*camera)
                    .ok_or_else(|| SceneError::NoSuchCamera(*camera))?;

                // For Now: position a model with an offset to the camera.
                let right = cam.right(phys);
                let forward = cam.forward(phys);
                let pos =
                    phys.position + Vector3::new(right.x + forward.x, -2.0, right.z + forward.z);
                let angles = Vector3::new(0.0, phys.angles.y - 1.57, 0.0);

                SceneModelRef {
                    model: cam.associated_model.unwrap(),
                    pos,
                    angles,
                }
            }
            world::thing::ThingType::ModelObject { phys, model } => {
                let facet = zelf
                    .world()
                    .facets
                    .physical(*phys)
                    .ok_or_else(|| SceneError::NoSuchPhys(*phys))?;

                SceneModelRef {
                    model: *model,
                    pos: facet.position.clone(),
                    angles: facet.angles.clone(),
                }
            }
        };
        drawables.push(model_ref);
    }
    let active_camera = if zelf.world().is_server() { 0 } else { 1 };
    let scene = RenderScene {
        active_camera,
        cameras,
        drawables,
    };
    zelf.render_state().update_scene(scene)?;
    Ok(())
}
