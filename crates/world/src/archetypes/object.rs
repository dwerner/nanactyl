use glam::Vec3;

use crate::def_archetype;
use crate::graphics::{GfxIndex, Shape};
use crate::health::HealthFacet;

def_archetype! {
    Object,
    gfx: GfxIndex,

    pos: Vec3,
    angles: Vec3,
    scale: f32,

    linear_velocity_intention: Vec3,
    angular_velocity_intention: Vec3,
    shape: Shape,

    health: HealthFacet
}

impl Default for ObjectBuilder {
    fn default() -> Self {
        ObjectBuilder {
            gfx: None,
            pos: Some(Vec3::ZERO),
            angles: Some(Vec3::ZERO),
            scale: Some(1.0),
            linear_velocity_intention: Some(Vec3::ZERO),
            angular_velocity_intention: Some(Vec3::ZERO),
            shape: Some(Shape::cuboid(1.0, 1.0, 1.0)),
            health: Some(HealthFacet::new(100)),
        }
    }
}
