//! Bundles for common archetypes

use glam::Mat4;
use hecs::{Bundle, Entity, Query};

use crate::components::{
    Camera, Control, Drawable, PhysicsBody, RelativeTransform, Spatial, StaticPhysics,
    WorldTransform,
};

#[derive(Debug, Bundle)]
pub struct StaticObject {
    pub spatial: Spatial,
    pub drawable: Drawable,
    pub parent: RelativeTransform,
    pub world: WorldTransform,
}

impl StaticObject {
    /// Create a new StaticObject with the given parent
    pub fn new(parent: Entity, gfx_prefab: Entity, spatial: Spatial) -> Self {
        Self {
            spatial,
            drawable: Drawable {
                gfx: gfx_prefab,
                scale: 1.0,
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

#[derive(Debug, Bundle)]
pub struct Player {
    pub camera: Camera,
    pub control: Control,
    pub spatial: Spatial,
    pub drawable: Drawable,
    pub physics: PhysicsBody,
    pub parent: RelativeTransform,
    pub world: WorldTransform,
}
// TODO: bundles move into the plugin?
#[derive(Debug, Query)]
pub struct PlayerQuery<'a> {
    pub camera: &'a mut Camera,
    pub control: &'a mut Control,
    pub spatial: &'a mut Spatial,
    pub drawable: &'a mut Drawable,
    pub physics: &'a mut PhysicsBody,
    pub parent: &'a mut RelativeTransform,
    pub world: &'a mut WorldTransform,
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
                projection: perspective,
                ..Default::default()
            },
            drawable: Drawable {
                gfx: gfx_prefab,
                scale: 1.0,
            },
            physics: PhysicsBody {
                mass: 1.0,
                ..Default::default()
            },
            parent: RelativeTransform {
                parent,
                relative_matrix: Mat4::IDENTITY,
            },
            world: WorldTransform {
                world_matrix: Mat4::IDENTITY,
            },
            control: Control {
                ..Default::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_player_bundle() {
        let mut world = hecs::World::new();
        let root = world.spawn((WorldTransform::default(),));
        let gfx = world.spawn((WorldTransform::default(),));
        let player = Player::new(root, gfx, Spatial::default());

        let p = world.spawn(player);

        let mut query = world.query_one::<PlayerQuery>(p).unwrap();

        assert!(matches!(query.get(), Some(_)));
    }
}
