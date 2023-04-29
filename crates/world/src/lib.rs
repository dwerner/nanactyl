//! Implements a world and entity system for the engine to mutate and render.

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_lock::{Mutex, MutexGuardArc};
use input::wire::InputState;
use logger::{LogLevel, Logger};
use models::Model;
use network::{Connection, RpcError};
use scene::Scene;
use thing::{
    CameraFacet, CameraIndex, HealthFacet, HealthIndex, ModelFacet, ModelIndex, PhysicalFacet,
    PhysicalIndex, Thing, ThingType,
};

mod scene;
pub mod thing;
mod tree;

pub use glam::{Mat4, Quat, Vec3};

#[repr(C)]
pub struct WorldLockAndControllerState {
    pub world: MutexGuardArc<World>,
    pub controller_state: MutexGuardArc<[InputState; 2]>,
    pub logger: Logger,
}

impl WorldLockAndControllerState {
    /// Locks the world and render state so that the renderstate may be updated
    /// from the world.
    pub async fn lock(
        world: &Arc<Mutex<World>>,
        controller_state: &Arc<Mutex<[InputState; 2]>>,
    ) -> Self {
        let world = Arc::clone(world).lock_arc().await;
        let controller_state = Arc::clone(controller_state).lock_arc().await;
        Self {
            world,
            controller_state,
            logger: LogLevel::Info.logger(),
        }
    }
}

#[derive(Default)]
pub struct AssetLoaderState {
    pub watched: Vec<PathBuf>,
}

#[repr(C)]
pub struct AssetLoaderStateAndWorldLock {
    pub world: MutexGuardArc<World>,
    pub asset_loader_state: MutexGuardArc<AssetLoaderState>,
}

impl AssetLoaderStateAndWorldLock {
    pub async fn lock(
        world: &Arc<Mutex<World>>,
        asset_loader_state: &Arc<Mutex<AssetLoaderState>>,
    ) -> Self {
        let world = Arc::clone(world).lock_arc().await;
        let asset_loader_state = Arc::clone(asset_loader_state).lock_arc().await;
        Self {
            world,
            asset_loader_state,
        }
    }
}

/// Identity of a game object. Used to look up game objects (`Thing`s) within a
/// `World`.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct Identity(u32);
impl From<u32> for Identity {
    fn from(value: u32) -> Self {
        Self(value)
    }
}
impl From<usize> for Identity {
    fn from(value: usize) -> Self {
        Self(value as u32)
    }
}
impl From<Identity> for usize {
    fn from(value: Identity) -> Self {
        value.0 as usize
    }
}

pub trait Identifyable {
    fn identify(&self) -> Identity;
}

// TODO implement the rest of the facets
// the main idea here is to construct contiguous areas in memory for different
// facets this is a premature optimization for the Thing/Facet system in general
// to avoid losing cache coherency whilst traversing a series of objects.
// Probably we want to integrate concurrency safety here.
#[derive(Default)]
pub struct WorldFacets {
    cameras: Vec<CameraFacet>,
    models: Vec<ModelFacet>,
    pub physical: Vec<PhysicalFacet>,
    health: Vec<HealthFacet>,
}

impl WorldFacets {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn camera(&self, index: CameraIndex) -> Option<&CameraFacet> {
        self.cameras.get(index.0 as usize)
    }

    pub fn camera_mut(&mut self, index: CameraIndex) -> Option<&mut CameraFacet> {
        self.cameras.get_mut(index.0 as usize)
    }

    pub fn model_iter(&self) -> impl Iterator<Item = (ModelIndex, &Model)> {
        self.models
            .iter()
            .enumerate()
            .map(|(index, facet)| (index.into(), &facet.model))
    }

    pub fn model(&self, index: ModelIndex) -> Option<&ModelFacet> {
        self.models.get(index.0 as usize)
    }

    pub fn physical(&self, index: PhysicalIndex) -> Option<&PhysicalFacet> {
        self.physical.get(index.0 as usize)
    }

    pub fn physical_mut(&mut self, index: PhysicalIndex) -> Option<&mut PhysicalFacet> {
        self.physical.get_mut(index.0 as usize)
    }

    pub fn health(&self, index: HealthIndex) -> Option<&HealthFacet> {
        self.health.get(index.0 as usize)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum WorldError {
    #[error("Too many objects added to world")]
    TooManyObjects,

    #[error("Network error {0:?}")]
    Network(RpcError),

    #[error("Error compressing updates {0:?}")]
    UpdateCompression(io::Error),

    #[error("Error decompressing updates {0:?}")]
    UpdateDecompression(io::Error),

    #[error("Error casting update from bytes {0:?}")]
    UpdateFromBytes(RpcError),

    #[error("no camera facet at index {0:?}")]
    NoSuchCamera(CameraIndex),

    #[error("no camera found in scene")]
    NoCameraFound,

    #[error("thing with id {0:?} not found in scene")]
    ThingNotFound(Identity),

    #[error("no phys facet at index {0:?}")]
    NoSuchPhys(PhysicalIndex),
}

pub struct World {
    pub maybe_camera: Option<Identity>,
    pub things: Vec<Thing>,
    pub facets: WorldFacets,
    pub scene: Scene,
    pub updates: u64,
    pub run_life: Duration,
    pub last_tick: Instant,

    // TODO: support more than one connection, for servers
    // TODO: move into networking related struct
    pub net_disabled: bool,
    pub connection: Option<Box<dyn Connection + Send + Sync + 'static>>,
    pub client_controller_state: Option<InputState>,
    pub server_controller_state: Option<InputState>,
    pub maybe_server_addr: Option<SocketAddr>,

    pub logger: Logger,
}

impl World {
    pub const SIM_TICK_DELAY: Duration = Duration::from_millis(8);

    /// Create a new client or server binding. Currently, in server mode, this
    /// waits for a client to connect before continuing.
    ///
    /// FIXME: make this /// independent of any connecting clients.
    pub fn new(maybe_server_addr: Option<SocketAddr>, logger: &Logger, net_disabled: bool) -> Self {
        Self {
            net_disabled,
            maybe_server_addr,
            maybe_camera: None,
            things: vec![],
            facets: WorldFacets::default(),
            scene: Scene::default(),
            updates: 0,
            run_life: Duration::from_millis(0),
            last_tick: Instant::now(),
            connection: None,
            client_controller_state: None,
            server_controller_state: None,
            logger: logger.sub("world"),
        }
    }

    pub fn get_camera_facet(
        &self,
        cam_id: Identity,
    ) -> Result<(PhysicalFacet, CameraFacet), WorldError> {
        // TODO fix hardcoded locations of cameras that rely on
        // camera 0 and 1 being the first 2 things added to the world.
        let camera = self
            .thing_as_ref(cam_id)
            .ok_or_else(|| WorldError::ThingNotFound(cam_id))?;

        let (phys_facet, camera_facet) = match camera.facets {
            ThingType::Camera { phys, camera } => {
                let world_facets = &self.facets;
                let camera = world_facets
                    .camera(camera)
                    .ok_or(WorldError::NoCameraFound)?;
                let phys = world_facets
                    .physical(phys)
                    .ok_or(WorldError::NoCameraFound)?;
                (phys.clone(), camera.clone())
            }
            _ => return Err(WorldError::NoCameraFound),
        };
        Ok((phys_facet, camera_facet))
    }

    pub fn camera_facet_indices(
        &self,
        cam_id: Identity,
    ) -> Result<(PhysicalIndex, CameraIndex), WorldError> {
        // TODO fix hardcoded locations of cameras that rely on
        // camera 0 and 1 being the first 2 things added to the world.
        let camera = self
            .thing_as_ref(cam_id)
            .ok_or_else(|| WorldError::ThingNotFound(cam_id))?;

        Ok(match camera.facets {
            ThingType::Camera { phys, camera } => (phys, camera),
            _ => return Err(WorldError::NoCameraFound),
        })
    }

    pub fn set_client_controller_state(&mut self, state: InputState) {
        self.client_controller_state = Some(state);
    }

    pub fn set_server_controller_state(&mut self, state: InputState) {
        self.server_controller_state = Some(state);
    }

    pub fn is_server(&self) -> bool {
        self.maybe_server_addr.is_none()
    }

    pub fn add_thing(&mut self, thing: Thing) -> Result<Identity, WorldError> {
        let id = self.things.len();
        if id > std::u32::MAX as usize {
            println!("too many objects, id: {}", id);
            return Err(WorldError::TooManyObjects);
        }
        self.things.push(thing);
        Ok(id.into())
    }

    pub fn add_camera(&mut self, camera: CameraFacet) -> CameraIndex {
        let cameras = &mut self.facets.cameras;
        let idx = cameras.len();
        cameras.push(camera);
        idx.into()
    }

    // Transform should be used as the offset of drawing from the physical facet
    pub fn add_model(&mut self, model: ModelFacet) -> ModelIndex {
        let models = &mut self.facets.models;
        let idx = models.len();
        models.push(model);
        idx.into()
    }

    pub fn add_physical(&mut self, phys: PhysicalFacet) -> PhysicalIndex {
        let physical = &mut self.facets.physical;
        let idx = physical.len();
        physical.push(phys);
        idx.into()
    }

    pub fn maybe_tick(&mut self, _dt: &Duration) {}

    pub fn things(&self) -> &[Thing] {
        &self.things
    }

    pub fn things_mut(&mut self) -> &mut [Thing] {
        &mut self.things
    }

    pub fn thing_as_ref(&self, id: Identity) -> Option<&Thing> {
        let id: usize = id.into();
        self.things.get(id)
    }

    pub fn thing_as_mut(&mut self, id: Identity) -> Option<&mut Thing> {
        let id: usize = id.into();
        self.things.get_mut(id)
    }

    pub fn clear(&mut self) {
        let facets = &mut self.facets;
        self.things.clear();
        facets.cameras.clear();
        facets.health.clear();
        facets.models.clear();
        facets.physical.clear();
    }
}
