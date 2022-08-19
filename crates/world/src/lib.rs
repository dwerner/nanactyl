use std::sync::{atomic::{AtomicUsize, Ordering}, Mutex};

use thing::{CameraFacet, HealthFacet, ModelInstanceFacet, PhysicalFacet, Thing, ThingBuilder};

pub mod thing;

static GLOBAL_IDENITY_CURSOR: AtomicUsize = AtomicUsize::new(0);

pub type Identity = u64;
pub fn create_next_identity() -> Identity {
    GLOBAL_IDENITY_CURSOR.fetch_add(1, Ordering::SeqCst) as Identity
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
    pub things: scc::HashSet<Thing>,
    facets: Mutex<WorldFacets>,
}

impl World {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn start_thing(&self) -> ThingBuilder {
        ThingBuilder {
            world: self,
            facets: Vec::new(),
        }
    }

    pub fn get_things(&self) -> &scc::HashSet<Thing> {
        &self.things
    }

    pub fn clear(&mut self) {
        let facets = &mut self.facets.lock().unwrap();
        facets.cameras.clear();
        facets.health.clear();
        facets.models.clear();
        facets.physical.clear();
    }
}
