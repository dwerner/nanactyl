//! Bundles for common archetypes

use glam::{Mat4, Vec3};
use hecs::{Bundle, Entity};

use crate::components::{
    Camera, Control, Drawable, DynamicPhysics, RelativeTransform, Spatial, WorldTransform,
};

#[derive(Debug, Bundle)]
pub struct StaticObject {
    pub spatial: Spatial,
    pub drawable: Drawable,
    pub physics: DynamicPhysics,
    pub parent: RelativeTransform,
    pub world: WorldTransform,
}

impl StaticObject {
    /// Create a new StaticObject with the given parent
    pub fn new(parent: Entity, gfx_prefab: Entity, spatial: Spatial) -> Self {
        StaticObject {
            spatial,
            drawable: Drawable {
                gfx: gfx_prefab,
                scale: 1.0,
            },
            physics: DynamicPhysics {
                velocity: Vec3::ZERO,
                acceleration: Vec3::ZERO,
                mass: 1.0,
            },
            parent: RelativeTransform {
                parent,
                relative_matrix: Mat4::IDENTITY,
            },
            world: WorldTransform {
                world_matrix: Mat4::IDENTITY,
            },
        }
    }
}

// TODO: bundles move into the plugin!
#[derive(Debug, Bundle)]
pub struct Player {
    pub camera: Camera,
    pub control: Control,
    pub spatial: Spatial,
    pub drawable: Drawable,
    pub physics: DynamicPhysics,
    pub parent: RelativeTransform,
    pub world: WorldTransform,
}

impl Player {
    /// Create a new Player with the given parent
    /// TODO:
    ///     - take a local transform?
    pub fn new(parent: Entity, gfx_prefab: Entity, spatial: Spatial) -> Self {
        let perspective = Mat4::perspective_lh(
            1.7,    //aspect
            0.75,   //fovy
            0.1,    // near
            1000.0, //far
        );
        Player {
            spatial,
            camera: Camera {
                fov: 0.75,
                near: 0.1,
                far: 1000.0,
                perspective,
            },
            drawable: Drawable {
                gfx: gfx_prefab,
                scale: 1.0,
            },
            physics: DynamicPhysics {
                velocity: Vec3::ZERO,
                acceleration: Vec3::ZERO,
                mass: 1.0,
            },
            parent: RelativeTransform {
                parent,
                relative_matrix: Mat4::IDENTITY,
            },
            world: WorldTransform {
                world_matrix: Mat4::IDENTITY,
            },
            control: Control {
                linear_intention: Vec3::ZERO,
                angular_intention: Vec3::ZERO,
            },
        }
    }
}
