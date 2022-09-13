use std::time::Duration;

use render::LockWorldAndRenderState;

#[no_mangle]
pub extern "C" fn load(state: &mut LockWorldAndRenderState) {
    println!(
        "loaded world render update plugin ({})",
        state.world().updates
    );
    state.update_render_scene().unwrap();
    // random gdc guy from amd: "Shoud provide a 'more declarative api' rather than hand back buffers"...
}

#[no_mangle]
pub extern "C" fn update(state: &mut LockWorldAndRenderState, _dt: &Duration) {
    state.update_render_scene().unwrap();
}

#[no_mangle]
pub extern "C" fn unload(state: &mut LockWorldAndRenderState) {
    println!(
        "unloaded world render update plugin ({})",
        state.world().updates
    );
}
