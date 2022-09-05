use std::time::Duration;

use network::Peer;
use scene::Scene;
use thing::{CameraFacet, HealthFacet, ModelInstanceFacet, PhysicalFacet, Thing, ThingBuilder};

mod scene;
pub mod thing;
mod tree;

/// Identity of a game object. Used to look up game objects (`Thing`s) within a `World`.
pub type Identity = u32;

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
    pub cameras: Vec<CameraFacet>,
    pub models: Vec<ModelInstanceFacet>,
    pub physical: Vec<PhysicalFacet>,
    pub health: Vec<HealthFacet>,
}

impl WorldFacets {
    pub fn new() -> Self {
        Default::default()
    }
}

#[derive(Default)]
pub struct World {
    pub things: Vec<Thing>,
    facets: WorldFacets,
    pub scene: Scene,
    pub updates: u64,
    pub run_life: Duration,
    _network_peers: Vec<Peer>,
}

impl World {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn start_thing(&mut self) -> ThingBuilder {
        ThingBuilder {
            world: self,
            facet_indices: Vec::new(),
            maybe_child_of: None,
        }
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
        self.things.get(id as usize)
    }

    pub fn thing_as_mut(&mut self, id: Identity) -> Option<&mut Thing> {
        self.things.get_mut(id as usize)
    }

    pub fn clear(&mut self) {
        let facets = &mut self.facets;
        facets.cameras.clear();
        facets.health.clear();
        facets.models.clear();
        facets.physical.clear();
    }
}
