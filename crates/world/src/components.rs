use gfx::Graphic;
use glam::{Mat4, Vec3};
use hecs::Entity;

use crate::graphics::Shape;

/// A component representing a camera.
#[derive(Debug, Default)]
pub struct Camera {
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    pub perspective: Mat4,
}

/// A component representing a control. Should encapsulate action intention.
#[derive(Debug, Default)]
pub struct Control {
    pub linear_intention: Vec3,
    pub angular_intention: Vec3,
}

/// A component representing a position and rotation.
#[derive(Debug, Default)]
pub struct Spatial {
    pub pos: Vec3,
    pub angles: Vec3,
    pub scale: f32,
    // pub rotation: Mat4,
}

impl Spatial {
    pub fn new_at(pos: Vec3) -> Self {
        Spatial {
            pos,
            angles: Vec3::ZERO,
            scale: 1.0,
        }
    }
}

/// Instance of a graphic, attached to an entity.
#[derive(Debug)]
pub struct Drawable {
    pub gfx: Entity,
    pub scale: f32,
}

/// Prefab of a graphic, represented as an entity.
#[derive(Debug)]
pub struct GraphicPrefab {
    pub gfx: Graphic,
}

impl GraphicPrefab {
    pub fn new(gfx: Graphic) -> Self {
        GraphicPrefab { gfx }
    }
}

/// Dynamic physics objects have a rigidbody.
/// TODO: revisit this and store handles for physics lookups?
#[derive(Debug, Default)]
pub struct DynamicPhysics {
    pub velocity: Vec3,
    pub acceleration: Vec3,
    pub mass: f32,
}

#[derive(Debug, Default)]
pub struct Shaped {
    pub shape: Shape,
}

/// Just a tag struct for static physics.
#[derive(Debug, Default)]
pub struct StaticPhysics;

/// Hierarchical transform relative to a parent.
#[derive(Debug)]
pub struct RelativeTransform {
    pub parent: Entity,
    pub relative_matrix: Mat4,
}

/// World transform, computed from the relative transform.
/// The world root is an entity with an absolute transform.
#[derive(Debug, Default)]
pub struct WorldTransform {
    pub world_matrix: Mat4,
}

/// A component representing an audio source.
#[derive(Debug, Default)]
pub struct AudioSource {
    enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundles::Player;

    #[test]
    fn playing_with_hecs() {
        let mut world = hecs::World::new();

        let root_transform = world.spawn((WorldTransform::default(),));
        let gfx_prefab = world.spawn((GraphicPrefab {
            // TODO: this isn't implemented
            gfx: Graphic::ParticleSystem,
        },));

        let mut player = Player::new(root_transform, gfx_prefab, Spatial::default());
        player.camera.far = 42.0;
        let _player_id = world.spawn(player);

        let entity = {
            let mut query = world.query::<(&Camera, &Spatial)>();
            let (entity, (camera, pos)) = query.iter().next().unwrap();
            println!("{:?}", entity);
            assert_eq!(camera.far, 42.0);

            let mut nodes = world.query::<&Player>();
            for node in nodes.iter() {
                println!("{:#?}", node);
            }
            entity
        };

        // add a single component
        world.insert_one(entity, AudioSource::default()).unwrap();
    }
}
