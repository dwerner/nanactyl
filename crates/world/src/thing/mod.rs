use gfx::{DebugMesh, Graphic, Model, Primitive, Vertex};
use glam::{EulerRot, Mat4, Vec3, Vec4};

use crate::archetypes::index::GfxIndex;

pub const EULER_ROT_ORDER: EulerRot = EulerRot::XYZ;

/// Index to address a physical object.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct PhysicalIndex(pub(crate) u32);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct HealthIndex(pub(crate) u32);

/// Index to address a camera.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct CameraIndex(pub(crate) u32);

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

impl From<usize> for HealthIndex {
    fn from(value: usize) -> Self {
        Self(value as u32)
    }
}

pub struct GraphicsFacet {
    pub gfx: Graphic,
}

impl GraphicsFacet {
    pub fn from_model(model: Model) -> Self {
        Self {
            gfx: Graphic::new_model(model),
        }
    }

    /// Convert a model into a linestrip mesh
    pub fn into_debug_mesh(self) -> Self {
        Self {
            gfx: self
                .gfx
                .into_debug_mesh(Vec4::new(1.0, 0.0, 0.0, 1.0), Primitive::LineStrip),
        }
    }

    pub fn with_debug_mesh(debug_mesh: DebugMesh) -> Self {
        Self {
            gfx: Graphic::DebugMesh(debug_mesh),
        }
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
    Cuboid { width: f32, height: f32, depth: f32 },
    Cylinder { radius: f32, height: f32 },
    Capsule { radius: f32, height: f32 },
    Sphere { radius: f32 },
}

impl Shape {
    pub fn cuboid(width: f32, height: f32, depth: f32) -> Self {
        Shape::Cuboid {
            width,
            height,
            depth,
        }
    }

    pub fn cylinder(radius: f32, height: f32) -> Self {
        Shape::Cylinder { radius, height }
    }

    pub fn capsule(radius: f32, height: f32) -> Self {
        Shape::Capsule { radius, height }
    }

    pub fn sphere(radius: f32) -> Self {
        Shape::Sphere { radius }
    }

    pub fn into_debug_mesh(&self, color: Vec4) -> DebugMesh {
        let (vertices, indices) = match self {
            Shape::Cuboid {
                width,
                height,
                depth,
            } => generate_cuboid(*width, *height, *depth),
            Shape::Cylinder { radius, height } => generate_cylinder(*radius, *height, 10),
            Shape::Sphere { radius } => generate_sphere(*radius, 10),
            Shape::Capsule { radius, height } => generate_capsule(*radius, *height, 10),
        };
        DebugMesh::line_list(vertices, indices, color)
    }
}

fn generate_cuboid(width: f32, height: f32, depth: f32) -> (Vec<Vertex>, Vec<u32>) {
    let w = width / 2.0;
    let h = height / 2.0;
    let d = depth / 2.0;

    let vertices = vec![
        // Bottom square
        Vertex::pos(-w, -h, -d),
        Vertex::pos(w, -h, -d),
        Vertex::pos(w, -h, d),
        Vertex::pos(-w, -h, d),
        // Top square
        Vertex::pos(-w, h, -d),
        Vertex::pos(w, h, -d),
        Vertex::pos(w, h, d),
        Vertex::pos(-w, h, d),
    ];

    let indices = vec![
        // Bottom square
        0, 1, 1, 2, 2, 3, 3, 0, // Top square
        4, 5, 5, 6, 6, 7, 7, 4, // Vertical lines
        0, 4, 1, 5, 2, 6, 3, 7,
    ];

    (vertices, indices)
}

pub fn generate_sphere(radius: f32, segments: usize) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    for i in 0..segments {
        for j in 0..segments {
            let theta1 = (i as f32) * 2.0 * std::f32::consts::PI / (segments as f32);
            let theta2 = ((i + 1) as f32) * 2.0 * std::f32::consts::PI / (segments as f32);

            let phi1 = (j as f32) * std::f32::consts::PI / (segments as f32);
            let phi2 = ((j + 1) as f32) * std::f32::consts::PI / (segments as f32);

            let x1 = radius * theta1.sin() * phi1.sin();
            let y1 = radius * phi1.cos();
            let z1 = radius * theta1.cos() * phi1.sin();

            let x2 = radius * theta2.sin() * phi1.sin();
            let y2 = radius * phi1.cos();
            let z2 = radius * theta2.cos() * phi1.sin();

            let x3 = radius * theta1.sin() * phi2.sin();
            let y3 = radius * phi2.cos();
            let z3 = radius * theta1.cos() * phi2.sin();

            let idx1 = (vertices.len() + 0) as u32;
            let idx2 = (vertices.len() + 1) as u32;
            let idx3 = (vertices.len() + 2) as u32;

            vertices.push(Vertex::pos(x1, y1, z1));
            vertices.push(Vertex::pos(x2, y2, z2));
            vertices.push(Vertex::pos(x3, y3, z3));

            indices.push(idx1);
            indices.push(idx2);
            indices.push(idx3);
        }
    }
    (vertices, indices)
}

pub fn generate_cylinder(radius: f32, height: f32, segments: usize) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    for i in 0..segments {
        let theta1 = (i as f32) * 2.0 * std::f32::consts::PI / (segments as f32);
        let theta2 = ((i + 1) as f32) * 2.0 * std::f32::consts::PI / (segments as f32);

        let x1 = radius * theta1.sin();
        let y1 = -height / 2.0;
        let z1 = radius * theta1.cos();

        let x2 = radius * theta2.sin();
        let y2 = -height / 2.0;
        let z2 = radius * theta2.cos();

        let x3 = radius * theta1.sin();
        let y3 = height / 2.0;
        let z3 = radius * theta1.cos();

        let x4 = radius * theta2.sin();
        let y4 = height / 2.0;
        let z4 = radius * theta2.cos();

        let idx1 = (vertices.len() + 0) as u32;
        let idx2 = (vertices.len() + 1) as u32;
        let idx3 = (vertices.len() + 2) as u32;
        let idx4 = (vertices.len() + 3) as u32;

        vertices.push(Vertex::pos(x1, y1, z1));
        vertices.push(Vertex::pos(x2, y2, z2));
        vertices.push(Vertex::pos(x3, y3, z3));
        vertices.push(Vertex::pos(x4, y4, z4));

        // Triangles for the side
        indices.push(idx1);
        indices.push(idx2);
        indices.push(idx3);

        indices.push(idx3);
        indices.push(idx2);
        indices.push(idx4);

        // Optionally, add code to generate the top and bottom cap of the
        // cylinder
    }

    (vertices, indices)
}

pub fn generate_capsule(radius: f32, height: f32, segments: usize) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    // Generate top hemisphere (sphere)
    let (mut top_hemisphere, mut top_indices) = generate_sphere(radius, segments);
    for vertex in &mut top_hemisphere {
        vertex.pos[1] += height / 2.0;
    }
    vertices.append(&mut top_hemisphere);
    indices.append(&mut top_indices);

    // Generate bottom hemisphere (sphere)
    let (mut bottom_hemisphere, mut bottom_indices) = generate_sphere(radius, segments);
    for vertex in &mut bottom_hemisphere {
        vertex.pos[1] -= height / 2.0;
    }
    // Offset the indices of the bottom hemisphere by the number of vertices in the
    // top hemisphere
    for index in &mut bottom_indices {
        *index += top_hemisphere.len() as u32;
    }
    vertices.append(&mut bottom_hemisphere);
    indices.append(&mut bottom_indices);

    // Generate cylinder
    let (mut cylinder, mut cylinder_indices) = generate_cylinder(radius, height, segments);
    // Offset the indices of the cylinder by the number of vertices in the top and
    // bottom hemispheres
    for index in &mut cylinder_indices {
        *index += (top_hemisphere.len() + bottom_hemisphere.len()) as u32;
    }
    vertices.append(&mut cylinder);
    indices.append(&mut cylinder_indices);

    (vertices, indices)
}

#[derive(Clone)]
pub struct PhysicalFacet {
    /// Absolute position.
    pub position: Vec3,

    /// Absolute actual angles of the object. Used for updates and rendering.
    pub angles: Vec3,

    /// Absolute scale.
    pub scale: f32,

    /// Intended linear velocity. Updated from input.
    pub linear_velocity_intention: Vec3,

    /// Intended angular velocity. Updated from input.
    pub angular_velocity_intention: Vec3,

    /// Basic shape and params for colliders to be built from.
    pub shape: Shape,
}

impl PhysicalFacet {
    /// Create a new physical facet.
    pub fn new(x: f32, y: f32, z: f32, scale: f32, shape: Shape) -> Self {
        Self {
            position: Vec3::new(x, y, z),
            angles: Vec3::ZERO,
            linear_velocity_intention: Vec3::ZERO,
            angular_velocity_intention: Vec3::ZERO,
            scale,
            shape,
        }
    }

    /// Create a new physical facet with a cuboid shape.
    pub fn new_cuboid(x: f32, y: f32, z: f32, scale: f32) -> Self {
        Self {
            position: Vec3::new(x, y, z),
            angles: Vec3::ZERO,
            linear_velocity_intention: Vec3::ZERO,
            angular_velocity_intention: Vec3::ZERO,
            scale,
            shape: Shape::Cuboid {
                width: scale,
                height: scale,
                depth: scale,
            },
        }
    }
}

impl std::fmt::Debug for PhysicalFacet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicalFacet")
            .field("position", &self.position)
            .field("scale", &self.scale)
            .field("angles", &self.angles)
            .field("linear_velocity", &self.linear_velocity_intention)
            .field("angular_velocity", &self.angular_velocity_intention)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct CameraFacet {
    pub view: Mat4,
    pub perspective: Mat4,
    pub associated_graphics: Option<GfxIndex>,
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

impl CameraFacet {}

#[derive(Hash, PartialEq, Eq, Debug, Clone)]
pub enum ThingType {
    Camera {
        phys: PhysicalIndex,
        camera: CameraIndex,
    },
    GraphicsObject {
        phys: PhysicalIndex,
        model: GfxIndex,
    },
}

#[derive(Hash, PartialEq, Eq, Debug, Clone)]
pub struct Thing {
    pub facets: ThingType,
}

impl Thing {
    pub fn model(phys: PhysicalIndex, model: GfxIndex) -> Self {
        Thing {
            facets: ThingType::GraphicsObject { phys, model },
        }
    }
    pub fn camera(phys: PhysicalIndex, camera: CameraIndex) -> Self {
        Thing {
            facets: ThingType::Camera { phys, camera },
        }
    }
}
