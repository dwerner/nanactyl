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
use logger::{info, warn, LogLevel, Logger};
use platform::WinPtr;
use plugin_self::StatefulPlugin;
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
    pub render_plugin: Option<Box<dyn RenderPlugin<State = RenderState> + Send + Sync>>,
    pub enable_validation_layer: bool,
    model_upload_queue: VecDeque<(ModelIndex, models::Model)>,
    pub scene: RenderScene,
    pub logger: Logger,
}

impl RenderState {
    pub fn new(win_ptr: WinPtr, enable_validation_layer: bool, is_server: bool) -> Self {
        Self {
            updates: 0,
            win_ptr,
            render_plugin: None,
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
        self.render_plugin
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
        self.scene.drawables = scene.drawables;
        self.scene.cameras = scene.cameras;
        Ok(())
    }
}

// TODO - replace with Box<dyn RenderPlugin>
#[derive(Default)]
pub struct VulkanRendererState {
    pub presenter: Option<Box<dyn Presenter + Send + Sync>>,
}

/// Basic trait for calling into rendering functionality.
pub trait Presenter {
    fn present(&mut self, scene: &RenderScene);
    fn update_resources(&mut self);
    fn drop_resources(&mut self);

    fn tracked_model(&mut self, index: ModelIndex) -> Option<Instant>;
    // TODO: upload_model, other resource management
}

/// A plugin that is also a Presenter.
pub trait RenderPlugin: StatefulPlugin + Presenter {}

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

/// Acts as a combiner for Mutex, locking both mutexes but also releases both
/// mutexes when dropped.
pub struct LockWorldAndRenderState {
    world: MutexGuardArc<World>,
    render_state: MutexGuardArc<RenderState>,
}

impl LockWorldAndRenderState {
    pub fn update_render_scene(&mut self) -> Result<(), SceneError> {
        // TODO Fix hardcoded cameras.
        let c1 = self
            .world()
            .get_camera_facet(0u32.into())
            .map_err(SceneError::World)?;
        let c2 = self
            .world()
            .get_camera_facet(1u32.into())
            .map_err(SceneError::World)?;

        let cameras = vec![c1, c2];
        let mut drawables = vec![];

        for (_id, thing) in self.world().things().iter().enumerate() {
            let model_ref = match &thing.facets {
                world::thing::ThingType::Camera { phys, camera } => {
                    let phys = self
                        .world()
                        .facets
                        .physical(*phys)
                        .ok_or(SceneQueryError::NoSuchPhys(*phys))?;
                    let cam = self
                        .world()
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
                    let facet = self
                        .world()
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
        let active_camera = if self.world().is_server() { 0 } else { 1 };
        let scene = RenderScene {
            active_camera,
            cameras,
            drawables,
        };
        self.render_state().update_scene(scene)?;
        Ok(())
    }

    /// Search through the world for models that need to be uploaded, and do so.
    /// Does not yet handle updates to models.
    pub fn update_models(&mut self) {
        let models: Vec<_> = {
            let world = self.world();
            world
                .facets
                .model_iter()
                .map(|(index, model)| (index, model.clone()))
                .collect()
        };
        // This needs to move to somewhere that owns the assets...
        for (index, model) in models {
            if let Some(_uploaded) = self.render_state().tracked_model(index) {
                // TODO: handle model updates
                // model already uploaded
            } else if self.render_state().queued_model(index) {
                // model already queued for upload
            } else {
                self.render_state()
                    .queue_model_for_upload(index, model)
                    .expect("should upload");
            }
        }
    }

    /// Locks the world and render state so that the renderstate may be updated
    /// from the world.
    pub async fn lock(world: &Arc<Mutex<World>>, render_state: &Arc<Mutex<RenderState>>) -> Self {
        let world = Arc::clone(world).lock_arc().await;
        let render_state = Arc::clone(render_state).lock_arc().await;
        Self {
            world,
            render_state,
        }
    }

    pub fn world(&self) -> &World {
        self.world.deref()
    }

    pub fn render_state(&mut self) -> &mut RenderState {
        self.render_state.deref_mut()
    }
}
