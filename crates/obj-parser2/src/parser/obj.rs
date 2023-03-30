use std::io::BufRead;
use std::str;

use nom::{
    branch::alt,
    bytes::complete::{tag, take_while, take_while1},
    character::complete::multispace0,
    combinator::{map, opt},
    sequence::{delimited, preceded, tuple},
    IResult,
};

/// http://paulbourke.net/dataformats/obj/
use crate::parser::common::*;

use crate::def_string_line;

#[derive(PartialEq, Eq, Debug)]
pub struct FaceIndex(pub u32, pub Option<u32>, pub Option<u32>);

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

def_string_line!(object_line, b"o", ObjLine, ObjectName);
def_string_line!(group_line, b"g", ObjLine, GroupName);
def_string_line!(mtllib_line, b"mtllib", ObjLine, MtlLib);
def_string_line!(usemtl_line, b"usemtl", ObjLine, UseMtl);
def_string_line!(s_line, b"s", ObjLine, SmoothShading);

fn vertex_line(input: &[u8]) -> IResult<&[u8], ObjLine> {
    map(
        delimited(tag(b"v"), float_triple_opt_4th, take_while(|c| c == b'\n')),
        |(x, y, z, w)| ObjLine::Vertex(x, y, z, w),
    )(input)
}

fn normal_line(input: &[u8]) -> IResult<&[u8], ObjLine> {
    map(
        delimited(tag(b"vn"), float_triple, take_while(|c| c == b'\n')),
        |(x, y, z)| ObjLine::Normal(x, y, z),
    )(input)
}

fn texcoord_line(input: &[u8]) -> IResult<&[u8], ObjLine> {
    map(
        delimited(tag(b"vt"), float_pair_opt_3rd, take_while(|c| c == b'\n')),
        |(u, v, w)| ObjLine::TextureUVW(u, v, w),
    )(input)
}

fn vertex_param_line(input: &[u8]) -> IResult<&[u8], ObjLine> {
    map(
        delimited(tag(b"vp"), float_triple, take_while(|c| c == b'\n')),
        |(x, y, z)| ObjLine::VertexParam(x, y, z),
    )(input)
}

fn comment_line(input: &[u8]) -> IResult<&[u8], ObjLine> {
    map(
        delimited(
            tag(b"#"),
            delimited(multispace0, take_while1(|c| c != b'\n'), multispace0),
            take_while(|c| c == b'\n'),
        ),
        |s: &[u8]| ObjLine::Comment(str::from_utf8(s).unwrap().trim().to_string()),
    )(input)
}

fn face_index(input: &[u8]) -> IResult<&[u8], FaceIndex> {
    let (input, v) = uint(input)?;
    let (input, uv) = opt(preceded(tag(b"/"), uint))(input)?;
    let (input, n) = opt(preceded(tag(b"/"), uint))(input)?;
    Ok((input, FaceIndex(v, uv, n)))
}

fn face_line(input: &[u8]) -> IResult<&[u8], ObjLine> {
    map(
        delimited(
            tag(b"f"),
            tuple((face_index, face_index, face_index)),
            take_while(|c| c == b'\n'),
        ),
        |(f1, f2, f3)| ObjLine::Face(f1, f2, f3),
    )(input)
}

pub fn parse_obj_line(input: &[u8]) -> IResult<&[u8], ObjLine> {
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

pub fn parse_obj<T: BufRead>(input: T) -> Result<Vec<ObjLine>, ()> {
    let lines = input
        .lines()
        .map(|l| l.unwrap())
        .collect::<Vec<String>>()
        .join("\n")
        .into_bytes();

    let mut result = Vec::new();
    let mut buf = lines.as_slice();
    loop {
        match parse_obj_line(buf) {
            Ok((rest, line)) => {
                buf = rest;
                result.push(line);
            }
            Err(_) => break,
        }
    }

    Ok(result)
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
                    let result = parse_obj_line(line.as_bytes());
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
    use std::error::Error;
    use std::fs::File;
    use std::io::BufReader;

    use super::*;

    #[test]
    fn can_parse_any_line() {
        let result =
            parse_obj_line("f 1/11/4 1/3/4 1/11/4  #this is an important face \n".as_bytes());
        let (_, line) = result.unwrap();
        assert_eq!(
            line,
            ObjLine::Face(
                FaceIndex(1, Some(11), Some(4)),
                FaceIndex(1, Some(3), Some(4)),
                FaceIndex(1, Some(11), Some(4))
            )
        );
    }

    #[test]
    fn can_ignore_comment_at_eol() {
        let ff = face_line("f 1/11/4 1/3/4 1/11/4  #this is an important face \n".as_bytes());
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex(1, Some(11), Some(4)),
                FaceIndex(1, Some(3), Some(4)),
                FaceIndex(1, Some(11), Some(4))
            )
        );
    }

    #[test]
    fn can_parse_face_line_1() {
        let ff = face_line("f 1/11/4 1/3/4 1/11/4  \n".as_bytes());
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex(1, Some(11), Some(4)),
                FaceIndex(1, Some(3), Some(4)),
                FaceIndex(1, Some(11), Some(4))
            )
        );
    }

    #[test]
    fn can_parse_face_line_2() {
        //
        let ff = face_line("f 1/3 2/62 4/3\n".as_bytes());
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex(1, Some(3), None),
                FaceIndex(2, Some(62), None),
                FaceIndex(4, Some(3), None),
            )
        );
    }

    #[test]
    fn can_parse_face_line_3() {
        let ff = face_line("f 1//4 1//4 1//11  \n".as_bytes());
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex(1, None, Some(4)),
                FaceIndex(1, None, Some(4)),
                FaceIndex(1, None, Some(11))
            )
        );
    }

    #[test]
    fn can_parse_face_line_4() {
        let ff = face_line("f 42 1 11  \n".as_bytes());
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex(42, None, None),
                FaceIndex(1, None, None),
                FaceIndex(11, None, None)
            )
        );
    }

    #[test]
    fn can_parse_face_line_5() {
        let ff = face_line("f 42/ 1/ 11/  \n".as_bytes());
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex(42, None, None),
                FaceIndex(1, None, None),
                FaceIndex(11, None, None)
            )
        );
    }

    #[test]
    fn can_parse_face_line_6() {
        let ff = face_line("f 42// 1// 11// \t \n".as_bytes());
        let (_, b) = ff.unwrap();
        assert_eq!(
            b,
            ObjLine::Face(
                FaceIndex(42, None, None),
                FaceIndex(1, None, None),
                FaceIndex(11, None, None)
            )
        );
    }

    #[test]
    fn can_parse_texcoord_line() {
        let vline = "vt -1.000000 -1.000000 \r\n".as_bytes();
        let v = texcoord_line(vline);
        let (_a, b) = v.unwrap();
        assert_eq!(b, ObjLine::TextureUVW(-1.0, -1.0, None));
    }

    #[test]
    fn can_parse_normal_line() {
        let vline = "vn -1.000000 -1.000000 1.000000  \r\n".as_bytes();
        let v = normal_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, ObjLine::Normal(-1.0, -1.0, 1.0));
    }

    #[test]
    #[should_panic]
    fn invalid_vertex_line_fails() {
        let vline = "vZZ -1.000000 -1.000000 1.000000 \r\n".as_bytes();
        let v = vertex_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, ObjLine::Vertex(-1.0, -1.0, 1.0, None));
    }

    #[test]
    fn can_parse_vertex_parameter_line() {
        let vline = "vp -1.000000 -1.000000 1.000000 \r\n".as_bytes();
        let v = vertex_param_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, ObjLine::VertexParam(-1.0, -1.0, 1.0));
    }

    #[test]
    fn can_parse_vertex_line_with_optional_w_value() {
        let vline = "v -1.000000 -1.000000 1.000000 42.000\r\n".as_bytes();
        let v = vertex_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, ObjLine::Vertex(-1.0, -1.0, 1.0, Some(42.0)));
    }

    #[test]
    fn can_parse_vertex_line() {
        let vline = "v -1.000000 -1.000000 1.000000 \r\n".as_bytes();
        let v = vertex_line(vline);
        let (_, b) = v.unwrap();
        assert_eq!(b, ObjLine::Vertex(-1.0, -1.0, 1.0, None));
    }

    #[test]
    fn can_parse_object_line() {
        let cmt = object_line("o someobject.999asdf.7 \n".as_bytes());
        let (_, b) = cmt.unwrap();
        assert_eq!(b, ObjLine::ObjectName("someobject.999asdf.7".to_string()));
    }

    #[test]
    fn can_parse_mtllib_line() {
        let cmt = mtllib_line("mtllib somelib \n".as_bytes());
        let (_, b) = cmt.unwrap();
        assert_eq!(b, ObjLine::MtlLib("somelib".to_string()));
    }

    #[test]
    fn can_parse_usemtl_line() {
        let cmt = usemtl_line("usemtl SomeMaterial\n".as_bytes());
        let (_, b) = cmt.unwrap();
        assert_eq!(b, ObjLine::UseMtl("SomeMaterial".to_string()));
    }

    #[test]
    fn can_parse_s_line() {
        let cmt = s_line("s off\n".as_bytes());
        let (_, b) = cmt.unwrap();
        assert_eq!(b, ObjLine::SmoothShading("off".to_string()));
    }
}
