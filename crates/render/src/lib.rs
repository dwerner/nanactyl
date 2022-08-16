use std::error::Error;

use world::{Identifyable, World};

#[derive(Debug)]
pub struct Drawable {
    id: world::Identity,
}

#[derive(Debug)]
pub struct RenderState {
    entities: Vec<Drawable>,
    pub updates: u64,
}

impl RenderState {
    pub fn new() -> Self {
        Self {
            entities: Vec::new(),
            updates: 0,
        }
    }

    pub fn update(&mut self, world: &World) -> Result<(), Box<dyn Error>> {
        self.updates += 1;
        self.entities.clear();
        for thing in world.get_things() {
            self.entities.push(Drawable {
                id: thing.identify(),
            })
        }
        Ok(())
    }
}
