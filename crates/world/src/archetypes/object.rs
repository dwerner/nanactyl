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
