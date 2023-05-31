//! Bundles for common archetypes

use glam::Mat4;
use heks::{Bundle, Entity, Query};

use crate::components::{
    Camera, Control, Drawable, PhysicsBody, RelativeTransform, Spatial, StaticPhysics,
    WorldTransform,
};

#[derive(Debug)]
pub struct StaticObject(pub (Spatial, Drawable, RelativeTransform, WorldTransform));

impl StaticObject {
    /// Create a new StaticObject with the given parent
    pub fn new(parent: Entity, gfx_prefab: Entity, spatial: Spatial) -> Self {
        Self((
            spatial,
            Drawable {
                gfx: gfx_prefab,
                scale: 1.0,
            },
            RelativeTransform {
                parent,
                relative_matrix: Mat4::IDENTITY,
            },
            WorldTransform {
                world_matrix: Mat4::IDENTITY,
            },
        ))
    }
}

#[derive(Debug)]
pub struct Player(
    pub  (
        Camera,
        Control,
        Spatial,
        Drawable,
        PhysicsBody,
        RelativeTransform,
        WorldTransform,
    ),
);

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
        Player((
            Camera {
                projection: perspective,
                ..Default::default()
            },
            Control {
                ..Default::default()
            },
            spatial,
            Drawable {
                gfx: gfx_prefab,
                scale: 1.0,
            },
            PhysicsBody {
                mass: 1.0,
                ..Default::default()
            },
            RelativeTransform {
                parent,
                relative_matrix: Mat4::IDENTITY,
            },
            WorldTransform {
                world_matrix: Mat4::IDENTITY,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_player_bundle() {
        let mut world = heks::World::new();
        let root = world.spawn((WorldTransform::default(),));
        let gfx = world.spawn((WorldTransform::default(),));
        let player = Player::new(root, gfx, Spatial::default());

        let p = world.spawn(player.0);

        let mut query = world.query_one::<(&Camera,)>(p).unwrap();

        assert!(matches!(query.get(), Some(_)));
    }
}
