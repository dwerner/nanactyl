use std::time::Duration;

use glam::{EulerRot, Mat4, Vec3};

pub const EULER_ROT_ORDER: EulerRot = EulerRot::XYZ;

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct PhysicalIndex(pub(crate) u32);
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct HealthIndex(pub(crate) u32);
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CameraIndex(pub(crate) u32);
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct ModelIndex(pub(crate) u32);

impl From<u16> for CameraIndex {
    fn from(value: u16) -> Self {
        Self(value as u32)
    }
}

impl From<CameraIndex> for u16 {
    fn from(value: CameraIndex) -> Self {
        value.0 as u16
    }
}

impl From<u32> for CameraIndex {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<u16> for PhysicalIndex {
    fn from(value: u16) -> Self {
        Self(value as u32)
    }
}

impl From<u32> for PhysicalIndex {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<PhysicalIndex> for u32 {
    fn from(value: PhysicalIndex) -> Self {
        value.0
    }
}

impl From<u16> for ModelIndex {
    fn from(value: u16) -> Self {
        Self(value as u32)
    }
}

impl From<ModelIndex> for u16 {
    fn from(value: ModelIndex) -> Self {
        value.0 as u16
    }
}

impl From<u32> for ModelIndex {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<u32> for HealthIndex {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<usize> for CameraIndex {
    fn from(value: usize) -> Self {
        Self(value as u32)
    }
}

impl From<usize> for PhysicalIndex {
    fn from(value: usize) -> Self {
        Self(value as u32)
    }
}

impl From<usize> for ModelIndex {
    fn from(value: usize) -> Self {
        Self(value as u32)
    }
}

impl From<usize> for HealthIndex {
    fn from(value: usize) -> Self {
        Self(value as u32)
    }
}

#[derive(Debug, Clone)]
pub struct ModelFacet {
    pub model: models::Model,
}
impl ModelFacet {
    pub fn new(model: models::Model) -> Self {
        Self { model }
    }
}

#[derive(Debug, Clone)]
pub struct HealthFacet {
    pub hp: u32,
}

impl HealthFacet {
    pub fn new(hp: u32) -> Self {
        HealthFacet { hp }
    }
    pub fn take_dmg(&mut self, dmg: u32) {
        if dmg > self.hp {
            self.hp = 0;
        } else {
            self.hp -= dmg;
        }
    }
    pub fn is_alive(&self) -> bool {
        self.hp > 0
    }
}

#[derive(Debug, Clone)]
pub enum Shape {
    Box { width: f32, height: f32, depth: f32 },
    Cone { radius: f32, height: f32 },
    Cylinder { radius: f32, height: f32 },
    Sphere { radius: f32 },
}

#[derive(Clone)]
pub struct PhysicalFacet {
    pub position: Vec3,
    pub angles: Vec3,
    pub scale: f32,
    pub linear_velocity: Vec3,
    pub angular_velocity: Vec3,
}

impl PhysicalFacet {
    pub fn new(x: f32, y: f32, z: f32, scale: f32, _TODO_bounding_mesh: &models::Mesh) -> Self {
        Self {
            position: Vec3::new(x, y, z),
            angles: Vec3::ZERO,
            scale,
            linear_velocity: Vec3::new(0.0, 0.0, 0.0),
            angular_velocity: Vec3::new(0.0, 0.0, 0.0),
        }
    }
}

impl std::fmt::Debug for PhysicalFacet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicalFacet")
            .field("position", &self.position)
            .field("scale", &self.scale)
            .field("angles", &self.angles)
            .field("linear_velocity", &self.linear_velocity)
            .field("angular_velocity", &self.angular_velocity)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct CameraFacet {
    pub view: Mat4,
    pub perspective: Mat4,
    pub associated_model: Option<ModelIndex>,
}

#[derive(Debug)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
    Forward,
    Backward,
}

impl CameraFacet {
    pub fn new(phys: &PhysicalFacet) -> Self {
        let mut c = CameraFacet {
            view: Mat4::IDENTITY,
            // TODO fix default perspective values
            perspective: Mat4::perspective_lh(
                1.7,    //aspect
                0.75,   //fovy
                0.1,    // near
                1000.0, //far
            ),
            associated_model: None,
        };
        c.update_view_matrix(phys);
        c
    }

    pub fn set_associated_model(&mut self, model: ModelIndex) {
        self.associated_model = Some(model);
    }

    pub fn set_perspective(&mut self, fov: f32, aspect: f32, near: f32, far: f32) {
        self.perspective = Mat4::perspective_lh(aspect, fov, near, far);
    }

    pub fn forward(&self, phys: &PhysicalFacet) -> Vec3 {
        let rx = phys.angles.x;
        let ry = phys.angles.y;
        let vec = {
            let x = -rx.cos() * ry.sin();
            let y = rx.sin();
            let z = rx.cos() * ry.cos();
            Vec3::new(x, y, z)
        };
        vec.normalize()
    }

    pub fn right(&self, phys: &PhysicalFacet) -> Vec3 {
        let y = Vec3::new(1.0, 0.0, 0.0);
        let forward = self.forward(phys);
        let cross = y.cross(forward);
        cross.normalize()
    }

    pub fn up(&self, phys: &PhysicalFacet) -> Vec3 {
        let x = Vec3::new(0.0, 1.0, 0.0);
        x.cross(self.forward(phys)).normalize()
    }

    pub fn update(&mut self, dt: &Duration, phys: &mut PhysicalFacet) {
        let amount = (dt.as_millis() as f64 / 100.0) as f32;
        phys.position += phys.linear_velocity * amount;
        self.update_view_matrix(phys);
    }

    pub fn update_view_matrix(&mut self, phys: &PhysicalFacet) {
        let rot = Mat4::from_euler(
            EULER_ROT_ORDER,
            phys.angular_velocity.x,
            phys.angular_velocity.y,
            0.0,
        );
        let trans = Mat4::from_translation(phys.position);
        self.view = trans * rot;
    }
}

#[derive(Hash, PartialEq, Eq, Debug, Clone)]
pub enum ThingType {
    Camera {
        phys: PhysicalIndex,
        camera: CameraIndex,
    },
    ModelObject {
        phys: PhysicalIndex,
        model: ModelIndex,
    },
}

#[derive(Hash, PartialEq, Eq, Debug, Clone)]
pub struct Thing {
    pub facets: ThingType,
}

impl Thing {
    pub fn model(phys: PhysicalIndex, model: ModelIndex) -> Self {
        Thing {
            facets: ThingType::ModelObject { phys, model },
        }
    }
    pub fn camera(phys: PhysicalIndex, camera: CameraIndex) -> Self {
        Thing {
            facets: ThingType::Camera { phys, camera },
        }
    }
}
