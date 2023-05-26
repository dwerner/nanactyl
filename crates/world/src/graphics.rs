use gfx::{DebugMesh, Vertex};
use glam::{EulerRot, Vec4};

pub const EULER_ROT_ORDER: EulerRot = EulerRot::XYZ;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Shape {
    Cuboid { width: f32, height: f32, depth: f32 },
    Cylinder { radius: f32, height: f32 },
    Capsule { radius: f32, height: f32 },
    Sphere { radius: f32 },
}

impl Default for Shape {
    fn default() -> Self {
        Shape::Cuboid {
            width: 1.0,
            height: 1.0,
            depth: 1.0,
        }
    }
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
