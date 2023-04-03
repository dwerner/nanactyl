use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::path::Path;

use crate::parser::mtl::{MtlLine, MtlParser};
use crate::parser::obj::{FaceIndex, ObjLine, ObjParser};

type T3<T> = (T, T, T);
type T4<T> = (T, T, T, T);

#[derive(thiserror::Error, Debug)]
pub enum MtlError {
    #[error("unable to load mtl")]
    UnableToLoad(io::Error),
}

#[derive(thiserror::Error, Debug)]
pub enum ObjError {
    #[error("unable to load obj")]
    UnableToLoad(io::Error),
}

pub struct Mtl {
    pub diffuse_map_filename: Option<String>,
}

impl Mtl {
    pub fn load(mtl_file: impl AsRef<Path>) -> Result<Self, MtlError> {
        let file = File::open(&mtl_file).map_err(MtlError::UnableToLoad)?;
        let reader = BufReader::new(file);
        let mtl_parser = MtlParser::new(reader);
        let mut diffuse_map_filename = None;
        for line in mtl_parser {
            if let MtlLine::DiffuseMap(diffuse_map) = line {
                diffuse_map_filename = Some(diffuse_map);
                continue;
            }
        }
        Ok(Mtl {
            diffuse_map_filename,
        })
    }
}

pub struct Obj {
    pub comments: Vec<String>,
    pub objects: Vec<ObjObject>,
}

impl Obj {
    pub fn load(obj_file: impl AsRef<Path>) -> Result<Self, ObjError> {
        let obj_file = File::open(obj_file).map_err(ObjError::UnableToLoad)?;
        let reader = BufReader::new(obj_file);
        Self::from_reader(reader)
    }

    pub fn from_reader<T>(reader: BufReader<T>) -> Result<Self, ObjError>
    where
        T: std::io::Read,
    {
        let parser = ObjParser::new(reader);

        let mut comments = Vec::new();
        let mut objects = Vec::new();
        let mut object = ObjObject::default();

        for line in parser {
            match line {
                ObjLine::ObjectName(name) => {
                    // new object encountered, when multiple objects exist
                    if object.name.is_some() {
                        objects.push(object);
                        object = ObjObject::default();
                    }
                    object.name = Some(name);
                }
                ObjLine::MtlLib(name) => {
                    object.material = Some(name);
                }
                ObjLine::Vertex(..) => object.vertices.push(line),
                ObjLine::VertexParam(..) => object.vertex_params.push(line),
                ObjLine::Face(..) => object.faces.push(line),
                ObjLine::Normal(..) => object.normals.push(line),
                ObjLine::TextureUVW(..) => object.texture_coords.push(line),
                ObjLine::Comment(comment) => comments.push(comment),
                _ => {}
            }
        }
        objects.push(object);
        Ok(Obj { comments, objects })
    }
}

#[derive(Debug, Default)]
pub struct ObjObject {
    pub name: Option<String>,
    pub material: Option<String>,
    pub vertices: Vec<ObjLine>,
    pub normals: Vec<ObjLine>,
    pub texture_coords: Vec<ObjLine>,
    pub vertex_params: Vec<ObjLine>,
    pub faces: Vec<ObjLine>,
}

#[derive(Debug)]
pub struct ObjMaterial {
    pub diffuse_map: String,
}

impl ObjObject {
    pub fn vertices(&self) -> &Vec<ObjLine> {
        &self.vertices
    }
    pub fn vertex_params(&self) -> &Vec<ObjLine> {
        &self.vertex_params
    }
    pub fn normals(&self) -> &Vec<ObjLine> {
        &self.normals
    }
    pub fn texture_coords(&self) -> &Vec<ObjLine> {
        &self.texture_coords
    }

    fn get_v_tuple(&self, face_index: &FaceIndex) -> (f32, f32, f32, f32) {
        let &FaceIndex(ix, _, _) = face_index;
        match self.vertices[(ix as usize) - 1] {
            ObjLine::Vertex(x, y, z, w) => (x, y, z, w.unwrap_or(1.0)),
            _ => panic!("not a vertex"),
        }
    }

    fn get_vt_tuple(&self, face_index: &FaceIndex) -> (f32, f32, f32) {
        let &FaceIndex(_, vt, _) = face_index;
        match vt {
            Some(vt) => match self.texture_coords[(vt as usize) - 1] {
                ObjLine::TextureUVW(u, v, w) => (u, v, w.unwrap_or(0.0)),
                _ => panic!("not a vertex"),
            },
            None => (0.0, 0.0, 0.0),
        }
    }

    fn get_vn_tuple(&self, face_index: &FaceIndex) -> (f32, f32, f32) {
        let &FaceIndex(_, _, vn) = face_index;
        match vn {
            Some(vn) => match self.normals[(vn as usize) - 1] {
                ObjLine::Normal(x, y, z) => (x, y, z),
                _ => panic!("not a vertex"),
            },
            None => (0.0, 0.0, 0.0),
        }
    }

    fn interleave_tuples(&self, id: &FaceIndex) -> (T4<f32>, T3<f32>, T3<f32>) {
        let vert = self.get_v_tuple(id);
        let mut text = self.get_vt_tuple(id);

        // Adjust v texture coordinate as .obj and vulkan use different systems
        text.1 = 1.0 - text.1;
        let norm = self.get_vn_tuple(id);
        (vert, text, norm)
    }

    // TODO contiguous array of vertices
    pub fn interleaved(&self) -> Interleaved {
        let mut vertices = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        for (index, line) in self.faces.iter().enumerate() {
            let idx = index as u32;
            match line {
                ObjLine::Face(ref id1, ref id2, ref id3) => {
                    let vert1 = self.interleave_tuples(id1);
                    let vert2 = self.interleave_tuples(id2);
                    let vert3 = self.interleave_tuples(id3);
                    vertices.push(vert1);
                    vertices.push(vert2);
                    vertices.push(vert3);
                    indices.push(idx * 3);
                    indices.push(idx * 3 + 1);
                    indices.push(idx * 3 + 2);
                }
                _ => {}
            }
        }
        Interleaved { vertices, indices }
    }
}

pub struct Interleaved {
    pub vertices: Vec<(T4<f32>, T3<f32>, T3<f32>)>,
    pub indices: Vec<u32>,
}

#[cfg(test)]
mod tests {

    use std::error::Error;

    use super::*;

    #[test]
    fn negative_texcoord_plane_regression() -> Result<(), Box<dyn Error>> {
        use std::io::Cursor;
        let plane_lines = "mtllib untitled.mtl
o Plane
v -1.000000 0.000000 1.000000
v 1.000000 0.000000 1.000000
v -1.000000 0.000000 -1.000000
v 1.000000 0.000000 -1.000000
vt 1.000000 0.000000
vt 0.000000 1.000000
vt 0.000000 0.000000
vt 1.000000 1.000000
vn 0.0000 1.0000 0.0000
# usemtl None
s off
f 2/1/1 3/2/1 1/3/1
f 2/1/1 4/4/1 3/2/1";

        let cursor = Cursor::new(plane_lines);
        let o = Obj::from_reader(BufReader::new(cursor))?;
        let interleaved = o.objects[0].interleaved();

        assert_eq!(o.objects[0].faces.len(), 2);
        assert_eq!(interleaved.vertices.len(), 4);

        for (_v, vt, _vn) in interleaved.vertices {
            assert!(vt.0 >= 0.0);
            assert!(vt.1 >= 0.0);
        }

        Ok(())
    }
}
