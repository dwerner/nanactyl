use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::ops::Index;
use std::path::Path;

use crate::parser::mtl::{MtlLine, MtlParser};
use crate::parser::obj::{FaceIndex, ObjLine, ObjParser};

type T3<T> = (T, T, T);
type T4<T> = (T, T, T, T);

#[derive(thiserror::Error, Debug)]
pub enum MtlError {
    #[error("unable to load mtl")]
    UnableToLoad(io::Error),
    #[error("all 3 specular components must be provided")]
    NotAllSpecularComponentsProvided,
}

#[derive(thiserror::Error, Debug)]
pub enum ObjError {
    #[error("unable to load obj")]
    UnableToLoad(io::Error),

    #[error("obj missing vertex data {0}")]
    MissingVertexData(u32),
    #[error("obj missing texcoord data {0}")]
    MissingTexcoordData(u32),
    #[error("obj missing vertex normal data {0}")]
    MissingNormalData(u32),
}

pub struct Mtl {
    pub diffuse_map_filename: Option<String>,
    pub bump_map_path: Option<String>,
    pub specular_map: Option<SpecularMap>,
}

pub struct SpecularMap {
    pub specular_map_path: String,
    pub specular_color: T3<f32>,
    pub exponent: f32,
}

impl SpecularMap {
    pub fn new(specular_map_path: String, specular_color: T3<f32>, exponent: f32) -> Self {
        Self {
            specular_map_path,
            specular_color,
            exponent,
        }
    }
}

impl Mtl {
    pub fn load(mtl_file: impl AsRef<Path>) -> Result<Self, MtlError> {
        let file = File::open(&mtl_file).map_err(MtlError::UnableToLoad)?;
        let reader = BufReader::new(file);
        Self::from_reader(reader)
    }

    pub fn from_reader<T>(reader: T) -> Result<Self, MtlError>
    where
        T: std::io::Read,
    {
        let reader = BufReader::new(reader);
        let mtl_parser = MtlParser::new(reader);

        let mut diffuse_map_filename = None;
        let mut bump_map_path = None;
        let mut specular_map_path = None;
        let mut specular_color = None;
        let mut specular_exponent = None;

        for line in mtl_parser {
            match line {
                MtlLine::DiffuseMap(diffuse_map) => {
                    diffuse_map_filename = Some(diffuse_map);
                }
                MtlLine::BumpMap(bump_map) => {
                    bump_map_path = Some(bump_map);
                }
                MtlLine::SpecularMap(spec_map) => {
                    specular_map_path = Some(spec_map);
                }
                MtlLine::SpecularColor(r, g, b) => {
                    specular_color = Some((r, g, b));
                }
                MtlLine::SpecularExponent(e) => {
                    specular_exponent = Some(e);
                }
                _ => {}
            }
        }

        let specular_map = match (specular_map_path, specular_color, specular_exponent) {
            (Some(path), Some(color), Some(exp)) => Some(SpecularMap::new(path, color, exp)),
            (None, None, None) => None,
            _ => return Err(MtlError::NotAllSpecularComponentsProvided),
        };

        Ok(Mtl {
            diffuse_map_filename,
            bump_map_path,
            specular_map,
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
                ObjLine::Face(..) => object.face_lines.push(line),
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
    pub face_lines: Vec<ObjLine>,
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

    fn get_vn_by_index(&self, vn_index: Option<u32>) -> Result<T3<f32>, ObjError> {
        Ok(match vn_index {
            Some(vn_index) => match self.normals.get((vn_index as usize) - 1) {
                Some(ObjLine::Normal(x, y, z)) => (*x, *y, *z),
                _ => return Err(ObjError::MissingNormalData(vn_index)),
            },
            None => (0.0, 0.0, 0.0),
        })
    }

    fn get_vt_by_index(&self, vt_index: Option<u32>) -> Result<T3<f32>, ObjError> {
        Ok(match vt_index {
            Some(vt_index) => {
                match self.texture_coords.get((vt_index as usize) - 1) {
                    // Adjust v texture coordinate as .obj and vulkan use different systems
                    Some(ObjLine::TextureUVW(u, v, w)) => (*u, 1.0 - v, w.unwrap_or(0.0)),
                    _ => return Err(ObjError::MissingTexcoordData(vt_index)),
                }
            }
            None => (0.0, 0.0, 0.0),
        })
    }

    fn get_vertex_by_index(&self, vertex_index: u32) -> Result<T4<f32>, ObjError> {
        let vert = match self.vertices.get((vertex_index as usize) - 1) {
            Some(ObjLine::Vertex(x, y, z, w)) => (*x, *y, *z, w.unwrap_or(1.0)),
            _ => return Err(ObjError::MissingVertexData(vertex_index)),
        };
        Ok(vert)
    }

    // TODO contiguous array of vertices
    pub fn interleaved(&self) -> Result<Interleaved, ObjError> {
        let mut vertices = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut seen_vertices = Vec::new();
        for line in self.face_lines.iter() {
            println!("face line: {:?}", line);
            match line {
                ObjLine::Face(id1, id2, id3) => {
                    for face in [id1, id2, id3] {
                        println!("face: {}", face);
                        if !seen_vertices.contains(&(face.v - 1)) {
                            println!("new vertex found {}", face.v);
                            let vert = self.get_vertex_by_index(face.v)?;
                            let text = self.get_vt_by_index(face.vt)?;
                            let norm = self.get_vn_by_index(face.vn)?;
                            seen_vertices.push(id1.v - 1);
                            let vertex_data = (vert, text, norm);
                            vertices.push(vertex_data);
                        }
                        indices.push(face.v - 1);
                        println!("indices: {:?}", indices);
                    }
                }
                _ => {}
            }
        }
        Ok(Interleaved { vertices, indices })
    }
}

pub struct Interleaved {
    pub vertices: Vec<(T4<f32>, T3<f32>, T3<f32>)>,
    pub indices: Vec<u32>,
}

#[cfg(test)]
mod tests {

    use std::error::Error;
    use std::io::Cursor;

    use super::*;

    #[test]
    fn test_interleaved_order() -> Result<(), Box<dyn Error>> {
        let obj_data = "o Object
v 0.0 0.0 0.0
v 1.0 0.0 0.0
v 0.0 1.0 0.0
vn 0.0 0.0 0.2
vn 0.0 0.2 0.0
vn 0.2 0.0 0.0
vt 0.0 0.0 0.3
vt 0.3 0.0 0.0
vt 0.0 0.3
f 1/1/1 2/2/2 3/3/3";

        let cursor = Cursor::new(obj_data);
        let o = Obj::from_reader(BufReader::new(cursor))?;
        let interleaved = o.objects[0].interleaved().unwrap();

        let expected_vertices = vec![
            ((0.0, 0.0, 0.0, 1.0), (0.0, 0.0, 0.3), (0.0, 0.0, 0.2)),
            ((1.0, 0.0, 0.0, 1.0), (0.3, 0.0, 0.0), (0.0, 0.2, 0.0)),
            ((0.0, 1.0, 0.0, 1.0), (0.0, 0.3, 0.0), (0.2, 0.0, 0.0)),
        ];

        assert_eq!(interleaved.vertices, expected_vertices);

        Ok(())
    }

    #[test]
    fn test_mtl_loading() -> Result<(), Box<dyn Error>> {
        let mtl_data = "newmtl material_name
map_Kd diffuse_map.png
map_bump bump_map.png
map_Ks specular_map.png
Ns 10.0
Ks 1.0 1.0 1.0";

        let cursor = Cursor::new(mtl_data);
        let mtl = Mtl::from_reader(cursor)?;

        assert_eq!(
            mtl.diffuse_map_filename,
            Some("diffuse_map.png".to_string())
        );
        assert_eq!(mtl.bump_map_path, Some("bump_map.png".to_string()));
        assert_eq!(
            mtl.specular_map.as_ref().map(|s| &s.specular_map_path),
            Some(&"specular_map.png".to_string())
        );
        assert_eq!(
            mtl.specular_map.as_ref().map(|s| s.specular_color),
            Some((1.0, 1.0, 1.0))
        );
        assert_eq!(mtl.specular_map.as_ref().map(|s| s.exponent), Some(10.0));

        Ok(())
    }

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
        let interleaved = o.objects[0].interleaved().unwrap();

        assert_eq!(o.objects[0].face_lines.len(), 2);
        assert_eq!(interleaved.vertices.len(), 4);

        for (_v, vt, _vn) in interleaved.vertices {
            assert!(vt.0 >= 0.0);
            assert!(vt.1 >= 0.0);
        }

        Ok(())
    }
}
