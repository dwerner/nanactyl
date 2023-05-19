//! Implements a world and entity system for the engine to mutate and render.

pub mod archetypes;
pub mod thing;

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use archetypes::graphics::GfxArchetype;
use archetypes::index::{GfxIndex, PlayerIndex};
use archetypes::player::{PlayerArchetype, PlayerRef};
use archetypes::Archetype;
use async_lock::{Mutex, MutexGuardArc};
use gfx::{DebugMesh, Graphic, Model};
pub use glam::{Mat4, Quat, Vec3};
use graphrox::Graph;
use input::wire::InputState;
use logger::{LogLevel, Logger};
use network::{Connection, RpcError};
use thing::{
    CameraFacet, CameraIndex, GraphicsFacet, HealthFacet, HealthIndex, PhysicalFacet, PhysicalIndex,
};

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

// TODO implement the rest of the facets
// the main idea here is to construct contiguous areas in memory for different
// facets this is a premature optimization for the Thing/Facet system in general
// to avoid losing cache coherency whilst traversing a series of objects.
// Probably we want to integrate concurrency safety here.
#[derive(Default)]
pub struct WorldFacets {
    cameras: Vec<CameraFacet>,
    graphics: Vec<GraphicsFacet>,
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

    pub fn gfx_iter(&self) -> impl Iterator<Item = (GfxIndex, &GraphicsFacet)> {
        self.graphics
            .iter()
            .enumerate()
            .map(|(index, facet)| (index.into(), facet))
    }

    pub fn model(&self, index: GfxIndex) -> Option<&GraphicsFacet> {
        self.graphics.get(index.0 as usize)
    }

    pub fn physical(&self, index: PhysicalIndex) -> Option<&PhysicalFacet> {
        self.physical.get(index.0 as usize)
    }

    pub fn physical_mut(&mut self, index: PhysicalIndex) -> Option<&mut PhysicalFacet> {
        self.physical.get_mut(index.0 as usize)
    }

    pub fn iter_physical_mut(
        &mut self,
    ) -> impl Iterator<Item = (PhysicalIndex, &mut PhysicalFacet)> {
        self.physical
            .iter_mut()
            .enumerate()
            .map(|(index, facet)| (index.into(), facet))
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
    PlayerNotFound,

    #[error("thing with id {0:?} not found in scene")]
    ThingNotFound(PlayerIndex),

    #[error("no phys facet at index {0:?}")]
    NoSuchPhys(PhysicalIndex),
}

pub struct World {
    pub maybe_camera: Option<PlayerIndex>,

    /// All entities
    // pub things: Vec<Thing>,
    // pub facets: WorldFacets,

    // TODO: multiple archetypes.
    pub players: PlayerArchetype,
    pub gfx: GfxArchetype,

    pub stats: Stats,
    pub config: Config,

    // TODO: support more than one connection, for servers
    // TODO: move into networking related struct
    pub connection: Option<Box<dyn Connection + Send + Sync + 'static>>,
    pub client_controller_state: Option<InputState>,
    pub server_controller_state: Option<InputState>,

    pub logger: Logger,

    pub graph: Graph,
}

pub struct Stats {
    pub updates: u64,
    pub run_life: Duration,
    pub last_tick: Instant,
}

pub struct Config {
    pub net_disabled: bool,
    pub maybe_server_addr: Option<SocketAddr>,
}

/// Reference to a graphics object and for now positional and orientation data.
/// Intended to represent a model (uploaded to the GPU once) with instance
/// information. Should attach to a game object or similar.
pub struct Drawable {
    pub gfx: GfxIndex,
    pub pos: Vec3,
    pub angles: Vec3,
    pub scale: f32,
}

impl World {
    pub const SIM_TICK_DELAY: Duration = Duration::from_millis(8);

    // pub fn get_drawable(
    //     &self,
    //     phys: &PhysicalIndex,
    //     gfx: &GraphicsIndex,
    // ) -> Result<Drawable, WorldError> {
    //     let physical_facet = self
    //         .facets
    //         .physical(*phys)
    //         .ok_or(WorldError::NoSuchPhys(*phys))?;
    //     Ok(Drawable {
    //         gfx: *gfx,
    //         pos: physical_facet.position,
    //         angles: physical_facet.angles,
    //         scale: physical_facet.scale,
    //     })
    // }

    // pub fn get_camera_drawable(
    //     &self,
    //     phys: &PhysicalIndex,
    //     camera: &CameraIndex,
    // ) -> Result<Drawable, WorldError> {
    //     let phys = self
    //         .facets
    //         .physical(*phys)
    //         .ok_or(WorldError::NoSuchPhys(*phys))?;
    //     let cam = self
    //         .facets
    //         .camera(*camera)
    //         .ok_or(WorldError::NoSuchCamera(*camera))?;
    //     let right = cam.right(phys);
    //     let forward = cam.forward(phys);
    //     let pos = phys.position + Vec3::new(right.x + forward.x, -2.0, right.z +
    // forward.z);     let angles = Vec3::new(0.0, phys.angles.y - 1.57, 0.0);
    //     Ok(Drawable {
    //         gfx: cam.associated_graphics.unwrap(),
    //         pos,
    //         angles,
    //         scale: phys.scale,
    //     })
    // }

    /// Create a new client or server binding. Currently, in server mode, this
    /// waits for a client to connect before continuing.
    ///
    /// FIXME: make this /// independent of any connecting clients.
    pub fn new(maybe_server_addr: Option<SocketAddr>, logger: &Logger, net_disabled: bool) -> Self {
        Self {
            config: Config {
                net_disabled,
                maybe_server_addr,
            },
            stats: Stats {
                updates: 0,
                run_life: Duration::from_millis(0),
                last_tick: Instant::now(),
            },
            maybe_camera: None,
            // things: vec![],
            // facets: WorldFacets::default(),
            players: PlayerArchetype::new(),
            gfx: GfxArchetype::new(),

            connection: None,
            client_controller_state: None,
            server_controller_state: None,
            logger: logger.sub("world"),

            // we'll represent the relationship between game objects as an undirected graph.
            graph: Graph::new_undirected(),
        }
    }

    pub fn player(&mut self, player_id: PlayerIndex) -> Result<PlayerRef, WorldError> {
        self.players
            .get_mut(player_id)
            .ok_or(WorldError::PlayerNotFound)
    }

    pub fn set_client_controller_state(&mut self, state: InputState) {
        self.client_controller_state = Some(state);
    }

    pub fn set_server_controller_state(&mut self, state: InputState) {
        self.server_controller_state = Some(state);
    }

    pub fn is_server(&self) -> bool {
        self.config.maybe_server_addr.is_none()
    }

    pub fn add_debug_mesh(&mut self, mesh: DebugMesh) -> GfxIndex {
        self.gfx.add(Graphic::DebugMesh(mesh))
    }

    pub fn add_model(&mut self, model: Model) -> GfxIndex {
        self.gfx.add(Graphic::Model(model))
    }

    // pub fn add_thing(&mut self, thing: Thing) -> Result<Identity, WorldError> {
    //     let id = self.things.len();
    //     if id > std::u32::MAX as usize {
    //         println!("too many objects, id: {}", id);
    //         return Err(WorldError::TooManyObjects);
    //     }
    //     self.things.push(thing);
    //     Ok(id.into())
    // }

    // pub fn add_camera(&mut self, camera: CameraFacet) -> CameraIndex {
    //     let cameras = &mut self.facets.cameras;
    //     let idx = cameras.len();
    //     cameras.push(camera);
    //     idx.into()
    // }

    // pub fn add_physical(&mut self, phys: PhysicalFacet) -> PhysicalIndex {
    //     let physical = &mut self.facets.physical;
    //     let idx = physical.len();
    //     physical.push(phys);
    //     idx.into()
    // }

    // pub fn things(&self) -> &[Thing] {
    //     &self.things
    // }

    // pub fn things_mut(&mut self) -> &mut [Thing] {
    //     &mut self.things
    // }

    // pub fn thing_as_ref(&self, id: Identity) -> Option<&Thing> {
    //     let id: usize = id.into();
    //     self.things.get(id)
    // }

    // pub fn thing_as_mut(&mut self, id: Identity) -> Option<&mut Thing> {
    //     let id: usize = id.into();
    //     self.things.get_mut(id)
    // }

    // pub fn clear(&mut self) {
    //     let facets = &mut self.facets;
    //     self.things.clear();
    //     facets.cameras.clear();
    //     facets.health.clear();
    //     facets.graphics.clear();
    //     facets.physical.clear();
    // }
}
