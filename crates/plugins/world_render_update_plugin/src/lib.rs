use std::time::Duration;

use render::WorldRenderState;

// The responsibility of this plugin is to update the RenderState struct from the World.
// This work needs to lock the World and RenderState.

#[no_mangle]
pub extern "C" fn load(state: &mut WorldRenderState) {
    println!(
        "loaded world render update plugin ({})!\nby design world is readonly, and render_state is mutable",
        state.world().updates
    );
}

#[no_mangle]
pub extern "C" fn update(state: &mut WorldRenderState, _dt: &Duration) {

    // random gdc guy from amd: "Shoud provide a 'more declarative api' rather than hand back buffers"...
    let _render_state = state.render_state();
}

#[no_mangle]
pub extern "C" fn unload(state: &mut WorldRenderState) {
    println!(
        "unloaded world render update plugin ({})",
        state.world().updates
    );
}
