use std::sync::Arc;
use std::time::Duration;

use nalgebra::{Matrix4, Perspective3, Scalar, Vector3};

use crate::{tree::TreeError, Identifyable, Identity, World};

#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone)]
pub enum FacetIndex {
    Physical(u32),
    Health(u32),
    Camera(u32),
    Model(u32),
}

pub struct ModelInstanceFacet<U = f32>
where
    U: Scalar,
{
    pub transform: Matrix4<U>,
    pub model: Arc<models::Model>,
}

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

pub enum Shape {
    Box { width: f32, height: f32, depth: f32 },
    Cone { radius: f32, height: f32 },
    Cylinder { radius: f32, height: f32 },
    Sphere { radius: f32 },
}

// TODO: Static v Dynamic
pub struct PhysicalFacet {
    pub position: Vector3<f32>,
    pub linear_velocity: Vector3<f32>,
    pub angular_velocity: Vector3<f32>,
    pub body: Shape,
    pub mass: f32,
}

pub struct CameraFacet {
    // TODO: pos and rotation should be part of PhysicalFacet
    pub pos: Vector3<f32>,
    pub view: Matrix4<f32>,
    pub pitch: f32,
    pub yaw: f32,

    pub rotation_speed: f32,
    pub movement_speed: f32,

    pub perspective: Perspective3<f32>,
    pub movement_dir: Option<Direction>,
    _dirty: bool,
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

// Just wrap Vector3::new to hide the rust-analyzer issue with Matrix::new conflicting with Vector3::new
#[inline(always)]
fn vec3(x: f32, y: f32, z: f32) -> Vector3<f32> {
    // Expected 4 arguments? No, not really. Not an error, actually rust-analyzer breaking.
    Vector3::new(x, y, z)
}

impl CameraFacet {
    pub fn new(pos: Vector3<f32>, pitch: f32, yaw: f32) -> Self {
        let mut c = CameraFacet {
            pos,
            pitch,
            yaw,
            rotation_speed: 1.0,
            movement_speed: 1.0,
            movement_dir: None,
            _dirty: false,
            view: Matrix4::<f32>::identity(),

            // TODO fix default perspective values
            perspective: Perspective3::<f32>::new(
                1.7,    //aspect
                0.75,   //fovy
                0.0,    // near
                1000.0, //far
            ),
        };
        c.update_view_matrix();
        c
    }

    pub fn set_perspective(&mut self, fov: f32, aspect: f32, near: f32, far: f32) {
        self.perspective = Perspective3::<f32>::new(aspect, fov, near, far);
    }

    pub fn update_aspect_ratio(&mut self, aspect: f32) {
        self.perspective.set_aspect(aspect);
    }

    pub fn forward(&self) -> Vector3<f32> {
        let rx = self.pitch;
        let ry = self.yaw;
        // Not an error, actually rust-analyzer breaking.
        let vec = vec3(-rx.cos() * ry.sin(), rx.sin(), rx.cos() * ry.cos());
        vec.normalize()
    }

    pub fn right(&self) -> Vector3<f32> {
        let y = vec3(1.0, 0.0, 0.0);
        let forward = self.forward();
        let cross = y.cross(&forward);
        cross.normalize()
    }

    pub fn up(&self) -> Vector3<f32> {
        let x = vec3(0.0, 1.0, 0.0);
        x.cross(&self.forward()).normalize()
    }

    pub fn update(&mut self, dt: &Duration) {
        let amount = (dt.as_millis() as f64 / 100.0) as f32;
        if let Some(move_dir) = &self.movement_dir {
            let m = self.movement_speed * amount;
            let d = match move_dir {
                Direction::Forward => self.forward(),
                Direction::Backward => -self.forward(),
                Direction::Right => self.right(),
                Direction::Left => -self.right(),
                Direction::Up => self.up(),
                Direction::Down => -self.up(),
            };
            self.pos += d * m;
        }
        self.update_view_matrix();
    }

    pub fn update_view_matrix(&mut self) {
        let rot = Matrix4::from_euler_angles(self.pitch, self.yaw, 0.0);
        let trans = Matrix4::new_translation(&self.pos);
        self.view = trans * rot;
        self._dirty = true;
    }
}

pub struct ThingBuilder<'a> {
    pub(crate) world: &'a mut World,
    pub(crate) facet_indices: Vec<FacetIndex>,
    pub(crate) maybe_child_of: Option<Identity>,
}

#[derive(thiserror::Error, Debug)]
pub enum ThingBuilderError {
    #[error("Too many objects added to world")]
    TooManyObjects,

    #[error("Unable to create child when parent identity {0} does not exist.")]
    UnableToCreateChild(u32),

    #[error("make branch error {0:?}")]
    TreeError(TreeError),
}

impl<'a> ThingBuilder<'a> {
    pub fn with_camera(mut self, camera: CameraFacet) -> Self {
        let cameras = &mut self.world.facets.cameras;
        let idx = cameras.len();
        cameras.push(camera);
        self.facet_indices.push(FacetIndex::Camera(idx as u32));
        self
    }

    // Transform should be used as the offset of drawing from the physical facet
    pub fn with_model(mut self, transform: Matrix4<f32>, model: Arc<models::Model>) -> Self {
        let models = &mut self.world.facets.models;
        let idx = models.len();
        models.push(ModelInstanceFacet { transform, model });
        self.facet_indices.push(FacetIndex::Model(idx as u32));
        self
    }

    pub fn with_physical(mut self, x: f32, y: f32, z: f32) -> Self {
        let physical = &mut self.world.facets.physical;
        let idx = physical.len();
        physical.push(PhysicalFacet {
            position: vec3(x, y, z),
            linear_velocity: vec3(0.0, 0.0, 0.1),
            angular_velocity: Vector3::identity(),
            body: Shape::Box {
                width: 1.0,
                height: 1.0,
                depth: 1.0,
            },
            mass: 1.0,
        });
        self.facet_indices.push(FacetIndex::Physical(idx as u32));
        self
    }

    pub fn as_child_of(mut self, id: Identity) -> Result<Self, ThingBuilderError> {
        if self.world.things.len() <= id as usize {
            return Err(ThingBuilderError::UnableToCreateChild(id));
        }
        self.maybe_child_of = Some(id);
        Ok(self)
    }

    pub fn emplace(self) -> Result<Identity, ThingBuilderError> {
        let id = self.world.things.len();
        if id > std::u32::MAX as usize {
            return Err(ThingBuilderError::TooManyObjects);
        }

        let id = id as Identity;
        let thing = Thing::new(id, self.facet_indices);
        self.world.things.push(thing);
        // for ref facet_index in self.facet_indices.iter() {
        //     if let model @ FacetIndex::Model(_) = facet_index {}
        // }
        Ok(id)
    }
}

#[derive(Hash, PartialEq, Eq, Debug, Clone)]
pub struct Thing {
    pub id: Identity,
    pub facets: Vec<FacetIndex>, // index pointers to WorldFacets' specific fields
}

impl Thing {
    pub fn new(id: Identity, facets: Vec<FacetIndex>) -> Self {
        Thing { id, facets }
    }

    pub fn get_camera_fi(&self) -> Option<FacetIndex> {
        self.facets
            .iter()
            .find(|i| matches!(i, FacetIndex::Camera(_)))
            .cloned()
    }

    pub fn get_model_fi(&self) -> Option<FacetIndex> {
        self.facets
            .iter()
            .find(|i| matches!(i, FacetIndex::Camera(_)))
            .cloned()
    }
}

impl Identifyable for Thing {
    fn identify(&self) -> Identity {
        self.id
    }
}
