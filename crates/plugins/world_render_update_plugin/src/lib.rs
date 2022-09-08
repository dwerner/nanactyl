use std::time::Duration;

use render::LockWorldAndRenderState;

#[no_mangle]
pub extern "C" fn load(state: &mut LockWorldAndRenderState) {
    println!(
        "loaded world render update plugin ({})",
        state.world().updates
    );
    let camera_idx = state.world().maybe_camera.unwrap();
    let _camera_thing = state.world().thing_as_ref(camera_idx).unwrap();
    // random gdc guy from amd: "Shoud provide a 'more declarative api' rather than hand back buffers"...
}

#[no_mangle]
pub extern "C" fn update(_state: &mut LockWorldAndRenderState, _dt: &Duration) {}

#[no_mangle]
pub extern "C" fn unload(state: &mut LockWorldAndRenderState) {
    println!(
        "unloaded world render update plugin ({})",
        state.world().updates
    );
}
