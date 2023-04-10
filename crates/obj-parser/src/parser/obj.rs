use std::fmt::{Display, Formatter};
use std::io::BufRead;
use std::str;

use nom::branch::alt;
use nom::bytes::complete::{tag, take_while, take_while1};
use nom::character::complete::{multispace0, multispace1, space1};
use nom::combinator::{map, opt};
use nom::sequence::{delimited, preceded, tuple};
use nom::IResult;

use crate::def_string_line;
/// http://paulbourke.net/dataformats/obj/
use crate::parser::common::*;

#[derive(PartialEq, Eq, Debug, Hash, Clone)]
pub struct FaceIndex {
    pub v: u32,
    pub vt: Option<u32>,
    pub vn: Option<u32>,
}

impl FaceIndex {
    pub fn new(v: u32, vt: Option<u32>, vn: Option<u32>) -> Self {
        Self { v, vt, vn }
    }
}

impl Display for FaceIndex {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/", self.v)?;
        if let Some(v) = self.vt {
            write!(f, "{}/", v)?;
        }
        if let Some(v) = self.vn {
            write!(f, "{}", v)?;
        }
        Ok(())
    }
}

#[derive(PartialEq, Debug)]
pub enum ObjLine {
    Comment(String),
    ObjectName(String),
    GroupName(String),
    MtlLib(String),
    UseMtl(String),
    SmoothShading(String),
    Vertex(f32, f32, f32, Option<f32>), // x, y, z, then w defaults to 1.0
    VertexParam(f32, f32, f32),
    Normal(f32, f32, f32),
    Face(FaceIndex, FaceIndex, FaceIndex),
    TextureUVW(f32, f32, Option<f32>), // u,v, then w defaults to 0.0
}

impl Display for ObjLine {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ObjLine::Comment(s) => write!(f, "# {}", s),
            ObjLine::ObjectName(s) => write!(f, "o {}", s),
            ObjLine::GroupName(s) => write!(f, "g {}", s),
            ObjLine::MtlLib(s) => write!(f, "mtllib {}", s),
            ObjLine::UseMtl(s) => write!(f, "usemtl {}", s),
            ObjLine::SmoothShading(s) => write!(f, "s {}", s),
            ObjLine::Vertex(x, y, z, w) => {
                write!(f, "v {} {} {}", x, y, z)?;
                if let Some(w) = w {
                    write!(f, " {}", w)?;
                }
                Ok(())
            }
            ObjLine::VertexParam(u, v, w) => write!(f, "vp {} {} {}", u, v, w),
            ObjLine::Normal(x, y, z) => write!(f, "vn {} {} {}", x, y, z),
            ObjLine::Face(a, b, c) => write!(f, "f {} {} {}", a, b, c),
            ObjLine::TextureUVW(u, v, w) => {
                write!(f, "vt {} {}", u, v)?;
                if let Some(w) = w {
                    write!(f, " {}", w)?;
                }
                Ok(())
            }
        }
    }
}

def_string_line!(object_line, "o", ObjLine, ObjectName);
def_string_line!(group_line, "g", ObjLine, GroupName);
def_string_line!(mtllib_line, "mtllib", ObjLine, MtlLib);
def_string_line!(usemtl_line, "usemtl", ObjLine, UseMtl);
def_string_line!(s_line, "s", ObjLine, SmoothShading);

fn vertex_line(input: &str) -> IResult<&str, ObjLine> {
    map(
        delimited(tag("v"), float_triple_opt_4th, take_while(|c| c == '\n')),
        |(x, y, z, w)| ObjLine::Vertex(x, y, z, w),
    )(input)
}

fn normal_line(input: &str) -> IResult<&str, ObjLine> {
    map(
        delimited(tag("vn"), float_triple, take_while(|c| c == '\n')),
        |(x, y, z)| ObjLine::Normal(x, y, z),
    )(input)
}

fn texcoord_line(input: &str) -> IResult<&str, ObjLine> {
    map(
        delimited(tag("vt"), float_pair_opt_3rd, take_while(|c| c == '\n')),
        |(u, v, w)| ObjLine::TextureUVW(u, v, w),
    )(input)
}

fn vertex_param_line(input: &str) -> IResult<&str, ObjLine> {
    map(
        delimited(tag("vp"), float_triple, take_while(|c| c == '\n')),
        |(x, y, z)| ObjLine::VertexParam(x, y, z),
    )(input)
}

fn comment_line(input: &str) -> IResult<&str, ObjLine> {
    map(
        delimited(
            tag("#"),
            delimited(multispace0, take_while1(|c| c != '\n'), multispace0),
            take_while(|c| c == '\n'),
        ),
        |s: &str| ObjLine::Comment(s.trim().to_string()),
    )(input)
}

fn face_index(input: &str) -> IResult<&str, FaceIndex> {
    let (input, v) = unsigned_integer(input)?;
    let (input, vt) = opt(preceded(tag("/"), unsigned_integer))(input)?;
    let (input, vn) = opt(preceded(tag("/"), unsigned_integer))(input)?;
    Ok((input, FaceIndex { v, vt, vn }))
}

fn face_triple(input: &str) -> IResult<&str, FaceIndex> {
    let (input, v) = unsigned_integer(input)?;
    let (input, _) = tag("/")(input)?;
    let (input, vt) = opt(unsigned_integer)(input)?;
    let (input, _) = tag("/")(input)?;
    let (input, vn) = opt(unsigned_integer)(input)?;

    Ok((input, FaceIndex { v, vt, vn }))
}

fn face_pair(input: &str) -> IResult<&str, FaceIndex> {
    let (input, v) = unsigned_integer(input)?;
    let (input, _) = tag("/")(input)?;
    let (input, vt) = opt(unsigned_integer)(input)?;
    Ok((input, FaceIndex { v, vt, vn: None }))
}
pub(crate) fn spaced_face_item(input: &str) -> IResult<&str, FaceIndex> {
    let (i, _) = multispace0(input)?;
    let (i, face) = alt((face_triple, face_pair, face_index))(i)?;
    let (i, _) = multispace0(i)?;
    Ok((i, face))
}

pub(crate) fn spaced_face(input: &str) -> IResult<&str, (FaceIndex, FaceIndex, FaceIndex)> {
    let (i, _) = multispace0(input)?;
    let (i, face) = tuple((spaced_face_item, spaced_face_item, spaced_face_item))(i)?;
    let (i, _) = multispace0(i)?;
    Ok((i, face))
}

fn face_line(input: &str) -> IResult<&str, ObjLine> {
    let (input, _) = delimited(opt(multispace1), tag("f"), space1)(input)?;
    let (input, face) = spaced_face(input)?;
    Ok((input, ObjLine::Face(face.0, face.1, face.2)))
}

pub(crate) fn parse_obj_line(input: &str) -> IResult<&str, ObjLine> {
    alt((
        comment_line,
        object_line,
        group_line,
        mtllib_line,
        usemtl_line,
        s_line,
        vertex_line,
        normal_line,
        texcoord_line,
        vertex_param_line,
        face_line,
    ))(input)
}

pub struct ObjParser<R> {
    reader: R,
}

impl<R> ObjParser<R>
where
    R: BufRead,
{
    pub fn new(reader: R) -> Self {
        ObjParser { reader }
    }
}

impl<R> Iterator for ObjParser<R>
where
    R: BufRead,
{
    type Item = ObjLine;

    fn next(&mut self) -> Option<Self::Item> {
        let mut line = String::new();
        let read_result = self.reader.read_line(&mut line);
        match read_result {
            Ok(len) => {
                if len > 0 {
                    let result = parse_obj_line(&line);
                    match result {
                        Ok((_, o)) => Some(o),
                        Err(_) => None,
                    }
                } else {
                    None
                }
            }
            Err(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {

    use std::io::{BufReader, Cursor};

    use super::*;

    #[test]
    fn test_face_index() {
        assert_eq!(
            face_index("1/2/3"),
            Ok(("", FaceIndex::new(1, Some(2), Some(3))))
        );
        assert_eq!(
            face_index("4/5"),
            Ok(("", FaceIndex::new(4, Some(5), None)))
        );
        assert_eq!(face_index("6"), Ok(("", FaceIndex::new(6, None, None))));
    }

    #[test]
    fn test_face_pair() {
        assert_eq!(
            spaced_face_item("1/2"),
            Ok(("", FaceIndex::new(1, Some(2), None)))
        );
        assert_eq!(
            spaced_face_item("3"),
            Ok(("", FaceIndex::new(3, None, None)))
        );
    }

    #[test]
    fn can_parse_any_line() {
        let result = parse_obj_line("f 1/11/4 1/3/4 1/11/4  #this is an important face \n");
        let (_, line) = result.unwrap();
        assert_eq!(
            line,
            ObjLine::Face(
                FaceIndex::new(1, Some(11), Some(4)),
                FaceIndex::new(1, Some(3), Some(4)),
                FaceIndex::new(1, Some(11), Some(4))
            )
        );
    }

    #[test]
    fn can_ignore_comment_at_eol() {
        let ff = face_line("f 1/11/4 1/3/4 1/11/4  #this is an important face \n");
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex::new(1, Some(11), Some(4)),
                FaceIndex::new(1, Some(3), Some(4)),
                FaceIndex::new(1, Some(11), Some(4))
            )
        );
    }

    #[test]
    fn can_parse_face_line_1() {
        let ff = face_line("f 1/11/4 1/3/4 1/11/4  \n");
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex::new(1, Some(11), Some(4)),
                FaceIndex::new(1, Some(3), Some(4)),
                FaceIndex::new(1, Some(11), Some(4))
            )
        );
    }

    #[test]
    fn can_parse_face_line_2() {
        //
        let ff = face_line("f 1/3 2/62 4/3\n");
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex::new(1, Some(3), None),
                FaceIndex::new(2, Some(62), None),
                FaceIndex::new(4, Some(3), None),
            )
        );
    }

    #[test]
    fn can_parse_face_line_3() {
        let ff = face_line("f 1//4 1//4 1//11  \n");
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex::new(1, None, Some(4)),
                FaceIndex::new(1, None, Some(4)),
                FaceIndex::new(1, None, Some(11))
            )
        );
    }

    #[test]
    fn can_parse_face_line_4() {
        let ff = face_line("f 42 1 11  \n");
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex::new(42, None, None),
                FaceIndex::new(1, None, None),
                FaceIndex::new(11, None, None)
            )
        );
    }

    #[test]
    fn can_parse_face_line_5() {
        let ff = face_line("f 42/ 1/ 11/  \n");
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex::new(42, None, None),
                FaceIndex::new(1, None, None),
                FaceIndex::new(11, None, None)
            )
        );
    }

    #[test]
    fn can_parse_face_line_6() {
        let ff = face_line("f 42// 1// 11// \t \n");
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex::new(42, None, None),
                FaceIndex::new(1, None, None),
                FaceIndex::new(11, None, None)
            )
        );
    }

    #[test]
    fn can_parse_texcoord_line() {
        let vline = "vt -1.000000 -1.000000 \r\n";
        let v = texcoord_line(vline);
        let (_a, b) = v.unwrap();
        assert_eq!(b, ObjLine::TextureUVW(-1.0, -1.0, None));
    }

    #[test]
    fn can_parse_normal_line() {
        let vline = "vn -1.000000 -1.000000 1.000000  \r\n";
        let v = normal_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, ObjLine::Normal(-1.0, -1.0, 1.0));
    }

    #[test]
    fn can_parse_vertex_parameter_line() {
        let vline = "vp -1.000000 -1.000000 1.000000 \r\n";
        let v = vertex_param_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, ObjLine::VertexParam(-1.0, -1.0, 1.0));
    }

    #[test]
    fn can_parse_vertex_line_with_optional_w_value() {
        let vline = "v -1.000000 -1.000000 1.000000 42.000\r\n";
        let v = vertex_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, ObjLine::Vertex(-1.0, -1.0, 1.0, Some(42.0)));
    }

    #[test]
    fn can_parse_vertex_line() {
        let vline = "v -1.000000 -1.000000 1.000000 \r\n";
        let v = vertex_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, ObjLine::Vertex(-1.0, -1.0, 1.0, None));
    }

    #[test]
    fn can_parse_object_line() {
        let cmt = object_line("o someobject.999asdf.7 \n");
        let (_, b) = cmt.unwrap();
        assert_eq!(b, ObjLine::ObjectName("someobject.999asdf.7".to_string()));
    }

    #[test]
    fn can_parse_mtllib_line() {
        let cmt = mtllib_line("mtllib somelib \n");
        let (_, b) = cmt.unwrap();
        assert_eq!(b, ObjLine::MtlLib("somelib".to_string()));
    }

    #[test]
    fn can_parse_usemtl_line() {
        let cmt = usemtl_line("usemtl SomeMaterial\n");
        let (_, b) = cmt.unwrap();
        assert_eq!(b, ObjLine::UseMtl("SomeMaterial".to_string()));
    }

    #[test]
    fn can_parse_s_line() {
        let cmt = s_line("s off\n");
        let (_, b) = cmt.unwrap();
        assert_eq!(b, ObjLine::SmoothShading("off".to_string()));
    }

    #[test]
    fn test_obj_line_display() {
        let cases = vec![
            (
                ObjLine::Comment("This is a comment".to_string()),
                "# This is a comment",
            ),
            (ObjLine::ObjectName("Cube".to_string()), "o Cube"),
            (ObjLine::GroupName("Group1".to_string()), "g Group1"),
            (
                ObjLine::MtlLib("material.mtl".to_string()),
                "mtllib material.mtl",
            ),
            (ObjLine::UseMtl("Material".to_string()), "usemtl Material"),
            (ObjLine::SmoothShading("1".to_string()), "s 1"),
            (ObjLine::Vertex(1.0, 2.0, 3.0, Some(1.0)), "v 1 2 3 1"),
            (ObjLine::VertexParam(0.0, 0.5, 1.0), "vp 0 0.5 1"),
            (ObjLine::Normal(1.0, 0.0, 0.0), "vn 1 0 0"),
            (
                ObjLine::Face(
                    FaceIndex::new(1, Some(2), Some(3)),
                    FaceIndex::new(4, Some(5), Some(6)),
                    FaceIndex::new(7, Some(8), Some(9)),
                ),
                "f 1/2/3 4/5/6 7/8/9",
            ),
            (ObjLine::TextureUVW(0.0, 1.0, Some(0.5)), "vt 0 1 0.5"),
        ];

        for (obj_line, expected_output) in cases {
            assert_eq!(format!("{}", obj_line), expected_output);
        }
    }

    const CUBE_OBJ_TEXT: &str = "# Blender 3.4.1
mtllib cube.mtl
o Cube
v 1 -1 -1
v 1 1 -1
v 1 -1 1
v 1 1 1
v -1 -1 -1
v -1 1 -1
v -1 -1 1
v -1 1 1
vn 1 -0 -0
vn -0 -0 1
vn -1 -0 -0
vn -0 -0 -1
vn -0 -1 -0
vn -0 1 -0
vt 0.375 0
vt 0.375 1
vt 0.125 0.75
vt 0.625 0
vt 0.625 1
vt 0.875 0.75
vt 0.125 0.5
vt 0.375 0.25
vt 0.625 0.25
vt 0.875 0.5
vt 0.375 0.75
vt 0.625 0.75
vt 0.375 0.5
vt 0.625 0.5
s 0
usemtl Material.001
f 2/4/1 3/8/1 1/1/1
f 4/9/2 7/13/2 3/8/2
f 8/14/3 5/11/3 7/13/3
f 6/12/4 1/2/4 5/11/4
f 7/13/5 1/3/5 3/7/5
f 4/10/6 6/12/6 8/14/6
f 2/4/1 4/9/1 3/8/1
f 4/9/2 8/14/2 7/13/2
f 8/14/3 6/12/3 5/11/3
f 6/12/4 2/5/4 1/2/4
f 7/13/5 5/11/5 1/3/5
f 4/10/6 2/6/6 6/12/6
";

    #[test]
    fn obj_parse_roundtrip() {
        let mut obj = {
            let cursor = Cursor::new(CUBE_OBJ_TEXT);
            let reader = BufReader::new(cursor);
            ObjParser::new(reader)
        };
        let cursor = Cursor::new(CUBE_OBJ_TEXT);
        let reader = BufReader::new(cursor);
        for line in reader.lines() {
            let line = line.unwrap();
            if let Some(obj_line) = obj.next() {
                let obj_text_line = obj_line.to_string();
                assert_eq!(obj_text_line.trim(), line.trim());
            }
        }
    }
}
