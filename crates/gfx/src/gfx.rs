use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::primitive;

use glam::Vec4;
use image::GenericImageView;
use obj_parser::model::{Interleaved, Mtl, MtlError, Obj, ObjError};

#[derive(Debug, Clone)]
pub struct Material {
    pub diffuse_map: Option<Image>,
    pub specular_map: Option<Image>,
    pub bump_map: Option<Image>,
}

#[derive(Clone)]
pub struct Image {
    pub path: PathBuf,
    pub image: image::DynamicImage,
}

impl Image {
    pub fn extent(&self) -> (u32, u32) {
        self.image.dimensions()
    }
}

impl Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("path", &self.path)
            .field("image", &"[<image data>]")
            .finish()
    }
}

#[derive(thiserror::Error, Debug)]
pub enum LoadError {
    #[error("obj has multiple models defined")]
    ObjHasMultipleModelsDefined,
    #[error("model has no vertices")]
    ModelHasNoVerts,
    #[error("obj {0:?}")]
    Obj(ObjError),
    #[error("Mtl {0:?}")]
    Mtl(MtlError),
    #[error("no material provided")]
    NoMaterial,
    #[error("no diffuse map provided")]
    NoDiffuseMap,
    #[error("unable to load image at {path:?} {err:?}")]
    UnableToLoadImage {
        err: image::ImageError,
        path: PathBuf,
    },
}

#[derive(Debug, Clone)]
pub struct Model {
    mesh: Mesh,
    vertex_shader: PathBuf,
    fragment_shader: PathBuf,
    material: Material,
}

impl Drop for Model {
    fn drop(&mut self) {
        println!("dropping model {:?}", self);
    }
}

impl GpuNeeds for Model {
    fn fragment_shader_path(&self) -> &Path {
        self.fragment_shader.as_path()
    }

    fn vertex_shader_path(&self) -> &Path {
        self.vertex_shader.as_path()
    }

    fn vertices(&self) -> &[Vertex] {
        self.mesh.vertices.as_slice()
    }

    fn indices(&self) -> &[u32] {
        self.mesh.indices.as_slice()
    }

    fn diffuse_color(&self) -> Option<DiffuseColor<'_>> {
        self.material
            .diffuse_map
            .as_ref()
            .map(DiffuseColor::Texture)
    }
}

#[derive(Debug, Clone)]
pub struct DebugMesh {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    color: Vec4,
    primitive: Primitive,
}

impl DebugMesh {
    /// Retrieve the primitive type for this debug mesh.
    pub fn primitive(&self) -> Primitive {
        self.primitive
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(u8)]
pub enum Primitive {
    PointList = 0,
    LineList = 1,
    LineStrip = 2,
    TriangleList = 3,
}

impl GpuNeeds for DebugMesh {
    fn fragment_shader_path(&self) -> &Path {
        Path::new("assets/shaders/spv/debug_mesh_fragment.spv")
    }

    fn vertex_shader_path(&self) -> &Path {
        Path::new("assets/shaders/spv/debug_mesh_vertex.spv")
    }

    fn vertices(&self) -> &[Vertex] {
        self.vertices.as_slice()
    }

    fn indices(&self) -> &[u32] {
        self.indices.as_slice()
    }

    fn diffuse_color(&self) -> Option<DiffuseColor<'_>> {
        Some(DiffuseColor::Color(self.color))
    }
}

pub enum DiffuseColor<'a> {
    Color(Vec4),
    Texture(&'a Image),
}

impl<'a> DiffuseColor<'a> {
    pub fn is_texture(&self) -> bool {
        matches!(self, DiffuseColor::Texture(_))
    }
    pub fn is_color(&self) -> bool {
        matches!(self, DiffuseColor::Color(_))
    }
}

/// Provide what we need to draw this object.
pub trait GpuNeeds {
    /// Return vertices of this object.
    fn vertices(&self) -> &[Vertex];

    /// Return indices of this object.
    fn indices(&self) -> &[u32];

    /// Return the diffuse color/map of this object.
    fn diffuse_color(&self) -> Option<DiffuseColor<'_>>;

    /// Return the path to the fragment shader.
    fn fragment_shader_path(&self) -> &Path;

    /// Return the path to the vertex shader.
    fn vertex_shader_path(&self) -> &Path;
}

/// A drawable object.
///
/// Add new variants here.
pub enum Graphic {
    /// A model, with a texture and shaders.
    Model(Model),

    /// A debug mesh, color and primitive types.
    DebugMesh(DebugMesh),

    // TODO
    ParticleSystem,
}

impl Graphic {
    /// Create a new graphic from a model.
    pub fn new_model(model: Model) -> Self {
        Graphic::Model(model)
    }

    /// Create a new debug mesh with parts.
    pub fn new_debug_mesh(
        vertices: Vec<Vertex>,
        indices: Vec<u32>,
        color: Vec4,
        primitive: Primitive,
    ) -> Self {
        let mesh = DebugMesh {
            vertices,
            indices,
            color,
            primitive,
        };
        Graphic::DebugMesh(mesh)
    }

    /// Convert a graphic into a debug mesh. For the model, it strips the
    /// material and applies a color.
    pub fn into_debug_mesh(self, color: Vec4, primitive: Primitive) -> Self {
        match self {
            s @ Graphic::DebugMesh(_) => s,
            Graphic::Model(model) => {
                let vertices = model.vertices().to_vec();
                let indices = model.indices().to_vec();
                Graphic::new_debug_mesh(vertices, indices, color, primitive)
            }
            Graphic::ParticleSystem => todo!(),
        }
    }

    pub fn primitive(&self) -> Primitive {
        match self {
            Graphic::Model(_) => Primitive::TriangleList,
            Graphic::DebugMesh(mesh) => mesh.primitive(),
            Graphic::ParticleSystem => todo!(),
        }
    }

    pub fn into_wireframe(self) -> Self {
        match self {
            Graphic::Model(model) => {
                let vertices = model.vertices().to_vec();
                let indices = model.indices().to_vec();
                Graphic::new_debug_mesh(
                    vertices,
                    indices,
                    Vec4::new(1.0, 0.0, 0.0, 1.0),
                    Primitive::LineList,
                )
            }
            Graphic::DebugMesh(mesh) => Graphic::new_debug_mesh(
                mesh.vertices,
                mesh.indices,
                Vec4::new(1.0, 0.0, 0.0, 1.0),
                Primitive::LineList,
            ),
            Graphic::ParticleSystem => todo!(),
        }
    }
}

impl GpuNeeds for Graphic {
    fn fragment_shader_path(&self) -> &Path {
        match self {
            Graphic::Model(model) => model.fragment_shader_path(),
            Graphic::DebugMesh(mesh) => mesh.fragment_shader_path(),
            Graphic::ParticleSystem => todo!(),
        }
    }

    fn vertex_shader_path(&self) -> &Path {
        match self {
            Graphic::Model(model) => model.vertex_shader_path(),
            Graphic::DebugMesh(mesh) => mesh.vertex_shader_path(),
            Graphic::ParticleSystem => todo!(),
        }
    }

    fn vertices(&self) -> &[Vertex] {
        match self {
            Graphic::Model(model) => model.vertices(),
            Graphic::DebugMesh(mesh) => mesh.vertices(),
            Graphic::ParticleSystem => todo!(),
        }
    }

    fn indices(&self) -> &[u32] {
        match self {
            Graphic::Model(model) => model.indices(),
            Graphic::DebugMesh(mesh) => mesh.indices(),
            Graphic::ParticleSystem => todo!(),
        }
    }

    fn diffuse_color(&self) -> Option<DiffuseColor<'_>> {
        match self {
            Graphic::Model(model) => model.diffuse_color(),
            Graphic::DebugMesh(mesh) => mesh.diffuse_color(),
            Graphic::ParticleSystem => todo!(),
        }
    }
}

impl Model {
    pub fn load_obj(
        filename: impl AsRef<Path>,
        vertex_shader: impl AsRef<Path>,
        fragment_shader: impl AsRef<Path>,
    ) -> Result<Self, LoadError> {
        let (mesh, obj) = Mesh::load(&filename)?;
        let obj = {
            obj.objects
                .get(0)
                .ok_or(LoadError::ObjHasMultipleModelsDefined)?
        };

        let mtl_file_path = &obj.material.as_ref().ok_or(LoadError::NoMaterial)?;

        let base_filename = filename.as_ref().to_path_buf();
        let base_path = base_filename.parent().unwrap().to_path_buf();
        let mut material_path = base_path.clone();
        material_path.push(mtl_file_path);

        let mtl = Mtl::load(&material_path).map_err(LoadError::Mtl)?;

        let diffuse_map = match mtl.diffuse_map_filename {
            Some(stem) => Some(load_image(&stem, &base_path)?),
            None => None,
        };
        let specular_map = match mtl.specular_map {
            Some(stem) => Some(load_image(&stem.specular_map_path, &base_path)?),
            None => None,
        };
        let bump_map = match mtl.bump_map_path {
            Some(stem) => Some(load_image(&stem, &base_path)?),
            None => None,
        };

        Ok(Model {
            mesh,
            material: Material {
                diffuse_map,
                specular_map,
                bump_map,
            },
            vertex_shader: vertex_shader.as_ref().to_path_buf(),
            fragment_shader: fragment_shader.as_ref().to_path_buf(),
        })
    }
}

fn load_image(stem: &str, base_path: &Path) -> Result<Image, LoadError> {
    let image_path = base_path.join(stem);
    let image_data = image::open(&image_path).map_err(|err| LoadError::UnableToLoadImage {
        err,
        path: image_path.clone(),
    })?;
    let diffuse_map = Image {
        path: Path::new(&image_path).to_path_buf(),
        image: image_data,
    };
    Ok(diffuse_map)
}

// TODO: consider a more efficient layout and reusable storage for vertices
// We want to copy less often.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct Vertex {
    pub pos: [f32; 4],
    pub uv: [f32; 2],
    pub normal: [f32; 3],
}

impl Vertex {
    pub fn new(v: (f32, f32, f32, f32), vt: (f32, f32, f32), vn: (f32, f32, f32)) -> Self {
        Vertex {
            pos: [v.0, v.1, v.2, v.3],
            uv: [vt.0, vt.1],
            normal: [vn.0, vn.1, vn.2],
        }
    }
}

#[derive(Clone)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl Debug for Mesh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mesh")
            .field("vertices", &self.vertices.len())
            .field("indices", &self.indices.len())
            .finish()
    }
}

impl Mesh {
    pub fn new(vertices: Vec<Vertex>, indices: Vec<u32>) -> Self {
        Mesh { vertices, indices }
    }

    /// Load a mesh from the given obj file at filename.
    /// TODO:
    ///     - reduce the use of tuples here
    ///     - similarly reduce the copying that is happening
    pub fn load(filename: impl AsRef<Path>) -> Result<(Self, Obj), LoadError> {
        let obj = Obj::load(&filename).map_err(LoadError::Obj)?;
        let object = {
            obj.objects
                .get(0)
                .ok_or(LoadError::ObjHasMultipleModelsDefined)?
        };
        let Interleaved { vertices, indices } = object.interleaved().map_err(LoadError::Obj)?;
        let verts = vertices
            .iter()
            .map(|&(v, vt, vn)| Vertex::new(v, vt, vn))
            .collect::<Vec<_>>();
        if verts.is_empty() {
            return Err(LoadError::ModelHasNoVerts);
        }
        Ok((Self::new(verts, indices), obj))
    }
}
