use std::time::Duration;

use glam::{Mat4, Vec3};

use crate::def_archetype;
use crate::graphics::{GfxIndex, Shape, EULER_ROT_ORDER};
use crate::health::HealthFacet;

def_archetype! {
    Player,
    gfx: GfxIndex,
    position: Vec3,
    view: Mat4,
    perspective: Mat4,
    angles: Vec3,
    scale: f32,
    linear_velocity_intention: Vec3,
    angular_velocity_intention: Vec3,
    shape: Shape,
    health: HealthFacet
}

impl PlayerBuilder {
    /// Create a player builder for spawning to the archetype.
    pub fn new(gfx: GfxIndex, pos: Vec3, shape: Shape) -> Self {
        Self {
            gfx: Some(gfx),
            position: Some(pos),
            shape: Some(shape),
            ..Default::default()
        }
    }
}

impl Default for PlayerBuilder {
    fn default() -> Self {
        let perspective = Mat4::perspective_lh(
            1.7,    //aspect
            0.75,   //fovy
            0.1,    // near
            1000.0, //far
        );
        PlayerBuilder {
            gfx: None,
            position: Some(Vec3::ZERO),
            view: Some(Mat4::IDENTITY),
            perspective: Some(perspective),
            angles: Some(Vec3::ZERO),
            scale: Some(1.0),
            linear_velocity_intention: Some(Vec3::ZERO),
            angular_velocity_intention: Some(Vec3::ZERO),
            shape: Some(Shape::cuboid(1.0, 1.0, 1.0)),
            health: Some(HealthFacet::new(100)),
        }
    }
}

impl<'a> PlayerRef<'a> {
    pub fn set_perspective(&mut self, fov: f32, aspect: f32, near: f32, far: f32) {
        *self.perspective = Mat4::perspective_lh(aspect, fov, near, far);
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

    pub fn update(&mut self, dt: &Duration) {
        let amount = (dt.as_millis() as f64 / 100.0) as f32;
        *self.position += *self.linear_velocity_intention * amount;
        self.update_view_matrix();
    }

    pub fn update_view_matrix(&mut self) {
        let rot = Mat4::from_euler(EULER_ROT_ORDER, self.angles.x, self.angles.y, 0.0);
        let trans = Mat4::from_translation(*self.position);
        *self.view = trans * rot;
    }
}
