use std::time::Duration;

use models::Model;
use network::Peer;
use scene::Scene;
use thing::{
    CameraFacet, CameraIndex, HealthFacet, HealthIndex, ModelFacet, ModelIndex, PhysicalFacet,
    PhysicalIndex, Thing,
};

mod scene;
pub mod thing;
mod tree;

pub use nalgebra::{Matrix4, Vector3};

/// Identity of a game object. Used to look up game objects (`Thing`s) within a `World`.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct Identity(u32);
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

#[derive(Default)]
pub struct World {
    pub maybe_camera: Option<Identity>,
    things: Vec<Thing>,
    pub facets: WorldFacets,
    pub scene: Scene,
    pub updates: u64,
    pub run_life: Duration,
    _network_peers: Vec<Peer>,
}

#[derive(thiserror::Error, Debug)]
pub enum WorldError {
    #[error("Too many objects added to world")]
    TooManyObjects,
}

impl World {
    pub fn new() -> Self {
        Default::default()
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

    pub fn tick(&mut self, dt: &Duration) {
        self.run_life += *dt;
        self.updates += 1;
        for physical in self.facets.physical.iter_mut() {
            physical.position += physical.linear_velocity * (dt.as_millis() / 1000) as f32;
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
        facets.cameras.clear();
        facets.health.clear();
        facets.models.clear();
        facets.physical.clear();
    }
}
