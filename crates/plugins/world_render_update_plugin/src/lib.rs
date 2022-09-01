use std::time::Duration;

use render::{RenderState, WorldRenderState};

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
pub extern "C" fn update(state: &mut WorldRenderState, dt: &Duration) {
    let render_state = state.render_state();
}

#[no_mangle]
pub extern "C" fn unload(state: &mut WorldRenderState) {
    println!(
        "unloaded world render update plugin ({})",
        state.world().updates
    );
}
