use std::{collections::HashMap, time::Duration};

use ash::vk;

use sdl2_sys::SDL_Window;
use world::{Identifyable, World};

#[derive(Debug)]
pub struct Drawable {
    pub id: world::Identity,
    pub rendered: Duration,
}

#[derive(Debug, Default)]
pub struct RenderState {
    pub entities: Vec<Drawable>,
    pub updates: u64,
}

#[derive(Copy, Clone)]
pub struct WinPtr {
    pub raw: *const SDL_Window,
}

unsafe impl Send for WinPtr {}
unsafe impl Sync for WinPtr {}

pub struct VulkanRenderState {
    pub win_ptr: WinPtr,
    pub devices: Vec<vk::Device>,
    pub swapchain: vk::SwapchainKHR,
    pub renderpass: vk::RenderPass,
    pub attachments: vk::AttachmentReference, //?
    pub buffers: HashMap<String, vk::Buffer>,
}

impl RenderState {

    pub fn new() -> Self {
        Default::default()
    }

    pub async fn update(&mut self, world: &World) {
        self.updates += 1;
        self.entities.clear();
        let entities = &mut self.entities;
        let time = world.run_life.clone();
        for thing in world.get_things() {
            entities.push(Drawable {
                id: thing.identify(),
                rendered: time.clone(),
            })
        }
    }
}
