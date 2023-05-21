use std::slice::IterMut;
use std::time::Duration;

use glam::{Mat4, Vec3};

use super::index::{GfxIndex, PlayerIndex};
use super::Archetype;
use crate::thing::{HealthFacet, Shape, EULER_ROT_ORDER};

pub struct PlayerArchetype {
    //camera
    pub view: Vec<Mat4>,

    pub perspective: Vec<Mat4>,

    pub gfx: Vec<GfxIndex>,

    /// Absolute position.
    pub position: Vec<Vec3>,

    /// Absolute actual angles of the object. Used for updates and rendering.
    pub angles: Vec<Vec3>,

    /// Absolute scale.
    pub scale: Vec<f32>,

    /// Intended linear velocity. Updated from input.
    pub linear_velocity_intention: Vec<Vec3>,

    /// Intended angular velocity. Updated from input.
    pub angular_velocity_intention: Vec<Vec3>,

    /// Basic shape and params for colliders to be built from.
    pub shape: Vec<Shape>,

    pub health: Vec<HealthFacet>,
}

pub struct PlayerRef<'a> {
    pub gfx: &'a mut GfxIndex,
    pub view: &'a mut Mat4,
    pub perspective: &'a mut Mat4,
    pub position: &'a mut Vec3,
    pub angles: &'a mut Vec3,
    pub scale: &'a mut f32,
    pub linear_velocity_intention: &'a mut Vec3,
    pub angular_velocity_intention: &'a mut Vec3,
    pub shape: &'a mut Shape,
    pub health: &'a mut HealthFacet,
}

#[derive(Debug)]
pub struct PlayerBuilder {
    gfx: Option<GfxIndex>,
    position: Option<Vec3>,
    view: Option<Mat4>,
    perspective: Option<Mat4>,
    angles: Option<Vec3>,
    scale: Option<f32>,
    linear_velocity_intention: Option<Vec3>,
    angular_velocity_intention: Option<Vec3>,
    shape: Option<Shape>,
    health: Option<HealthFacet>,
}

// we intentionally don't want to expose a Default impl
fn default_player() -> PlayerBuilder {
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
        scale: Some(0.0),
        linear_velocity_intention: Some(Vec3::ZERO),
        angular_velocity_intention: Some(Vec3::ZERO),
        shape: Some(Shape::cuboid(1.0, 1.0, 1.0)),
        health: Some(HealthFacet::new(100)),
    }
}

impl PlayerBuilder {
    pub fn new(gfx: GfxIndex, pos: Vec3, shape: Shape) -> Self {
        Self {
            gfx: Some(gfx),
            position: Some(pos),
            shape: Some(shape),
            ..default_player()
        }
    }

    pub fn gfx(&mut self, gfx: GfxIndex) {
        self.gfx = Some(gfx);
    }

    pub fn position(&mut self, pos: Vec3) {
        self.position = Some(pos);
    }

    pub fn view(&mut self, view: Mat4) {
        self.view = Some(view);
    }

    pub fn perspective(&mut self, perspective: Mat4) {
        self.perspective = Some(perspective);
    }

    pub fn angles(&mut self, angles: Vec3) {
        self.angles = Some(angles);
    }

    pub fn scale(&mut self, scale: f32) {
        self.scale = Some(scale);
    }

    pub fn linear_velocity_intention(&mut self, linear_velocity_intention: Vec3) {
        self.linear_velocity_intention = Some(linear_velocity_intention);
    }

    pub fn angular_velocity_intention(&mut self, angular_velocity_intention: Vec3) {
        self.angular_velocity_intention = Some(angular_velocity_intention);
    }

    pub fn shape(&mut self, shape: Shape) {
        self.shape = Some(shape);
    }

    pub fn health(&mut self, health: HealthFacet) {
        self.health = Some(health);
    }
}

pub struct PlayerIterator<'a> {
    position: IterMut<'a, Vec3>,
    angles: IterMut<'a, Vec3>,
    scale: IterMut<'a, f32>,
    linear_velocity_intention: IterMut<'a, Vec3>,
    angular_velocity_intention: IterMut<'a, Vec3>,
    view: IterMut<'a, Mat4>,
    perspective: IterMut<'a, Mat4>,
    gfx: IterMut<'a, GfxIndex>,
    health: IterMut<'a, HealthFacet>,
    shape: IterMut<'a, Shape>,
}

impl<'a> Iterator for PlayerIterator<'a> {
    type Item = PlayerRef<'a>;
    fn next(&mut self) -> Option<PlayerRef<'a>> {
        let position = self.position.next()?;
        let angles = self.angles.next()?;
        let scale = self.scale.next()?;
        let linear_velocity_intention = self.linear_velocity_intention.next()?;
        let angular_velocity_intention = self.angular_velocity_intention.next()?;
        let view = self.view.next()?;
        let perspective = self.perspective.next()?;
        let gfx = self.gfx.next()?;
        let health = self.health.next()?;
        let shape = self.shape.next()?;
        Some(PlayerRef {
            health,
            shape,
            position,
            angles,
            scale,
            linear_velocity_intention,
            angular_velocity_intention,
            view,
            perspective,
            gfx,
        })
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
        let rot = Mat4::from_euler(
            EULER_ROT_ORDER,
            self.angular_velocity_intention.x,
            self.angular_velocity_intention.y,
            0.0,
        );
        let trans = Mat4::from_translation(*self.position);
        *self.view = trans * rot;
    }
}

impl Archetype for PlayerArchetype {
    type Index = PlayerIndex;
    type Error = PlayerError;

    type ItemRef<'a> = PlayerRef<'a>;
    type IterMut<'a> = PlayerIterator<'a>;
    type Builder = PlayerBuilder;

    fn len(&self) -> Self::Index {
        (self.view.len().saturating_sub(1)).into()
    }

    fn iter_mut(&mut self) -> Self::IterMut<'_> {
        PlayerIterator {
            position: self.position.iter_mut(),
            angles: self.angles.iter_mut(),
            scale: self.scale.iter_mut(),
            linear_velocity_intention: self.linear_velocity_intention.iter_mut(),
            angular_velocity_intention: self.angular_velocity_intention.iter_mut(),
            view: self.view.iter_mut(),
            perspective: self.perspective.iter_mut(),
            gfx: self.gfx.iter_mut(),
            shape: self.shape.iter_mut(),
            health: self.health.iter_mut(),
        }
    }

    fn get_mut(&mut self, index: Self::Index) -> Option<Self::ItemRef<'_>> {
        let index: usize = index.into();
        let position = self.position.get_mut(index)?;
        let angles = self.angles.get_mut(index)?;
        let scale = self.scale.get_mut(index)?;
        let linear_velocity_intention = self.linear_velocity_intention.get_mut(index)?;
        let angular_velocity_intention = self.angular_velocity_intention.get_mut(index)?;
        let view = self.view.get_mut(index)?;
        let perspective = self.perspective.get_mut(index)?;
        let gfx = self.gfx.get_mut(index)?;
        let health = self.health.get_mut(index)?;
        let shape = self.shape.get_mut(index)?;
        Some(PlayerRef {
            position,
            angles,
            scale,
            linear_velocity_intention,
            angular_velocity_intention,
            view,
            perspective,
            gfx,
            health,
            shape,
        })
    }

    /// Returns an error if the builder isn't valid.
    fn spawn(&mut self, builder: Self::Builder) -> Result<Self::Index, PlayerError> {
        match builder {
            PlayerBuilder {
                gfx: Some(gfx),
                position: Some(position),
                angles: Some(angles),
                scale: Some(scale),
                linear_velocity_intention: Some(linear_velocity_intention),
                angular_velocity_intention: Some(angular_velocity_intention),
                view: Some(view),
                perspective: Some(perspective),
                shape: Some(shape),
                health: Some(health),
            } => {
                self.gfx.push(gfx);
                self.position.push(position);
                self.angles.push(angles);
                self.scale.push(scale);
                self.linear_velocity_intention
                    .push(linear_velocity_intention);
                self.angular_velocity_intention
                    .push(angular_velocity_intention);
                self.view.push(view);
                self.perspective.push(perspective);
                self.shape.push(shape);
                self.health.push(health);
                Ok((self.view.len() - 1).into())
            }
            _ => Err(PlayerError::IncompleteBuilder { builder }),
        }
    }

    fn despawn(&mut self, _index: Self::Index) -> Result<(), Self::Error> {
        todo!()
    }

    fn builder(&self) -> Self::Builder {
        todo!()
    }

    fn set_default_builder(&mut self, _builder: Self::Builder) {
        todo!()
    }
}

impl PlayerArchetype {
    pub fn new() -> Self {
        Self {
            view: Vec::new(),
            perspective: Vec::new(),
            gfx: Vec::new(),
            position: Vec::new(),
            angles: Vec::new(),
            scale: Vec::new(),
            linear_velocity_intention: Vec::new(),
            angular_velocity_intention: Vec::new(),
            shape: Vec::new(),
            health: Vec::new(),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum PlayerError {
    #[error("Player archetype is incomplete {builder:?}")]
    IncompleteBuilder { builder: PlayerBuilder },
}
