//! Plugin: `tui_renderer_plugin`
//! Implements a plugin for prototyping terminal UI rendering for the engine.
//! Just a placeholder but this could be fun.

use std::time::Duration;

use logger::info;
use render::RenderState;

#[no_mangle]
pub extern "C" fn load(state: &mut RenderState) {
    info!(state.logger.sub("tui-render-load"), "loaded tui_renderer");
}

#[no_mangle]
pub extern "C" fn update(state: &mut RenderState, dt: &Duration) {
    // Call render, buffers are updated etc
    if state.updates % 600 == 0 {
        info!(
            state.logger.sub("tui-render-update"),
            "tui state: {} dt: {:?}", state.updates, dt
        );
    }
}

#[no_mangle]
pub extern "C" fn unload(state: &mut RenderState) {
    info!(
        state.logger.sub("tui-render-unload"),
        "unloaded tui_renderer"
    );
}
