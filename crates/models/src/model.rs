use std::fmt::Debug;
use std::path::Path;
use std::path::PathBuf;

// TODO: still need to refactor nom-obj to take BufReader, among other things
use obj_parser::model::{Interleaved, Mtl, MtlError, Obj, ObjError};

#[derive(Debug, Clone)]
pub struct Material {
    pub path: PathBuf,
    pub diffuse_map: Image,
}

#[derive(Clone)]
pub struct Image {
    pub path: PathBuf,
    pub image: image::DynamicImage,
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
pub enum ModelLoadError {
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
    pub path: PathBuf,
    pub material: Material,
    pub mesh: Mesh,
}

impl Model {
    pub fn load(filename: impl AsRef<Path>) -> Result<Self, ModelLoadError> {
        let parsed_obj = Obj::load(&filename).map_err(ModelLoadError::Obj)?;

        let obj = parsed_obj
            .objects
            .get(0)
            .ok_or(ModelLoadError::ObjHasMultipleModelsDefined)?;

        let Interleaved { v_vt_vn, idx } = obj.interleaved();

        let verts = v_vt_vn
            .iter()
            .map(|&(v, vt, vn)| Vertex::new((v.0, v.1, v.2), vt, vn))
            .collect::<Vec<_>>();

        if verts.is_empty() {
            return Err(ModelLoadError::ModelHasNoVerts);
        }

        let indices = idx.iter().map(|x: &usize| *x as u16).collect::<Vec<_>>();

        let mtl_file_path = &parsed_obj.objects[0]
            .material
            .as_ref()
            .ok_or(ModelLoadError::NoMaterial)?;

        let base_filename = filename.as_ref().to_path_buf();
        let base_path = base_filename.clone().parent().unwrap().to_path_buf();
        let mut material_path = base_path.clone();
        material_path.push(mtl_file_path);

        let mtl = Mtl::load(&material_path).map_err(ModelLoadError::Mtl)?;
        let diffuse_map_stem = mtl
            .diffuse_map_filename
            .ok_or(ModelLoadError::NoDiffuseMap)?;
        let mut diffuse_map_path = base_path.clone();
        diffuse_map_path.push(diffuse_map_stem);
        let diffuse_map_data =
            image::open(&diffuse_map_path).map_err(|err| ModelLoadError::UnableToLoadImage {
                err,
                path: diffuse_map_path.clone(),
            })?;
        let diffuse_map = Image {
            path: Path::new(&diffuse_map_path).to_path_buf(),
            image: diffuse_map_data,
        };

        Ok(Model {
            path: base_filename,
            mesh: Mesh::create(verts, indices),
            material: Material {
                path: material_path,
                diffuse_map,
            },
        })
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Vector(pub f32, pub f32, pub f32);

#[derive(Debug, Copy, Clone)]
pub struct UVW(pub f32, pub f32, pub f32);

#[derive(Debug, Copy, Clone)]
pub struct Normal(pub f32, pub f32, pub f32);

#[derive(Debug, Copy, Clone)]
pub struct Vertex {
    pub position: Vector,
    pub uvw: UVW,
    pub normal: Normal,
}

impl Vertex {
    pub fn new(v: (f32, f32, f32), vt: (f32, f32, f32), vn: (f32, f32, f32)) -> Self {
        Vertex {
            position: Vector(v.0, v.1, v.2),
            uvw: UVW(vt.0, vt.1, vt.2),
            normal: Normal(vn.0, vn.1, vn.2),
        }
    }

    pub fn from_parts(v: Vector, u: UVW, n: Normal) -> Self {
        Vertex {
            position: v,
            uvw: u,
            normal: n,
        }
    }
}

#[derive(Clone)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
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
    pub fn create(vertices: Vec<Vertex>, indices: Vec<u16>) -> Self {
        Mesh { vertices, indices }
    }
}
