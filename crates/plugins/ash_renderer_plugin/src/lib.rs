use std::time::Duration;

use render::{RenderState, VulkanBase, Present};

struct Presenter {

}

impl Present for Presenter {
    fn present(&self) {
        //println!("presented something... Ha HAA");
    }
}

#[no_mangle]
pub extern "C" fn load(state: &mut RenderState) {
    println!("loaded ash_renderer_plugin");
    println!("set up some vulkan state!");

    state.vulkan.base = Some(VulkanBase::new(state.win_ptr.clone()));
    state.vulkan.presenter = Some(Box::pin(Presenter{}));

}

#[no_mangle]
pub extern "C" fn update(state: &mut RenderState, dt: &Duration) {
    // Call render, buffers are updated etc
    if state.updates % 600 == 0 {
        println!("state: {} dt: {:?}", state.updates, dt);
    }
    if let Some(ref mut present) = state.vulkan.presenter {
        present.present();
    }
}

#[no_mangle]
pub extern "C" fn unload(state: &mut RenderState) {
    println!("unloaded ash_renderer_plugin");
    state.vulkan.presenter.take();
    state.vulkan.base.take();
}
