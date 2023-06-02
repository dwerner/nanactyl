use glam::{Mat4, Vec3};
use heks::Entity;

use crate::graphics::EULER_ROT_ORDER;

/// TODO: Docs
/// A component representing a relative transform from a parent.
/// Hierarchical transform relative to a parent.
#[derive(Debug)]
pub struct SpatialNode {
    pub parent: Entity,
    pub transform: Mat4,
    dirty: bool,
}

// Really should be a world transform.
impl SpatialNode {
    /// Construct a new node with a parent.
    pub fn new(parent: Entity) -> Self {
        Self {
            dirty: true,
            transform: Mat4::IDENTITY,
            parent,
        }
    }

    /// Construct a new node with a parent and a scale.
    pub fn new_with_scale(parent: Entity, scale: f32) -> Self {
        Self {
            transform: Mat4::from_scale(Vec3::ONE * scale),
            dirty: true,
            parent,
        }
    }

    /// Construct a new node with a parent and a position.
    pub fn new_at(parent: Entity, pos: Vec3) -> Self {
        Self {
            transform: Mat4::from_translation(pos),
            dirty: true,
            parent,
        }
    }

    /// Construct a new node from the existing one with a new rotation in
    /// EULER_ROT_ORDER.
    pub fn with_angles(self, angles: Vec3) -> Self {
        let transform =
            Mat4::from_euler(EULER_ROT_ORDER, angles.x, angles.y, angles.z) * self.transform;
        Self {
            transform,
            dirty: true,
            parent: self.parent,
        }
    }

    /// Mark this node as dirty, so that it will be used by the world transform
    /// update.
    pub fn set_dirty(&mut self) {
        self.dirty = true;
    }

    /// Mark this node as clean, so that it will not be used by the world
    /// transform
    pub fn set_clean(&mut self) {
        self.dirty = false;
    }

    /// Returns true if this node is dirty.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Translate this node to a position.
    pub fn translate(&mut self, translation: Vec3) {
        self.set_dirty();
        self.transform = Mat4::from_translation(translation) * self.transform;
    }

    /// Rotate this node by euler angles in EULER_ROT_ORDER.
    pub fn rotate(&mut self, angles: Vec3) {
        self.set_dirty();
        self.transform =
            Mat4::from_euler(EULER_ROT_ORDER, angles.x, angles.y, angles.z) * self.transform;
    }

    /// Scale this node by a factor.
    pub fn scale(&mut self, scale: Vec3) {
        self.set_dirty();
        self.transform = Mat4::from_scale(scale) * self.transform;
    }

    /// Get the position of this transform.
    pub fn get_pos(&self) -> Vec3 {
        let (_scale, _rot, trans) = self.transform.to_scale_rotation_translation();
        trans
    }

    /// Get the angles of rotation in EULER_ROT_ORDER.
    pub fn get_angles(&self) -> Vec3 {
        let (_scale, rot, _trans) = self.transform.to_scale_rotation_translation();
        rot.to_euler(EULER_ROT_ORDER).into()
    }

    /// Get the scale of this transform.
    pub fn get_scale(&self) -> Vec3 {
        let (scale, _rot, _trans) = self.transform.to_scale_rotation_translation();
        scale
    }

    /// Get a vector pointing forward from this transform.
    pub fn forward(&self) -> Vec3 {
        -self.transform.z_axis.truncate().normalize()
    }

    /// Get a vector pointing right from this transform.
    pub fn right(&self) -> Vec3 {
        self.transform.x_axis.truncate().normalize()
    }

    /// Get a vector pointing up from this transform.
    pub fn up(&self) -> Vec3 {
        self.transform.y_axis.truncate().normalize()
    }
}
