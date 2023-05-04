use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::time::Instant;

use image::GenericImageView;
use obj_parser::model::{Interleaved, Mtl, MtlError, Obj, ObjError};

#[derive(Debug, Clone)]
pub struct Material {
    pub path: PathBuf,
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
    pub path: PathBuf,
    pub loaded_time: Instant,
    pub material: Material,
    pub mesh: Mesh,
    pub vertex_shader: PathBuf,
    pub fragment_shader: PathBuf,
}

impl Model {
    pub fn load(
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
            path: base_filename,
            mesh,
            loaded_time: Instant::now(),
            material: Material {
                path: material_path,
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
