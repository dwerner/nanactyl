//! In support of ash_rendering_plugin, implements various wrappers over
//! vulkan/ash that are used in the plugin.
//!
//! This module is a landing-pad (In particular VulkanBase) for functionality
//! from

use std::sync::Arc;
use std::time::Instant;

use async_lock::Mutex;
use logger::{info, warn, LogLevel, Logger};
use platform::WinPtr;
use plugin_self::PluginState;
use world::thing::{CameraFacet, GraphicsFacet, GraphicsIndex, PhysicalFacet, ThingType};
use world::{Drawable, World};

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

    pub scene: RenderScene,

    /// Internal plugin state held by RenderState. Must be cleared between each
    /// update to the plugin, and unload called.
    ///
    /// Currently this *internal* state and the externally controlled "plugin"
    /// owned by the app conflict in name a bit.
    pub render_plugin_state: Option<Box<dyn RenderPluginState<State = Self> + Send + Sync>>,
}

impl RenderState {
    pub fn new(win_ptr: WinPtr, enable_validation_layer: bool, is_server: bool) -> Self {
        Self {
            updates: 0,
            win_ptr,
            render_plugin_state: None,
            enable_validation_layer,
            scene: RenderScene {
                active_camera: if is_server { 0 } else { 1 },
                cameras: vec![],
                drawables: vec![],
            },
            logger: LogLevel::Info.logger(),
        }
    }

    pub fn into_shared(self) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(self))
    }

    pub fn tracked_model(&mut self, index: GraphicsIndex) -> Option<Instant> {
        self.render_plugin_state
            .as_mut()
            .map(|plugin| plugin.tracked_model(index))
            .flatten()
    }

    pub fn update_scene(&mut self, scene: RenderScene) -> Result<(), SceneError> {
        // TODO: remove this copying and indirection from RenderState
        self.scene.drawables = scene.drawables;
        self.scene.cameras = scene.cameras;
        Ok(())
    }

    pub fn update_render_scene(&mut self, world: &World) -> Result<(), SceneError> {
        // TODO Fix hardcoded cameras.
        let c1 = world
            .get_camera_facet(0u32.into())
            .map_err(SceneError::World)?;

        let c2 = world
            .get_camera_facet(1u32.into())
            .map_err(SceneError::World)?;

        let cameras = vec![c1, c2];

        let mut drawables = vec![];
        for (_id, thing) in world.things().iter().enumerate() {
            let model_ref = match &thing.facets {
                ThingType::Camera { phys, camera } => world
                    .get_camera_drawable(phys, camera)
                    .map_err(SceneError::World)?,
                ThingType::GraphicsObject { phys, model } => {
                    world.get_drawable(phys, model).map_err(SceneError::World)?
                }
            };
            drawables.push(model_ref);
        }
        let active_camera = if world.is_server() { 0 } else { 1 };
        let scene = RenderScene {
            active_camera,
            cameras,
            drawables,
        };
        self.update_scene(scene)?;
        Ok(())
    }

    /// Search through the world for models that need to be uploaded, and do so.
    /// Does not yet handle updates to models.
    pub fn upload_untracked_graphics(&mut self, world: &World) {
        let drawables: Vec<_> = world.facets.gfx_iter().collect();
        // This needs to move to somewhere that owns the assets...
        for (index, graphic) in drawables {
            if let Some(_uploaded) = self.tracked_model(index) {
                // TODO: handle model updates
                // model already uploaded
                // } else if self.queued_model(index) {
                //     // model already queued for upload
            } else {
                info!(self.logger, "uploading graphic {:?}", index);
                match self.render_plugin_state.as_mut() {
                    // for now upload one at a time, with a barrier between each.
                    // we can also upload all at once, but this currently easier to debug.
                    Some(render_plugin_state) => {
                        render_plugin_state
                            .upload_graphics(&[(index, graphic)])
                            .expect("should upload graphic");
                    }
                    None => {
                        warn!(self.logger, "no render state to upload to");
                    }
                }
            }
        }
    }
}

/// Basic trait for calling into rendering functionality.
pub trait Presenter {
    fn present(&mut self, scene: &RenderScene);
    fn update_resources(&mut self);
    fn drop_resources(&mut self);

    /// Query for a tracked model.
    fn tracked_model(&mut self, index: GraphicsIndex) -> Option<Instant>;

    fn upload_graphics(
        &mut self,
        graphics: &[(GraphicsIndex, &GraphicsFacet)],
    ) -> Result<(), RenderStateError>;
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

/// Represents the constructed scene as references into world state.
pub struct RenderScene {
    // TODO: should this just be indices?
    pub active_camera: usize,
    pub cameras: Vec<(PhysicalFacet, CameraFacet)>,
    pub drawables: Vec<Drawable>,
}
