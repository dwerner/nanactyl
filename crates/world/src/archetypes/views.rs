use glam::{Mat4, Vec3};

use super::object::ObjectArchetype;
use super::player::PlayerArchetype;
use crate::graphics::{GfxIndex, Shape};
use crate::{def_ref_and_iter, impl_iter_method};

def_ref_and_iter! {
    Draw,
    gfx: GfxIndex,
    pos: Vec3,
    angles: Vec3,
    scale: f32
}

def_ref_and_iter! {
    Camera,
    perspective: Mat4,
    view: Mat4
}

def_ref_and_iter! {
    Physics,
    pos: Vec3,
    angles: Vec3,
    scale: f32,
    linear_velocity_intention: Vec3,
    angular_velocity_intention: Vec3,
    shape: Shape
}

impl_iter_method! {
    Draw => PlayerArchetype,
    gfx,
    pos,
    angles,
    scale
}

impl_iter_method! {
    Draw => ObjectArchetype,
    gfx,
    pos,
    angles,
    scale
}

// We can't impl camera_iter_mut() on ObjectArchetype because it doesn't have
// camera fields.
impl_iter_method! {
    Camera => PlayerArchetype,
    perspective,
    view
}

impl_iter_method! {
    Physics => PlayerArchetype,
    pos,
    angles,
    scale,
    linear_velocity_intention,
    angular_velocity_intention,
    shape
}

impl_iter_method! {
    Physics => ObjectArchetype,
    pos,
    angles,
    scale,
    linear_velocity_intention,
    angular_velocity_intention,
    shape
}
