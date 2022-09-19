use std::{
    io,
    net::{SocketAddr, SocketAddrV4},
    time::{Duration, Instant},
};

use bytemuck::{Pod, PodCastError, Zeroable};
use core_executor::ThreadExecutorSpawner;
use histogram::Histogram;
use models::Model;
use network::{Peer, RpcError};
use scene::Scene;
use thing::{
    CameraFacet, CameraIndex, HealthFacet, HealthIndex, ModelFacet, ModelIndex, PhysicalFacet,
    PhysicalIndex, Thing,
};

mod scene;
pub mod thing;
mod tree;

pub use nalgebra::{Matrix4, Vector3};

use crate::wire::{decompress_world_updates, NUM_UPDATES_PER_MSG};

/// Identity of a game object. Used to look up game objects (`Thing`s) within a `World`.
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
impl Into<usize> for Identity {
    fn into(self) -> usize {
        self.0 as usize
    }
}

pub trait Identifyable {
    fn identify(&self) -> Identity;
}

// TODO implement the rest of the facets
// the main idea here is to construct contiguous areas in memory for different facets
// this is a premature optimization for the Thing/Facet system in general to avoid losing cache
// coherency whilst traversing a series of objects. Probably we want to integrate concurrency
// safety here.
#[derive(Default)]
pub struct WorldFacets {
    cameras: Vec<CameraFacet>,
    models: Vec<ModelFacet>,
    physical: Vec<PhysicalFacet>,
    health: Vec<HealthFacet>,
}

impl WorldFacets {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn camera(&self, index: CameraIndex) -> Option<&CameraFacet> {
        self.cameras.get(index.0 as usize)
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

    pub fn health(&self, index: HealthIndex) -> Option<&HealthFacet> {
        self.health.get(index.0 as usize)
    }
}

pub mod wire {

    use std::mem::size_of;

    use super::*;

    pub(crate) const NUM_UPDATES_PER_MSG: u32 = 96;
    pub(crate) const UPDATE_PAYLOAD_LEN: usize =
        size_of::<WorldUpdate>() * NUM_UPDATES_PER_MSG as usize;

    #[derive(Debug, Copy, Clone, Pod, Zeroable)]
    #[repr(C)]
    pub struct WorldUpdate {
        pub thing: WireThing,
        pub position: WirePosition,
    }

    impl From<(Identity, Thing)> for WireThing {
        fn from((id, thing): (Identity, Thing)) -> Self {
            let (tag, phys, facet) = match thing.facets {
                thing::ThingType::Camera { phys, camera } => (0, phys.0, camera.0),
                thing::ThingType::ModelObject { phys, model } => (1, phys.0, model.0),
            };
            Self(id.0, tag, phys, facet)
        }
    }

    impl From<WireThing> for (Identity, Thing) {
        fn from(wt: WireThing) -> Self {
            match wt {
                WireThing(id, 0, phys, camera) => {
                    (id.into(), Thing::camera(phys.into(), camera.into()))
                }
                WireThing(id, 1, phys, model) => {
                    (id.into(), Thing::model(phys.into(), model.into()))
                }
                _ => unreachable!(),
            }
        }
    }

    #[derive(Debug, Copy, Clone, Pod, Zeroable)]
    #[repr(C)]
    pub struct WireThing(pub u32, pub u32, pub u32, pub u32);

    #[derive(Debug, Copy, Clone, Pod, Zeroable)]
    #[repr(C)]
    pub struct WirePosition(pub u32, pub f32, pub f32, pub f32);

    const ZSTD_LEVEL: i32 = 1;
    pub(crate) fn compress_world_updates(values: &[WorldUpdate]) -> Result<Vec<u8>, WorldError> {
        let mut sized: [WorldUpdate; NUM_UPDATES_PER_MSG as usize] = unsafe { std::mem::zeroed() };
        sized.copy_from_slice(&values);
        let mut compressed_bytes = vec![];
        let read_bytes = bytemuck::bytes_of(&sized);
        let encoded =
            zstd::encode_all(read_bytes, ZSTD_LEVEL).map_err(WorldError::UpdateCompression)?;
        let len = encoded.len() as u16;
        let len = bytemuck::bytes_of(&len);
        compressed_bytes.extend(len);
        compressed_bytes.extend(encoded);
        Ok(compressed_bytes)
    }

    pub(crate) fn decompress_world_updates(
        compressed: &[u8],
        passthrough: bool,
    ) -> Result<Vec<WorldUpdate>, WorldError> {
        let mut decoded_bytes = vec![];
        let len: &u16 = bytemuck::from_bytes(&compressed[0..2]);
        let len = *len;
        let decoded = zstd::decode_all(&compressed[2..2 + len as usize])
            .map_err(WorldError::UpdateDecompression)?;
        decoded_bytes.extend(decoded);
        let updates: &[WorldUpdate; NUM_UPDATES_PER_MSG as usize] =
            bytemuck::try_from_bytes(&decoded_bytes)
                .map_err(|err| WorldError::FromBytes(err, decoded_bytes.len()))?;
        Ok(updates.iter().cloned().collect())
    }

    #[cfg(test)]
    mod tests {

        use super::*;

        #[test]
        fn test_compression_roundtrip() {
            let values = (0..NUM_UPDATES_PER_MSG)
                .map(|i| {
                    let physical = PhysicalIndex(i);
                    let model = ModelIndex(i);
                    let wt: WireThing = (Identity(i), Thing::model(physical, model)).into();
                    let wpos = WirePosition(i, i as f32, i as f32, i as f32);
                    WorldUpdate {
                        thing: wt,
                        position: wpos,
                    }
                })
                .collect::<Vec<_>>();

            let compressed_bytes = compress_world_updates(&values).unwrap();
            println!("compressed_bytes {}", compressed_bytes.len());
            let decompressed = decompress_world_updates(&compressed_bytes).unwrap();
            assert_eq!(values.len(), decompressed.len());
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

    #[error("Error pod casting update from bytes {0:?} len {1}")]
    FromBytes(PodCastError, usize),
}

pub struct World {
    pub maybe_camera: Option<Identity>,
    things: Vec<Thing>,
    pub facets: WorldFacets,
    pub scene: Scene,
    pub updates: u64,
    pub run_life: Duration,
    last_tick: Instant,

    // TODO: support more than one connection, for servers
    connection: Peer,

    maybe_server_addr: Option<SocketAddr>,
    spawner: ThreadExecutorSpawner,
}

impl World {
    const SIM_TICK_DELAY: Duration = Duration::from_millis(16);

    pub fn rtt_micros(&self) -> Histogram {
        self.connection.rtt_micros.clone()
    }

    pub fn new(maybe_server_addr: Option<SocketAddr>, spawner: ThreadExecutorSpawner) -> Self {
        let connection = match maybe_server_addr {
            Some(addr) => {
                let conn = futures_lite::future::block_on(async move {
                    let mut server = Peer::bind_dest("0.0.0.0:12001", &addr.to_string())
                        .await
                        .unwrap();
                    server.send(b"moar plz").await.unwrap();
                    server
                });
                conn
            }
            None => {
                // We will run as a server, accepting new connections.
                let conn = futures_lite::future::block_on(async move {
                    let mut client = Peer::bind_only("0.0.0.0:12002").await.unwrap();
                    client.recv().await.unwrap();
                    client
                });
                conn
            }
        };
        Self {
            maybe_camera: None,
            things: vec![],
            facets: WorldFacets::default(),
            scene: Scene::default(),
            updates: 0,
            run_life: Duration::from_millis(0),
            last_tick: Instant::now(),
            maybe_server_addr,
            connection,
            spawner,
        }
    }

    pub async fn poll_connection(&mut self) -> Result<(), WorldError> {
        if self.connection.is_connected() {
            // TODO: something better if we have not enough updates
            if self.is_server() && self.things.len() >= NUM_UPDATES_PER_MSG as usize {
                let packet = self
                    .things
                    .iter()
                    .enumerate()
                    .map(|(idx, thing)| {
                        let thing: wire::WireThing = (Identity(idx as u32), thing.clone()).into();
                        let p = self.facets.physical[idx].position;
                        wire::WorldUpdate {
                            thing,
                            position: wire::WirePosition(idx as u32, p.x, p.y, p.z),
                        }
                    })
                    .take(NUM_UPDATES_PER_MSG as usize)
                    .collect::<Vec<_>>();
                let compressed = wire::compress_world_updates(&packet)?;
                let _seq = self.connection.send(&compressed).await; // .map_err(WorldError::Network)?;
                let _data = self
                    .connection
                    .recv_with_timeout(Duration::from_millis(0))
                    .await; //.map_err(WorldError::Network)?;
            } else {
                let data = self
                    .connection
                    .recv_with_timeout(Duration::from_millis(0))
                    .await
                    .map_err(WorldError::Network)?;

                let decompressed_updates = decompress_world_updates(
                    &data.try_ref().map_err(WorldError::UpdateFromBytes)?.payload,
                    false,
                )?;
                for wire::WorldUpdate { thing, position } in decompressed_updates {
                    let (id, thing): (Identity, Thing) = thing.into();
                    match self.things.get_mut(id.0 as usize) {
                        Some(t) => *t = thing,
                        None => println!("thing not found at index {}", id.0),
                    };
                    match self.facets.physical.get_mut(position.0 as usize) {
                        Some(phys) => {
                            phys.position = Vector3::new(position.1, position.2, position.3);
                        }
                        None => println!("no physical facet at index {}", position.0),
                    }
                }
                // as the client
                match self.connection.send(b"moar plz").await {
                    Ok(_) => (),
                    Err(_) => (),
                }
            }
        }

        Ok(())
    }

    pub fn is_server(&mut self) -> bool {
        self.maybe_server_addr.is_none()
    }

    pub fn add_thing(&mut self, thing: Thing) -> Result<Identity, WorldError> {
        let id = self.things.len();
        if id > std::u32::MAX as usize {
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

    pub fn maybe_tick(&mut self, dt: &Duration) {
        self.run_life += *dt;
        self.updates += 1;

        if self.is_server() {
            let now = Instant::now();
            let since_last_tick = now.duration_since(self.last_tick);
            if since_last_tick > Self::SIM_TICK_DELAY {
                for physical in self.facets.physical.iter_mut() {
                    let amount = physical.linear_velocity
                        * ((since_last_tick.as_micros() as f32) / 1000.0 / 1000.0);
                    physical.position += amount;
                }
                self.last_tick = Instant::now();
            }
        }
    }

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
