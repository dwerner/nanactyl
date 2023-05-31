//! In support of ash_rendering_plugin, implements various wrappers over
//! vulkan/ash that are used in the plugin.
//!
//! This module is a landing-pad (In particular VulkanBase) for functionality
//! from

use std::sync::Arc;
use std::time::Instant;

use async_lock::Mutex;
use gfx::Graphic;
use logger::{info, trace, warn, LogLevel, Logger};
use platform::WinPtr;
use plugin_self::PluginState;
use world::components::GraphicPrefab;
use world::{Entity, World};

#[derive(thiserror::Error, Debug)]
pub enum RenderStateError {
    #[error("plugin error {0:?}")]
    PluginError(Box<dyn std::error::Error + Send + Sync>),
    #[error("model upload error")]
    ModelUpload,
    #[error("vulkan base doesn't exist. Is a renderer set up?")]
    NoVulkanBase,
}

#[derive(thiserror::Error, Debug)]
pub enum SceneError {
    #[error("world error {0:?}")]
    World(world::WorldError),
}

/// "Declarative" style api attempt - don't expose any renderer details/buffers,
/// instead have RenderState track them
pub struct RenderState {
    pub updates: u64,
    pub win_ptr: WinPtr,
    pub enable_validation_layer: bool,
    pub logger: Logger,
}

impl RenderState {
    pub fn new(
        win_ptr: WinPtr,
        enable_validation_layer: bool,
        is_server: bool,
        logger: Logger,
    ) -> Self {
        Self {
            updates: 0,
            win_ptr,
            enable_validation_layer,
            logger,
        }
    }

    pub fn into_shared(self) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(self))
    }

    /// Search through the world for models that need to be uploaded, and do so.
    /// Does not yet handle updates to models.
    pub fn upload_untracked_graphics_prefabs(
        &mut self,
        world: &World,
        mut plugin: Option<&mut Box<dyn RenderPluginState<GameState = RenderState> + Send + Sync>>,
    ) {
        for (entity, graphic) in world.heks_world.query::<&GraphicPrefab>().iter() {
            match plugin.as_mut() {
                Some(plugin) => {
                    if let Some(uploaded_at) = plugin.tracked_graphics(entity) {
                        trace!(
                            self.logger,
                            "graphic {:?} already tracked for {}ms",
                            entity,
                            Instant::now().duration_since(uploaded_at).as_millis()
                        );
                    } else {
                        info!(self.logger, "uploading graphic {:?}", entity);

                        plugin
                            .upload_graphics(&[(entity, &graphic.gfx)])
                            .expect("unable to upload graphics");
                    }
                }
                None => {
                    warn!(self.logger, "no render state to upload to");
                }
            }
        }
    }
}

/// Basic trait for calling into rendering functionality.
pub trait Presenter {
    fn present(&mut self, world: &World);
    fn update_resources(&mut self);
    fn deallocate(&mut self);

    /// Query for a tracked drawable.
    fn tracked_graphics(&self, entity: Entity) -> Option<Instant>;

    fn upload_graphics(&mut self, graphics: &[(Entity, &Graphic)]) -> Result<(), RenderStateError>;
}

/// A trait for the loader side to call into the renderer side.
pub trait RenderPluginState: PluginState + Presenter {}

#[derive(Debug, Copy, Clone)]
pub struct TextureId(u32);

impl TextureId {
    pub fn new(value: u32) -> Self {
        Self(value)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum TextureUploaderError {
    #[error("queue send error")]
    QueueSend,
    #[error("queue send error")]
    QueueRecv,
}
