use std::error::Error;

use world::{Identifyable, World};

#[derive(Debug)]
pub struct Drawable {
    id: world::Identity,
}

#[derive(Debug)]
pub struct RenderState {
    entities: Vec<Drawable>,
}

impl RenderState {
    pub fn new() -> Self {
        Self {
            entities: Vec::new(),
        }
    }

    pub fn update(&mut self, world: &World) -> Result<(), Box<dyn Error>> {
        self.entities.clear();
        for thing in world.get_things() {
            self.entities.push(Drawable {
                id: thing.identify(),
            })
        }
        Ok(())
    }
}
