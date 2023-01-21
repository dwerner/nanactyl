use std::io;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use bytemuck::{Pod, PodCastError, Zeroable};
use histogram::Histogram;
use input::wire::InputState;
use models::Model;
use network::{Peer, RpcError, PAYLOAD_LEN};
use scene::Scene;
use thing::{
    CameraFacet, CameraIndex, HealthFacet, HealthIndex, ModelFacet, ModelIndex, PhysicalFacet,
    PhysicalIndex, Thing, ThingType,
};

mod scene;
pub mod thing;
mod tree;

pub use nalgebra::{Matrix4, Vector3};

use crate::wire::{decompress_world_updates, NUM_UPDATES_PER_MSG};

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
impl Into<usize> for Identity {
    fn into(self) -> usize {
        self.0 as usize
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

pub mod wire {

    use super::*;

    pub(crate) const NUM_UPDATES_PER_MSG: u32 = 96;

    #[derive(Debug, Default, Copy, Clone, Pod, Zeroable)]
    #[repr(C)]
    pub struct WorldUpdate {
        pub id: u32,
        pub thing: WireThing,
        pub position: WirePosition,

        // FOR RIGHT NOW, only support y axis rotation
        pub y_rotation: f32,
    }

    impl From<&Thing> for WireThing {
        fn from(thing: &Thing) -> Self {
            match thing.facets {
                thing::ThingType::Camera { phys, camera } => Self {
                    tag: 0,
                    phys: phys.0,
                    facet: camera.0 as u16,
                    _pad: 0,
                },
                thing::ThingType::ModelObject { phys, model } => Self {
                    tag: 1,
                    phys: phys.0,
                    facet: model.0 as u16,
                    _pad: 0,
                },
            }
        }
    }

    impl From<WireThing> for Thing {
        fn from(wt: WireThing) -> Self {
            match wt {
                WireThing {
                    tag: 0,
                    phys,
                    facet,
                    ..
                } => Thing::camera(phys.into(), facet.into()),
                WireThing {
                    tag: 1,
                    phys,
                    facet,
                    ..
                } => Thing::model(phys.into(), facet.into()),
                _ => unreachable!(),
            }
        }
    }

    #[derive(Debug, Default, Copy, Clone, Pod, Zeroable)]
    #[repr(C)]
    pub struct WireThing {
        pub tag: u8,
        _pad: u8,
        pub facet: u16,
        pub phys: u32,
    }

    #[derive(Debug, Default, Copy, Clone, Pod, Zeroable)]
    #[repr(C)]
    pub struct WirePosition(pub f32, pub f32, pub f32);

    const ZSTD_LEVEL: i32 = 3;
    pub(crate) fn compress_world_updates(values: &[WorldUpdate]) -> Result<Vec<u8>, WorldError> {
        let mut sized: [WorldUpdate; NUM_UPDATES_PER_MSG as usize] =
            [WorldUpdate::default(); NUM_UPDATES_PER_MSG as usize];
        sized.copy_from_slice(&values);
        let mut compressed_bytes = vec![];
        let read_bytes = bytemuck::bytes_of(&sized);
        let encoded =
            zstd::encode_all(read_bytes, ZSTD_LEVEL).map_err(WorldError::UpdateCompression)?;
        let len = encoded.len();
        let len = len.min(PAYLOAD_LEN) as u16;
        let len = bytemuck::bytes_of(&len);
        compressed_bytes.extend(len);
        compressed_bytes.extend(encoded);
        Ok(compressed_bytes)
    }

    pub(crate) fn decompress_world_updates(
        compressed: &[u8],
    ) -> Result<Vec<WorldUpdate>, WorldError> {
        let mut decoded_bytes = vec![];
        let len: &u16 = bytemuck::from_bytes(&compressed[0..2]);
        let len = *len;
        let len = len.min(PAYLOAD_LEN as u16);
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
                    let model = Thing::model(physical, model);
                    let wt: WireThing = (&model).into();
                    let wpos = WirePosition(i as f32, i as f32, i as f32);
                    WorldUpdate {
                        id: i,
                        thing: wt,
                        position: wpos,
                        y_rotation: 0.0,
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
    connection: Peer,
    pub client_controller_state: Option<InputState>,
    pub server_controller_state: Option<InputState>,

    maybe_server_addr: Option<SocketAddr>,
}

impl World {
    pub const SIM_TICK_DELAY: Duration = Duration::from_millis(8);

    pub fn rtt_micros(&self) -> Histogram {
        self.connection.rtt_micros.clone()
    }

    pub fn new(maybe_server_addr: Option<SocketAddr>, wait_for_client: bool) -> Self {
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
                    if wait_for_client {
                        client.recv().await.unwrap();
                    } else {
                        client
                            .recv_with_timeout(Duration::from_millis(8))
                            .await
                            .unwrap();
                    }
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
            client_controller_state: None,
            server_controller_state: None,
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
                    .ok_or_else(|| WorldError::NoCameraFound)?;
                let phys = world_facets
                    .physical(phys)
                    .ok_or_else(|| WorldError::NoCameraFound)?;
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

    pub async fn pump_connection_as_server(&mut self) -> Result<[InputState; 2], WorldError> {
        let packet = self
            .things
            .iter()
            .enumerate()
            .map(|(idx, thing)| {
                let id = idx as u32;
                let thing: wire::WireThing = thing.into();
                let p = &self.facets.physical[thing.phys as usize];
                wire::WorldUpdate {
                    id,
                    thing,
                    position: wire::WirePosition(p.position.x, p.position.y, p.position.z),
                    y_rotation: p.angles.y,
                }
            })
            .take(NUM_UPDATES_PER_MSG as usize)
            .collect::<Vec<_>>();
        let compressed = wire::compress_world_updates(&packet)?;
        let _seq = self.connection.send(&compressed).await;
        let client_controller_data = self
            .connection
            .recv_with_timeout(Duration::from_millis(0))
            .await
            .map_err(WorldError::Network)?;

        let payload = client_controller_data
            .try_ref()
            .map_err(WorldError::Network)?
            .payload;
        let len: &u16 = bytemuck::from_bytes(&payload[0..2]);
        let len = *len;
        let cast: &[InputState; 2] = bytemuck::try_from_bytes(&payload[2..2 + len as usize])
            .map_err(|err| WorldError::FromBytes(err, payload.len()))?;
        Ok(*cast)
    }

    pub async fn pump_connection_as_client(
        &mut self,
        controllers: [InputState; 2],
    ) -> Result<(), WorldError> {
        let data = self
            .connection
            .recv_with_timeout(Duration::from_millis(0))
            .await
            .map_err(WorldError::Network)?;
        let decompressed_updates = decompress_world_updates(
            &data.try_ref().map_err(WorldError::UpdateFromBytes)?.payload,
        )?;
        for wire::WorldUpdate {
            id,
            thing,
            position,
            y_rotation,
        } in decompressed_updates
        {
            let thing: Thing = thing.into();
            match self.things.get_mut(id as usize) {
                Some(t) => *t = thing,
                None => println!("thing not found at index {}", id),
            };
            match self.facets.physical.get_mut(id as usize) {
                Some(phys) => {
                    phys.position = Vector3::new(position.0, position.1, position.2);
                    phys.angles.y = y_rotation;
                }
                None => println!("no physical facet at index {}", position.0),
            }
        }

        let mut msg_bytes = vec![];
        let controller_state_bytes = bytemuck::bytes_of(&controllers);
        let len = controller_state_bytes.len().min(PAYLOAD_LEN);
        msg_bytes.extend(bytemuck::bytes_of(&(len as u16)));
        msg_bytes.extend(controller_state_bytes);

        Ok(match self.connection.send(&msg_bytes).await {
            Ok(_) => (),
            Err(_) => (),
        })
    }

    pub fn is_server(&self) -> bool {
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
