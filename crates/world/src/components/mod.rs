pub mod spatial;

use gfx::Graphic;
use glam::{Mat4, Vec3};
use hecs::Entity;

use crate::graphics::{Shape, EULER_ROT_ORDER};
use crate::World;

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
    pub fn new(world_transform: &WorldTransform) -> Self {
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
        camera.update_view_matrix(world_transform);
        camera
    }

    pub fn update_view_matrix(&mut self, world: &WorldTransform) {
        // fine to offset the camera somewhat here, but the orientation of the model
        // should be done at the entity level. I.e. spatial hierarchy entity
        let camera_offset = Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0));
        self.view = (world.world * camera_offset).inverse();
    }

    pub fn set_perspective(&mut self, fov: f32, aspect: f32, near: f32, far: f32) {
        self.projection = Mat4::perspective_lh(aspect, fov, near, far);
    }

    pub fn combined_projection(&self) -> Mat4 {
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

impl WorldTransform {
    pub fn forward(&self) -> Vec3 {
        -self.world.z_axis.truncate().normalize()
    }
    /// Get the position of this transform.
    pub fn get_pos(&self) -> Vec3 {
        let (_scale, _rot, trans) = self.world.to_scale_rotation_translation();
        trans
    }

    /// Get the angles of rotation in EULER_ROT_ORDER.
    pub fn get_angles(&self) -> Vec3 {
        let (_scale, rot, _trans) = self.world.to_scale_rotation_translation();
        rot.to_euler(EULER_ROT_ORDER).into()
    }
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
    use crate::components::spatial::SpatialHierarchyNode;

    #[test]
    fn playing_with_hecs() {
        let mut world = hecs::World::new();

        let root_transform = world.spawn((WorldTransform::default(),));
        let gfx_prefab = world.spawn((GraphicPrefab {
            // TODO: this isn't implemented
            gfx: Graphic::ParticleSystem,
        },));

        let player = Player::new(gfx_prefab, SpatialHierarchyNode::new(root_transform));
        let _player_id = world.spawn(player);

        let entity = {
            let mut query = world.query::<(&Camera, &SpatialHierarchyNode)>();
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
