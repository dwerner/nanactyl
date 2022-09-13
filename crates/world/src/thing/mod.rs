use std::time::Duration;

use nalgebra::{Matrix4, Perspective3, Vector3};

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct PhysicalIndex(pub(crate) u32);
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct HealthIndex(pub(crate) u32);
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CameraIndex(pub(crate) u32);
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct ModelIndex(pub(crate) u32);

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

#[derive(Debug, Clone)]
pub struct PhysicalFacet {
    pub position: Vector3<f32>,
    pub linear_velocity: Vector3<f32>,
    pub angular_velocity: Vector3<f32>,
    pub body: Shape,
    pub mass: f32,
}

impl PhysicalFacet {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self {
            position: Vector3::new(x, y, z),
            linear_velocity: Vector3::new(0.0, 0.0, 0.0),
            angular_velocity: Vector3::identity(),
            body: Shape::Box {
                width: 1.0,
                height: 1.0,
                depth: 1.0,
            },
            mass: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CameraFacet {
    pub view: Matrix4<f32>,
    pub perspective: Perspective3<f32>,
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
            view: Matrix4::<f32>::identity(),
            // TODO fix default perspective values
            perspective: Perspective3::<f32>::new(
                1.7,    //aspect
                0.75,   //fovy
                0.0,    // near
                1000.0, //far
            ),
        };
        c.update_view_matrix(phys);
        c
    }

    pub fn set_perspective(&mut self, fov: f32, aspect: f32, near: f32, far: f32) {
        self.perspective = Perspective3::<f32>::new(aspect, fov, near, far);
    }

    pub fn update_aspect_ratio(&mut self, aspect: f32) {
        self.perspective.set_aspect(aspect);
    }

    pub fn forward(&self, phys: &PhysicalFacet) -> Vector3<f32> {
        let rx = phys.angular_velocity.x;
        let ry = phys.angular_velocity.y;
        let vec = {
            let x = -rx.cos() * ry.sin();
            let y = rx.sin();
            let z = rx.cos() * ry.cos();
            // Expected 4 arguments? No, not really. Not an error, actually rust-analyzer breaking.
            Vector3::new(x, y, z)
        };
        vec.normalize()
    }

    pub fn right(&self, phys: &PhysicalFacet) -> Vector3<f32> {
        let y = Vector3::new(1.0, 0.0, 0.0);
        let forward = self.forward(phys);
        let cross = y.cross(&forward);
        cross.normalize()
    }

    pub fn up(&self, phys: &PhysicalFacet) -> Vector3<f32> {
        let x = Vector3::new(0.0, 1.0, 0.0);
        x.cross(&self.forward(phys)).normalize()
    }

    pub fn update(&mut self, dt: &Duration, phys: &mut PhysicalFacet) {
        let amount = (dt.as_millis() as f64 / 100.0) as f32;
        phys.position += phys.linear_velocity * amount;
        self.update_view_matrix(phys);
    }

    pub fn update_view_matrix(&mut self, phys: &PhysicalFacet) {
        let rot = Matrix4::from_euler_angles(phys.angular_velocity.x, phys.angular_velocity.y, 0.0);
        let trans = Matrix4::new_translation(&phys.position);
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
    pub fn model_object(phys: PhysicalIndex, model: ModelIndex) -> Self {
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
