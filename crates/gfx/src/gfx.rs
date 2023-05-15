use std::fmt::Debug;
use std::path::{Path, PathBuf};

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

    fn mesh_vertices(&self) -> &[Vertex] {
        self.mesh.vertices.as_slice()
    }

    fn mesh_indices(&self) -> &[u32] {
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
}

impl GpuNeeds for DebugMesh {
    fn fragment_shader_path(&self) -> &Path {
        Path::new("assets/shaders/debug_frag.spv")
    }

    fn vertex_shader_path(&self) -> &Path {
        Path::new("assets/shaders/debug_vert.spv")
    }

    fn mesh_vertices(&self) -> &[Vertex] {
        self.vertices.as_slice()
    }

    fn mesh_indices(&self) -> &[u32] {
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

// TODO: consider an interface for all graphics drawable types
pub trait GpuNeeds {
    fn fragment_shader_path(&self) -> &Path;
    fn vertex_shader_path(&self) -> &Path;
    fn mesh_vertices(&self) -> &[Vertex];
    fn mesh_indices(&self) -> &[u32];
    fn diffuse_color(&self) -> Option<DiffuseColor<'_>>;
}

pub enum Graphic {
    Model(Model),
    DebugMesh(DebugMesh),
}

impl Graphic {
    pub fn new_model(model: Model) -> Self {
        Graphic::Model(model)
    }

    pub fn new_debug_mesh(vertices: Vec<Vertex>, indices: Vec<u32>, color: Vec4) -> Self {
        let mesh = DebugMesh {
            vertices,
            indices,
            color,
        };
        Graphic::DebugMesh(mesh)
    }
}

impl GpuNeeds for Graphic {
    fn fragment_shader_path(&self) -> &Path {
        match self {
            Graphic::Model(model) => model.fragment_shader_path(),
            Graphic::DebugMesh(mesh) => mesh.fragment_shader_path(),
        }
    }

    fn vertex_shader_path(&self) -> &Path {
        match self {
            Graphic::Model(model) => model.vertex_shader_path(),
            Graphic::DebugMesh(mesh) => mesh.vertex_shader_path(),
        }
    }

    fn mesh_vertices(&self) -> &[Vertex] {
        match self {
            Graphic::Model(model) => model.mesh_vertices(),
            Graphic::DebugMesh(mesh) => mesh.mesh_vertices(),
        }
    }

    fn mesh_indices(&self) -> &[u32] {
        match self {
            Graphic::Model(model) => model.mesh_indices(),
            Graphic::DebugMesh(mesh) => mesh.mesh_indices(),
        }
    }

    fn diffuse_color(&self) -> Option<DiffuseColor<'_>> {
        match self {
            Graphic::Model(model) => model.diffuse_color(),
            Graphic::DebugMesh(mesh) => mesh.diffuse_color(),
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
