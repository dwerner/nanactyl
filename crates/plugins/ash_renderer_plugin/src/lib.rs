use std::time::Duration;

use render::RenderState;

#[no_mangle]
pub extern "C" fn load(_state: &mut RenderState) {
    println!("loaded ash_renderer_plugin");
}

#[no_mangle]
pub extern "C" fn update(state: &mut RenderState, dt: &Duration) {
    // Call render, buffers are updated etc
    if state.updates % 600 == 0 {
        println!("state: {} dt: {:?}", state.entities.len(), dt);
    }
}

#[no_mangle]
pub extern "C" fn unload(_state: &mut RenderState) {
    println!("unloaded ash_renderer_plugin");
}

