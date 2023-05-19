use gfx::Graphic;
use glam::Vec3;

use super::index::GfxIndex;

pub struct GfxArchetype {
    pub graphics: Vec<Graphic>,
}

impl GfxArchetype {
    pub fn new() -> Self {
        Self {
            graphics: Vec::new(),
        }
    }

    pub fn add(&mut self, graphic: Graphic) -> GfxIndex {
        let index = self.graphics.len().into();
        self.graphics.push(graphic);
        index
    }
}

pub struct PositionProjection<'a> {
    pub position: &'a mut Vec3,
    pub angles: &'a Vec3,
    pub scale: &'a f32,
}
