//! In support of ash_rendering_plugin, implements various wrappers over
//! vulkan/ash that are used in the plugin.
//!
//! This module is a landing-pad (In particular VulkanBase) for functionality
//! from

use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_lock::{Mutex, MutexGuardArc};
use logger::{LogLevel, Logger};
use platform::WinPtr;
use plugin_self::PluginState;
use world::thing::{CameraFacet, CameraIndex, ModelIndex, PhysicalFacet, PhysicalIndex};
use world::{Identity, Vec3, World};

#[derive(Debug)]
pub struct Drawable {
    pub id: world::Identity,
    pub rendered: Duration,
}

#[derive(thiserror::Error, Debug)]
pub enum RenderStateError {
    #[error("plugin error {0:?}")]
    PluginError(Box<dyn std::error::Error + Send + Sync>),
    #[error("model upload error")]
    ModelUploadTODO,
}

#[derive(thiserror::Error, Debug)]
pub enum SceneError {
    #[error("query error {0}")]
    Query(#[from] SceneQueryError),
    #[error("world error {0:?}")]
    World(world::WorldError),
}

#[derive(thiserror::Error, Debug)]
pub enum SceneQueryError {
    #[error("thing with id {0:?} not found in scene")]
    ThingNotFound(Identity),
    #[error("no phys facet at index {0:?}")]
    NoSuchPhys(PhysicalIndex),
    #[error("no camera facet at index {0:?}")]
    NoSuchCamera(CameraIndex),
}

/// "Declarative" style api attempt - don't expose any renderer details/buffers,
/// instead have RenderState track them
pub struct RenderState {
    pub updates: u64,
    pub win_ptr: WinPtr,
    pub enable_validation_layer: bool,
    pub model_upload_queue: VecDeque<(ModelIndex, models::Model)>,
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
            model_upload_queue: Default::default(),
            logger: LogLevel::Info.logger(),
        }
    }

    pub fn into_shared(self) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(self))
    }

    /// Queue a model for upload to the GPU. This is done by adding it to a
    /// queue on RenderState, and from there it is uploaded at a plugin-defined
    /// point.
    pub fn queue_model_for_upload(
        &mut self,
        index: ModelIndex,
        model: models::Model,
    ) -> Result<(), RenderStateError> {
        self.model_upload_queue.push_front((index, model));
        Ok(())
    }

    pub fn drain_upload_queue(&mut self) -> VecDeque<(ModelIndex, models::Model)> {
        std::mem::take(&mut self.model_upload_queue)
    }

    pub fn tracked_model(&mut self, index: ModelIndex) -> Option<Instant> {
        self.render_plugin_state
            .as_mut()
            .map(|plugin| plugin.tracked_model(index))
            .flatten()
    }

    pub fn queued_model(&self, index: ModelIndex) -> bool {
        self.model_upload_queue
            .iter()
            .any(|(queued_idx, _)| index == *queued_idx)
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
                world::thing::ThingType::Camera { phys, camera } => {
                    let phys = world
                        .facets
                        .physical(*phys)
                        .ok_or(SceneQueryError::NoSuchPhys(*phys))?;
                    let cam = world
                        .facets
                        .camera(*camera)
                        .ok_or(SceneQueryError::NoSuchCamera(*camera))?;

                    let right = cam.right(phys);
                    let forward = cam.forward(phys);
                    let pos =
                        phys.position + Vec3::new(right.x + forward.x, -2.0, right.z + forward.z);
                    let angles = Vec3::new(0.0, phys.angles.y - 1.57, 0.0);

                    SceneModelInstance {
                        model: cam.associated_model.unwrap(),
                        pos,
                        angles,
                        scale: phys.scale,
                    }
                }
                world::thing::ThingType::ModelObject { phys, model } => {
                    let facet = world
                        .facets
                        .physical(*phys)
                        .ok_or(SceneQueryError::NoSuchPhys(*phys))?;

                    SceneModelInstance {
                        model: *model,
                        pos: facet.position,
                        angles: facet.angles,
                        scale: facet.scale,
                    }
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
    pub fn update_models(&mut self, world: &World) {
        let models: Vec<_> = {
            world
                .facets
                .model_iter()
                .map(|(index, model)| (index, model.clone()))
                .collect()
        };
        // This needs to move to somewhere that owns the assets...
        for (index, model) in models {
            if let Some(_uploaded) = self.tracked_model(index) {
                // TODO: handle model updates
                // model already uploaded
            } else if self.queued_model(index) {
                // model already queued for upload
            } else {
                self.queue_model_for_upload(index, model)
                    .expect("should upload");
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
    fn tracked_model(&mut self, index: ModelIndex) -> Option<Instant>;
    // TODO: upload_model, other resource management
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
    pub drawables: Vec<SceneModelInstance>,
}

/// Reference to a model and for now positional and orientation data.
/// Intended to represent a model (uploaded to the GPU once) with instance
/// information. Should attach to a game object or similar.
pub struct SceneModelInstance {
    pub model: ModelIndex,
    pub pos: Vec3,
    pub angles: Vec3,
    pub scale: f32,
}
