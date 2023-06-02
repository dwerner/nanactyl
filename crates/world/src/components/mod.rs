use std::time::Duration;

pub mod spatial;

use gfx::Graphic;
use glam::{Mat4, Vec3};
use heks::Entity;
use spatial::SpatialNode;

use crate::graphics::Shape;

/// A component representing a camera.
#[derive(Debug, Default)]
#[repr(C)]
pub struct Camera {
    pub projection: Mat4,
    pub view: Mat4,
    pub occlusion_culling: bool,
}

impl Camera {
    // Problematic because multiple components make up the properties of the camera,
    // including position, matrices, etc.
    pub fn new(spatial: &SpatialNode) -> Self {
        let mut camera = Camera {
            view: Mat4::IDENTITY,

            // TODO fix default perspective values
            projection: Mat4::perspective_lh(
                1.7,    //aspect
                0.75,   //fovy
                0.1,    // near
                1000.0, //far
            ),

            // because it's not supported yet
            occlusion_culling: false,
        };
        camera.update_view_matrix(spatial);
        camera
    }

    pub fn update_from_phys(
        &mut self,
        dt: &Duration,
        spatial: &mut SpatialNode,
        physics: &PhysicsBody,
    ) {
        let amount = (dt.as_millis() as f64 / 100.0) as f32;
        spatial.translate(physics.linear_velocity * amount);
        self.update_view_matrix(spatial);
    }

    pub fn update_view_matrix(&mut self, spatial: &SpatialNode) {
        // TODO: debugging view matrix

        self.view =
            (spatial.transform * Mat4::from_translation(Vec3::new(0.0, -1.0, 0.0))).inverse();
    }

    pub fn set_perspective(&mut self, fov: f32, aspect: f32, near: f32, far: f32) {
        self.projection = Mat4::perspective_lh(aspect, fov, near, far);
    }

    pub fn view_projection(&self) -> Mat4 {
        self.projection * self.view
    }
}

/// A component representing a control. Should encapsulate action intention.
/// Should be drawn on the debug layer.
#[derive(Debug, Default)]
pub struct Control {
    pub linear_intention: Vec3,
    pub angular_intention: Vec3,
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
pub struct PhysicsBody {
    pub linear_velocity: Vec3,
    pub linear_acceleration: Vec3,
    pub angular_velocity: Vec3,
    pub angular_acceleration: Vec3,
    pub mass: f32,
}

#[derive(Debug, Default)]
pub struct Shaped {
    pub shape: Shape,
}

/// Just a tag struct for static physics.
#[derive(Debug, Default)]
pub struct StaticPhysics;

/// World transform, computed from the relative transform.
/// The world root is an entity with an absolute transform.
#[derive(Debug, Default)]
pub struct WorldTransform {
    pub world: Mat4,
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
    fn playing_with_heks() {
        let mut world = heks::World::new();

        let root_transform = world.spawn((WorldTransform::default(),));
        let gfx_prefab = world.spawn((GraphicPrefab {
            // TODO: this isn't implemented
            gfx: Graphic::ParticleSystem,
        },));

        let player = Player::new(gfx_prefab, SpatialNode::new(root_transform));
        let _player_id = world.spawn(player);

        let entity = {
            let mut query = world.query::<(&Camera, &SpatialNode)>();
            let (entity, (camera, pos)) = query.iter().next().unwrap();
            println!("{:?}", entity);

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
