//! Implements a world and entity system for the engine to mutate and render.

pub mod bundles;
pub mod components;
pub mod graphics;
pub mod health;

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_lock::{Mutex, MutexGuardArc};
use bundles::Player;
use components::{GraphicPrefab, WorldTransform};
use gfx::{DebugMesh, Graphic, Model};
pub use glam::{Mat4, Quat, Vec3};
use graphrox::Graph;
pub use heks::Entity;
use input::wire::InputState;
use logger::{info, LogLevel, Logger};
use network::{Connection, RpcError};
use stable_typeid::StableTypeId;

use crate::components::Camera;

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

    #[error("no camera")]
    NoSuchCamera,

    #[error("no camera found in scene")]
    PlayerNotFound,

    #[error("thing with id {0:?} not found in scene")]
    ThingNotFound(u32),

    #[error("no phys facet at index {0:?}")]
    NoSuchPhys(u32),

    #[error("component error {0:?}")]
    Component(heks::ComponentError),

    #[error("component error {0:?}")]
    NoSuchEntity(heks::NoSuchEntity),
}

pub struct World {
    pub heks_world: heks::World,

    pub root: heks::Entity,

    pub stats: Stats,
    pub config: Config,

    // TODO: support more than one connection, for servers
    // TODO: move into networking related struct
    pub connection: Option<Box<dyn Connection + Send + Sync + 'static>>,

    players: Vec<Entity>,
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

impl World {
    pub const SIM_TICK_DELAY: Duration = Duration::from_millis(8);

    /// Create a new client or server binding. Currently, in server mode, this
    /// waits for a client to connect before continuing.
    ///
    /// FIXME: make this /// independent of any connecting clients.
    pub fn new(maybe_server_addr: Option<SocketAddr>, logger: &Logger, net_disabled: bool) -> Self {
        let mut heks_world = heks::World::new();
        let root_entity = heks_world.spawn((WorldTransform::default(),));
        Self {
            connection: None,

            players: Vec::new(),
            client_controller_state: None,
            server_controller_state: None,

            config: Config {
                net_disabled,
                maybe_server_addr,
            },

            stats: Stats {
                updates: 0,
                run_life: Duration::from_millis(0),
                last_tick: Instant::now(),
            },

            heks_world,
            root: root_entity,

            logger: logger.sub("world"),

            // we'll represent the relationship between game objects as an undirected graph.
            graph: Graph::new_undirected(),
        }
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

    pub fn add_debug_mesh(&mut self, mesh: DebugMesh) -> Entity {
        self.heks_world.spawn((GraphicPrefab {
            gfx: Graphic::DebugMesh(mesh),
        },))
    }

    pub fn add_model(&mut self, model: Model) -> Entity {
        self.heks_world.spawn((GraphicPrefab {
            gfx: Graphic::Model(model),
        },))
    }

    pub fn add_player(&mut self, player: Player) -> Entity {
        let player = self.heks_world.spawn(player);
        let log = self.logger.sub("entity");
        info!(
            log,
            "spawned player entity: {:?} camera type_name {} typeid {:?}",
            player,
            std::any::type_name::<Camera>(),
            StableTypeId::of::<Camera>()
        );
        self.players.push(player);
        player
    }

    pub fn player(&self, index: usize) -> Option<Entity> {
        let entity = self.players.get(index)?;
        Some(*entity)
    }

    pub fn camera(&self) -> Option<Entity> {
        if self.is_server() {
            self.players.get(0).copied()
        } else {
            self.players.get(1).copied()
        }
    }
}
