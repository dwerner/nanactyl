use std::time::Duration;

use render::WorldRenderState;

// The responsibility of this plugin is to update the RenderState struct from the World.
// This work needs to lock the World and RenderState.

#[no_mangle]
pub extern "C" fn load(state: &mut WorldRenderState) {
    println!(
        "loaded world render update plugin ({})",
        state.world().updates
    );
    let camera_idx = state.world().maybe_camera.unwrap();
    let camera_thing = state.world().thing_as_ref(camera_idx).unwrap();
    // random gdc guy from amd: "Shoud provide a 'more declarative api' rather than hand back buffers"...
    let render_state = state.render_state();

    for (id, model) in state.world().facets.models.iter().enumerate() {
        println!("would need to check/upload model + material {id} {model:?}");
    }
}

#[no_mangle]
pub extern "C" fn update(state: &mut WorldRenderState, _dt: &Duration) {}

#[no_mangle]
pub extern "C" fn unload(state: &mut WorldRenderState) {
    println!(
        "unloaded world render update plugin ({})",
        state.world().updates
    );
}
