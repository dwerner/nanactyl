use std::time::Duration;

use render::RenderState;

#[no_mangle]
pub extern "C" fn load(state: &mut RenderState) {
    println!("loaded tui_renderer");
}

#[no_mangle]
pub extern "C" fn update(state: &mut RenderState, dt: &Duration) {
    if state.updates % 1000 == 0 {
        println!("state: {:?} dt: {:?}", state, dt);
    }
}

#[no_mangle]
pub extern "C" fn unload(state: &mut RenderState) {
    println!("unloaded tui_renderer");
}
