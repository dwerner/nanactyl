use std::time::Duration;

use gfx::Graphic;
use glam::{Mat4, Vec3};
use heks::Entity;

use crate::graphics::{Shape, EULER_ROT_ORDER};

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
    pub fn new(spatial: &Spatial, physics: &PhysicsBody) -> Self {
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
        camera.update_view_matrix(spatial, physics);
        camera
    }

    pub fn update(&mut self, dt: &Duration, spatial: &mut Spatial, physics: &PhysicsBody) {
        let amount = (dt.as_millis() as f64 / 100.0) as f32;
        spatial.pos += physics.linear_velocity * amount;
        self.update_view_matrix(spatial, physics);
    }

    pub fn update_view_matrix(&mut self, spatial: &Spatial, physics: &PhysicsBody) {
        let rot = Mat4::from_euler(
            EULER_ROT_ORDER,
            physics.angular_velocity.x,
            physics.angular_velocity.y,
            0.0,
        );
        let trans = Mat4::from_translation(spatial.pos);
        self.view = trans * rot;
    }

    pub fn set_perspective(&mut self, fov: f32, aspect: f32, near: f32, far: f32) {
        self.projection = Mat4::perspective_lh(aspect, fov, near, far);
    }
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
    pub fn with_angles(mut self, angles: Vec3) -> Self {
        self.angles = angles;
        self
    }
    pub fn forward(&self) -> Vec3 {
        let rx = self.angles.x;
        let ry = self.angles.y;
        let vec = {
            let x = -rx.cos() * ry.sin();
            let y = rx.sin();
            let z = rx.cos() * ry.cos();
            Vec3::new(x, y, z)
        };
        vec.normalize()
    }

    pub fn right(&self) -> Vec3 {
        let y = Vec3::new(1.0, 0.0, 0.0);
        let forward = self.forward();
        let cross = y.cross(forward);
        cross.normalize()
    }

    pub fn up(&self) -> Vec3 {
        let x = Vec3::new(0.0, 1.0, 0.0);
        x.cross(self.forward()).normalize()
    }
}

impl Spatial {
    pub fn new_at(pos: Vec3) -> Self {
        Spatial {
            pos,
            angles: Vec3::ZERO,
            scale: 1.0,
        }
    }
    pub fn new_with_scale(scale: f32) -> Self {
        Spatial {
            pos: Vec3::ZERO,
            angles: Vec3::ZERO,
            scale,
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
    fn playing_with_heks() {
        let mut world = heks::World::new();

        let root_transform = world.spawn((WorldTransform::default(),));
        let gfx_prefab = world.spawn((GraphicPrefab {
            // TODO: this isn't implemented
            gfx: Graphic::ParticleSystem,
        },));

        let player = Player::new(root_transform, gfx_prefab, Spatial::default());
        let _player_id = world.spawn(player);

        let entity = {
            let mut query = world.query::<(&Camera, &Spatial)>();
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
