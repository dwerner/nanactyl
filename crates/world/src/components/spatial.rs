use glam::{Mat4, Vec3};
use hecs::Entity;

use crate::graphics::EULER_ROT_ORDER;

/// TODO: Docs
/// A component representing a relative transform from a parent.
/// Hierarchical transform relative to a parent.
#[derive(Debug)]
pub struct SpatialHierarchyNode {
    /// The parent of this node.
    pub parent: Entity,

    /// The transform of this node relative to its parent.
    pub transform: Mat4,

    /// Whether this node has been updated since the last world transform
    updated: bool,
}

// Really should be a world transform.
impl SpatialHierarchyNode {
    /// Construct a new node with a parent.
    pub fn new(parent: Entity) -> Self {
        Self {
            updated: true,
            transform: Mat4::IDENTITY,
            parent,
        }
    }

    /// Construct a new node with a parent and a scale.
    pub fn new_with_scale(parent: Entity, scale: f32) -> Self {
        Self {
            transform: Mat4::from_scale(Vec3::ONE * scale),
            updated: true,
            parent,
        }
    }

    /// Construct a new node with a parent and a position.
    pub fn new_at(parent: Entity, pos: Vec3) -> Self {
        Self {
            transform: Mat4::from_translation(pos),
            updated: true,
            parent,
        }
    }

    /// Construct a new node from the existing one with a new rotation in
    /// EULER_ROT_ORDER.
    pub fn with_angles(self, angles: Vec3) -> Self {
        let rotation = Mat4::from_euler(EULER_ROT_ORDER, angles.x, angles.y, angles.z);
        let transform = self.transform * rotation;
        Self {
            transform,
            updated: true,
            parent: self.parent,
        }
    }

    /// Mark this node as dirty, so that it will be used by the world transform
    /// update.
    pub fn mark_updated(&mut self) {
        self.updated = true;
    }

    /// Mark this node as clean, so that it will not be used by the world
    /// transform
    pub fn set_clean(&mut self) {
        self.updated = false;
    }

    /// Returns true if this node is dirty.
    pub fn is_dirty(&self) -> bool {
        self.updated
    }

    /// Translate this node to a position.
    pub fn translate(&mut self, translation: Vec3) {
        self.mark_updated();
        self.transform = Mat4::from_translation(translation) * self.transform;
    }

    /// Rotate this node by euler angles in EULER_ROT_ORDER.
    pub fn local_rotate(&mut self, angles: Vec3) {
        let rot = Mat4::from_euler(EULER_ROT_ORDER, angles.x, angles.y, angles.z);
        self.transform = self.transform * rot;
        self.mark_updated();
    }

    /// Scale this node by a factor.
    pub fn scale(&mut self, scale: Vec3) {
        self.transform = Mat4::from_scale(scale) * self.transform;
        self.mark_updated();
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

    /// get a vector pointing forward from this transform.
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
